use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use image::DynamicImage;
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tracing::{debug, info, trace, warn};

use super::VmProviderTrait;
use crate::automation::vm::{VmInstance, VmState};
use crate::utils::net;

pub struct VirtualBoxProvider;

impl VirtualBoxProvider {
    pub fn new() -> Self {
        Self
    }

    fn vboxmanage_cmd(&self) -> Command {
        #[cfg(windows)]
        {
            Command::new("VBoxManage.exe")
        }
        #[cfg(unix)]
        {
            Command::new("VBoxManage")
        }
    }

    async fn vm_exists(&self, vm_name: &str) -> Result<bool> {
        let output = self
            .vboxmanage_cmd()
            .args(["list", "vms"])
            .output()
            .context("Failed to list VirtualBox VMs")?;

        if !output.status.success() {
            return Err(anyhow!(
                "Failed to list VMs: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let output_str = String::from_utf8_lossy(&output.stdout);
        Ok(output_str.contains(&format!("\"{vm_name}\"")))
    }
}

#[async_trait]
impl VmProviderTrait for VirtualBoxProvider {
    fn get_ssh_endpoint(&self, instance: &VmInstance) -> (String, u16) {
        // Try to get the VM's real IP using VBoxManage guestproperty
        let output = self
            .vboxmanage_cmd()
            .args([
                "guestproperty",
                "get",
                &instance.name,
                "/VirtualBox/GuestInfo/Net/0/V4/IP",
            ])
            .output();
        if let Ok(output) = output {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Some(ip) = stdout.strip_prefix("Value: ") {
                    let ip = ip.trim().to_string();
                    if !ip.is_empty() && ip != "null" {
                        return (ip, 22);
                    }
                }
            }
        }
        // Fallback to 127.0.0.1 and forwarded port
        (
            "127.0.0.1".to_string(),
            instance.config.network_config.ssh_port,
        )
    }
    async fn create_vm(&self, instance: &mut VmInstance) -> Result<()> {
        info!("Creating VirtualBox VM: {}", instance.name);

        // Check if VM already exists
        if self.vm_exists(&instance.name).await? {
            info!(
                "VirtualBox VM {} already exists, skipping creation",
                instance.name
            );
            instance.set_state(VmState::Stopped);
            return Ok(());
        }

        // Create VM
        let output = self
            .vboxmanage_cmd()
            .args([
                "createvm",
                "--name",
                &instance.name,
                "--ostype",
                "Linux_64", // Default, could be configurable
                "--register",
            ])
            .output()
            .context("Failed to execute VBoxManage createvm")?;

        if !output.status.success() {
            return Err(anyhow!(
                "Failed to create VirtualBox VM: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // Configure VM settings
        let configs = [
            ("--memory", instance.config.memory_mb.to_string()),
            ("--cpus", instance.config.cpus.to_string()),
            ("--vram", "128".to_string()),
            ("--boot1", "dvd".to_string()),
            ("--boot2", "disk".to_string()),
            ("--acpi", "on".to_string()),
            ("--ioapic", "on".to_string()),
            ("--rtcuseutc", "on".to_string()),
        ];

        for (key, value) in &configs {
            let output = self
                .vboxmanage_cmd()
                .args(["modifyvm", &instance.name, key, value])
                .output()
                .context("Failed to configure VM")?;

            if !output.status.success() {
                return Err(anyhow!(
                    "Failed to configure VM setting {}: {}",
                    key,
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        }

        // Configure network adapter (NAT with port forwarding for SSH)
        let output = self
            .vboxmanage_cmd()
            .args([
                "modifyvm",
                &instance.name,
                "--nic1",
                "nat",
                "--nictype1",
                "82540EM",
                "--cableconnected1",
                "on",
            ])
            .output()
            .context("Failed to configure network adapter")?;
        if !output.status.success() {
            return Err(anyhow!(
                "Failed to configure network adapter: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // Find a random unoccupied port for SSH forwarding
        let ssh_host_port = net::find_free_port()
            .ok_or_else(|| anyhow!("No free port found for SSH forwarding"))?;
        // Store the port in the VM config for later use
        instance.config.network_config.ssh_port = ssh_host_port;

        // Set up port forwarding for SSH (host port to guest 22)
        let output = self
            .vboxmanage_cmd()
            .args([
                "modifyvm",
                &instance.name,
                "--natpf1",
                &format!("ssh,tcp,,{ssh_host_port},,22"),
            ])
            .output()
            .context("Failed to set up port forwarding for SSH")?;
        if !output.status.success() {
            return Err(anyhow!(
                "Failed to set up port forwarding: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // Create and attach disk
        let disk_path = format!("{}.vdi", instance.name);

        let output = self
            .vboxmanage_cmd()
            .args([
                "createmedium",
                "disk",
                "--filename",
                &disk_path,
                "--size",
                &(instance.config.disk_size_gb * 1024).to_string(), // Convert to MB
                "--format",
                "VDI",
            ])
            .output()
            .context("Failed to create VM disk")?;

        if !output.status.success() {
            return Err(anyhow!(
                "Failed to create VirtualBox disk: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // Attach disk to VM
        let output = self
            .vboxmanage_cmd()
            .args([
                "storagectl",
                &instance.name,
                "--name",
                "SATA Controller",
                "--add",
                "sata",
                "--controller",
                "IntelAHCI",
            ])
            .output()
            .context("Failed to add SATA controller")?;

        if !output.status.success() {
            return Err(anyhow!(
                "Failed to add SATA controller: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let output = self
            .vboxmanage_cmd()
            .args([
                "storageattach",
                &instance.name,
                "--storagectl",
                "SATA Controller",
                "--port",
                "0",
                "--device",
                "0",
                "--type",
                "hdd",
                "--medium",
                &disk_path,
            ])
            .output()
            .context("Failed to attach disk")?;

        if !output.status.success() {
            return Err(anyhow!(
                "Failed to attach disk: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        instance.set_state(VmState::Stopped);
        Ok(())
    }

    async fn start_vm(&self, instance: &mut VmInstance) -> Result<()> {
        info!("Starting VirtualBox VM: {}", instance.name);

        if instance.is_running() {
            return Ok(());
        }

        // Ensure VM is created first
        if !self.vm_exists(&instance.name).await? {
            self.create_vm(instance).await?;
        }

        instance.set_state(VmState::Starting);

        let output = self
            .vboxmanage_cmd()
            .args(["startvm", &instance.name, "--type", "headless"])
            .output()
            .context("Failed to start VirtualBox VM")?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            instance.set_state(VmState::Error(error_msg.to_string()));
            return Err(anyhow!("Failed to start VirtualBox VM: {}", error_msg));
        }

        // Wait for VM to be running
        sleep(Duration::from_secs(5)).await;

        if self.is_running(instance).await? {
            instance.set_state(VmState::Running);
        } else {
            instance.set_state(VmState::Error("VM failed to start".to_string()));
            return Err(anyhow!("VM failed to start properly"));
        }

        Ok(())
    }

    async fn stop_vm(&self, instance: &mut VmInstance) -> Result<()> {
        info!("Stopping VirtualBox VM: {}", instance.name);

        if instance.is_stopped() {
            return Ok(());
        }

        instance.set_state(VmState::Stopping);

        // Try graceful shutdown first
        let output = self
            .vboxmanage_cmd()
            .args(["controlvm", &instance.name, "acpipowerbutton"])
            .output()
            .context("Failed to send ACPI power button")?;

        if output.status.success() {
            // Wait for graceful shutdown
            if timeout(Duration::from_secs(30), self.wait_for_shutdown(instance))
                .await
                .is_ok()
            {
                instance.set_state(VmState::Stopped);
                return Ok(());
            }
        }

        // Force power off
        let output = self
            .vboxmanage_cmd()
            .args(["controlvm", &instance.name, "poweroff"])
            .output()
            .context("Failed to power off VM")?;

        if !output.status.success() {
            return Err(anyhow!(
                "Failed to power off VM: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        instance.set_state(VmState::Stopped);
        Ok(())
    }

    async fn delete_vm(&self, instance: &mut VmInstance) -> Result<()> {
        info!("Deleting VirtualBox VM: {}", instance.name);

        // Stop VM first
        if !instance.is_stopped() {
            self.stop_vm(instance).await?;
        }

        // Unregister and delete VM
        let output = self
            .vboxmanage_cmd()
            .args(["unregistervm", &instance.name, "--delete"])
            .output()
            .context("Failed to delete VirtualBox VM")?;

        if !output.status.success() {
            return Err(anyhow!(
                "Failed to delete VM: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(())
    }

    async fn attach_iso(&self, instance: &mut VmInstance, iso_path: &Path) -> Result<()> {
        info!("Attaching ISO to VirtualBox VM: {}", iso_path.display());

        if !iso_path.exists() {
            return Err(anyhow!("ISO file does not exist: {}", iso_path.display()));
        }

        // Ensure VM is created first
        if !self.vm_exists(&instance.name).await? {
            self.create_vm(instance).await?;
        }

        // Create IDE controller if it doesn't exist
        let _ = self
            .vboxmanage_cmd()
            .args([
                "storagectl",
                &instance.name,
                "--name",
                "IDE Controller",
                "--add",
                "ide",
            ])
            .output();

        // Attach ISO
        let output = self
            .vboxmanage_cmd()
            .args([
                "storageattach",
                &instance.name,
                "--storagectl",
                "IDE Controller",
                "--port",
                "1",
                "--device",
                "0",
                "--type",
                "dvddrive",
                "--medium",
                iso_path.to_str().unwrap(),
            ])
            .output()
            .context("Failed to attach ISO")?;

        if !output.status.success() {
            return Err(anyhow!(
                "Failed to attach ISO: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        instance.set_iso_path(iso_path.to_path_buf());
        Ok(())
    }

    async fn detach_iso(&self, instance: &mut VmInstance) -> Result<()> {
        info!("Detaching ISO from VirtualBox VM");

        let output = self
            .vboxmanage_cmd()
            .args([
                "storageattach",
                &instance.name,
                "--storagectl",
                "IDE Controller",
                "--port",
                "1",
                "--device",
                "0",
                "--medium",
                "none",
            ])
            .output()
            .context("Failed to detach ISO")?;

        if !output.status.success() {
            return Err(anyhow!(
                "Failed to detach ISO: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        instance.iso_path = None;
        Ok(())
    }

    async fn create_snapshot(&self, instance: &VmInstance, snapshot_name: &str) -> Result<()> {
        info!("Creating VirtualBox snapshot: {}", snapshot_name);

        let output = self
            .vboxmanage_cmd()
            .args([
                "snapshot",
                &instance.name,
                "take",
                snapshot_name,
                "--description",
                &format!("Isotope snapshot: {snapshot_name}"),
            ])
            .output()
            .context("Failed to create snapshot")?;

        if !output.status.success() {
            return Err(anyhow!(
                "Failed to create snapshot: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(())
    }

    async fn restore_snapshot(&self, instance: &mut VmInstance, snapshot_name: &str) -> Result<()> {
        info!("Restoring VirtualBox snapshot: {}", snapshot_name);

        // VM must be stopped to restore snapshot
        if !instance.is_stopped() {
            self.stop_vm(instance).await?;
        }

        let output = self
            .vboxmanage_cmd()
            .args(["snapshot", &instance.name, "restore", snapshot_name])
            .output()
            .context("Failed to restore snapshot")?;

        if !output.status.success() {
            return Err(anyhow!(
                "Failed to restore snapshot: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(())
    }

    async fn is_running(&self, instance: &VmInstance) -> Result<bool> {
        let output = self
            .vboxmanage_cmd()
            .args(["showvminfo", &instance.name, "--machinereadable"])
            .output()
            .context("Failed to get VM info")?;

        if !output.status.success() {
            return Ok(false);
        }

        let output_str = String::from_utf8_lossy(&output.stdout);
        Ok(output_str.contains("VMState=\"running\""))
    }

    async fn wait_for_shutdown(&self, instance: &VmInstance) -> Result<()> {
        let timeout_duration = instance.config.timeout;
        let check_interval = Duration::from_secs(2);

        timeout(timeout_duration, async {
            loop {
                if !self.is_running(instance).await? {
                    break;
                }
                sleep(check_interval).await;
            }
            Ok::<(), anyhow::Error>(())
        })
        .await
        .context("Timeout waiting for VM shutdown")?
    }

    async fn send_keys(&self, instance: &VmInstance, keys: &[String]) -> Result<()> {
        debug!("Sending keys to VirtualBox VM: {:?}", keys);

        for key in keys {
            // Convert key to VirtualBox scancode format
            let scancodes = self.key_to_scancodes(key)?;

            let output = self
                .vboxmanage_cmd()
                .args(["controlvm", &instance.name, "keyboardputscancode"])
                .args(scancodes.iter().map(|s| s.as_str()))
                .output()
                .context("Failed to send keyboard input")?;

            if !output.status.success() {
                return Err(anyhow!(
                    "Failed to send key '{}': {}",
                    key,
                    String::from_utf8_lossy(&output.stderr)
                ));
            }

            sleep(Duration::from_millis(50)).await;
        }

        Ok(())
    }

    async fn capture_screen(&self, instance: &VmInstance) -> Result<DynamicImage> {
        trace!("=== VBOX SCREEN CAPTURE START ===");
        trace!("Capturing screen from VirtualBox VM: {}", instance.name);

        let screenshot_path = format!("{}-screenshot.png", instance.name);
        trace!("Screenshot will be saved to: {}", screenshot_path);

        let output = self
            .vboxmanage_cmd()
            .args([
                "controlvm",
                &instance.name,
                "screenshotpng",
                &screenshot_path,
            ])
            .output()
            .context("Failed to capture screenshot")?;

        trace!("VBoxManage screenshotpng exit code: {}", output.status);
        if !output.stdout.is_empty() {
            trace!(
                "VBoxManage stdout: {}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
        if !output.stderr.is_empty() {
            trace!(
                "VBoxManage stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        if !output.status.success() {
            return Err(anyhow!(
                "Failed to capture screenshot: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // Wait a moment for the file to be written
        sleep(Duration::from_millis(300)).await;

        // Check if file exists and get its size
        if let Ok(metadata) = std::fs::metadata(&screenshot_path) {
            trace!(
                "Screenshot file created successfully. Size: {} bytes",
                metadata.len()
            );
            if metadata.len() == 0 {
                warn!("Screenshot file is empty (0 bytes)");
            }
        } else {
            warn!("Screenshot file was not created: {}", screenshot_path);
        }

        trace!("Loading screenshot image...");
        let image = image::open(&screenshot_path).context("Failed to load screenshot image")?;

        trace!(
            "Screenshot loaded: {}x{} pixels, format: {:?}",
            image.width(),
            image.height(),
            image.color()
        );

        // Clean up the temporary file
        let _ = std::fs::remove_file(&screenshot_path);
        trace!("=== VBOX SCREEN CAPTURE END ===");

        Ok(image)
    }

    async fn get_console_output(&self, instance: &VmInstance) -> Result<String> {
        trace!(
            "Getting console output from VirtualBox VM: {}",
            instance.name
        );

        // Check if VM has serial port configured for console output
        let serial_file_path = format!("{}-console.log", instance.name);

        // First, ensure serial port is configured for this VM
        self.configure_console_output(instance, &serial_file_path)
            .await?;

        // Read the console output from the serial file
        match std::fs::read_to_string(&serial_file_path) {
            Ok(content) => Ok(content),
            Err(_) => {
                // If no file exists yet, try to get output via VM info
                self.get_vm_console_info(instance).await
            }
        }
    }

    fn name(&self) -> &'static str {
        "virtualbox"
    }
}

impl VirtualBoxProvider {
    async fn configure_console_output(
        &self,
        instance: &VmInstance,
        output_file: &str,
    ) -> Result<()> {
        // Check if VM is running - can't modify VM config while running
        if self.is_running(instance).await? {
            trace!(
                "VM {} is running, skipping serial port configuration",
                instance.name
            );
            return Ok(());
        }

        // Configure serial port 1 to output to file
        let configs = [
            ("--uart1", "0x3F8", "4"),
            ("--uartmode1", "file", output_file),
        ];

        for (key, value1, value2) in &configs {
            let mut cmd = self.vboxmanage_cmd();
            cmd.args(["modifyvm", &instance.name, key]);

            if key == &"--uart1" {
                cmd.args([value1, value2]);
            } else {
                cmd.args([&format!("{value1} {value2}")]);
            }

            let output = cmd.output().context("Failed to configure serial port")?;

            if !output.status.success() {
                warn!(
                    "Failed to configure serial port: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }

        Ok(())
    }

    async fn get_vm_console_info(&self, instance: &VmInstance) -> Result<String> {
        // Get VM runtime information
        let output = self
            .vboxmanage_cmd()
            .args(["showvminfo", &instance.name, "--machinereadable"])
            .output()
            .context("Failed to get VM info")?;

        if !output.status.success() {
            return Err(anyhow!(
                "Failed to get VM console info: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let output_str = String::from_utf8_lossy(&output.stdout);

        // Extract relevant console/boot information
        let mut console_lines = Vec::new();

        for line in output_str.lines() {
            if line.contains("VMState")
                || line.contains("bootmenu")
                || line.contains("boot")
                || line.contains("uart")
                || line.contains("serial")
            {
                console_lines.push(line);
            }
        }

        if console_lines.is_empty() {
            return Ok("No console output available".to_string());
        }

        Ok(console_lines.join("\n"))
    }
}

impl VirtualBoxProvider {
    fn key_to_scancodes(&self, key: &str) -> Result<Vec<String>> {
        // Handle key combinations like "ctrl+c"
        if key.contains('+') {
            return self.handle_key_combination(key);
        }

        let scancodes = match key.to_lowercase().as_str() {
            "enter" | "return" => vec!["1c", "9c"],
            "tab" => vec!["0f", "8f"],
            "space" => vec!["39", "b9"],
            "esc" | "escape" => vec!["01", "81"],
            "up" => vec!["48", "c8"],
            "down" => vec!["50", "d0"],
            "left" => vec!["4b", "cb"],
            "right" => vec!["4d", "cd"],
            "f1" => vec!["3b", "bb"],
            "f2" => vec!["3c", "bc"],
            "f3" => vec!["3d", "bd"],
            "f4" => vec!["3e", "be"],
            "f5" => vec!["3f", "bf"],
            "f6" => vec!["40", "c0"],
            "f7" => vec!["41", "c1"],
            "f8" => vec!["42", "c2"],
            "f9" => vec!["43", "c3"],
            "f10" => vec!["44", "c4"],
            "f11" => vec!["57", "d7"],
            "f12" => vec!["58", "d8"],
            // Character keys
            "a" => vec!["1e", "9e"],
            "b" => vec!["30", "b0"],
            "c" => vec!["2e", "ae"],
            "d" => vec!["20", "a0"],
            "e" => vec!["12", "92"],
            "f" => vec!["21", "a1"],
            "g" => vec!["22", "a2"],
            "h" => vec!["23", "a3"],
            "i" => vec!["17", "97"],
            "j" => vec!["24", "a4"],
            "k" => vec!["25", "a5"],
            "l" => vec!["26", "a6"],
            "m" => vec!["32", "b2"],
            "n" => vec!["31", "b1"],
            "o" => vec!["18", "98"],
            "p" => vec!["19", "99"],
            "q" => vec!["10", "90"],
            "r" => vec!["13", "93"],
            "s" => vec!["1f", "9f"],
            "t" => vec!["14", "94"],
            "u" => vec!["16", "96"],
            "v" => vec!["2f", "af"],
            "w" => vec!["11", "91"],
            "x" => vec!["2d", "ad"],
            "y" => vec!["15", "95"],
            "z" => vec!["2c", "ac"],
            // Numbers
            "0" => vec!["0b", "8b"],
            "1" => vec!["02", "82"],
            "2" => vec!["03", "83"],
            "3" => vec!["04", "84"],
            "4" => vec!["05", "85"],
            "5" => vec!["06", "86"],
            "6" => vec!["07", "87"],
            "7" => vec!["08", "88"],
            "8" => vec!["09", "89"],
            "9" => vec!["0a", "8a"],
            // Special characters
            "-" => vec!["0c", "8c"],  // Hyphen/minus
            "=" => vec!["0d", "8d"],  // Equals
            "[" => vec!["1a", "9a"],  // Left bracket
            "]" => vec!["1b", "9b"],  // Right bracket
            "\\" => vec!["2b", "ab"], // Backslash
            ";" => vec!["27", "a7"],  // Semicolon
            "'" => vec!["28", "a8"],  // Apostrophe/single quote
            "`" => vec!["29", "a9"],  // Grave accent/backtick
            "," => vec!["33", "b3"],  // Comma
            "." => vec!["34", "b4"],  // Period/dot
            "/" => vec!["35", "b5"],  // Forward slash
            // Shifted characters (using shift scancode 2a for press, aa for release)
            "!" => vec!["2a", "02", "82", "aa"],  // Shift+1
            "@" => vec!["2a", "03", "83", "aa"],  // Shift+2
            "#" => vec!["2a", "04", "84", "aa"],  // Shift+3
            "$" => vec!["2a", "05", "85", "aa"],  // Shift+4
            "%" => vec!["2a", "06", "86", "aa"],  // Shift+5
            "^" => vec!["2a", "07", "87", "aa"],  // Shift+6
            "&" => vec!["2a", "08", "88", "aa"],  // Shift+7
            "*" => vec!["2a", "09", "89", "aa"],  // Shift+8
            "(" => vec!["2a", "0a", "8a", "aa"],  // Shift+9
            ")" => vec!["2a", "0b", "8b", "aa"],  // Shift+0
            "_" => vec!["2a", "0c", "8c", "aa"],  // Shift+-
            "+" => vec!["2a", "0d", "8d", "aa"],  // Shift+=
            "{" => vec!["2a", "1a", "9a", "aa"],  // Shift+[
            "}" => vec!["2a", "1b", "9b", "aa"],  // Shift+]
            "|" => vec!["2a", "2b", "ab", "aa"],  // Shift+\
            ":" => vec!["2a", "27", "a7", "aa"],  // Shift+;
            "\"" => vec!["2a", "28", "a8", "aa"], // Shift+'
            "~" => vec!["2a", "29", "a9", "aa"],  // Shift+`
            "<" => vec!["2a", "33", "b3", "aa"],  // Shift+,
            ">" => vec!["2a", "34", "b4", "aa"],  // Shift+.
            "?" => vec!["2a", "35", "b5", "aa"],  // Shift+/
            // Uppercase letters (using shift)
            "a" => vec!["2a", "1e", "9e", "aa"],
            "b" => vec!["2a", "30", "b0", "aa"],
            "c" => vec!["2a", "2e", "ae", "aa"],
            "d" => vec!["2a", "20", "a0", "aa"],
            "E" => vec!["2a", "12", "92", "aa"],
            "F" => vec!["2a", "21", "a1", "aa"],
            "G" => vec!["2a", "22", "a2", "aa"],
            "H" => vec!["2a", "23", "a3", "aa"],
            "I" => vec!["2a", "17", "97", "aa"],
            "J" => vec!["2a", "24", "a4", "aa"],
            "K" => vec!["2a", "25", "a5", "aa"],
            "L" => vec!["2a", "26", "a6", "aa"],
            "M" => vec!["2a", "32", "b2", "aa"],
            "N" => vec!["2a", "31", "b1", "aa"],
            "O" => vec!["2a", "18", "98", "aa"],
            "P" => vec!["2a", "19", "99", "aa"],
            "Q" => vec!["2a", "10", "90", "aa"],
            "R" => vec!["2a", "13", "93", "aa"],
            "S" => vec!["2a", "1f", "9f", "aa"],
            "T" => vec!["2a", "14", "94", "aa"],
            "U" => vec!["2a", "16", "96", "aa"],
            "V" => vec!["2a", "2f", "af", "aa"],
            "W" => vec!["2a", "11", "91", "aa"],
            "X" => vec!["2a", "2d", "ad", "aa"],
            "Y" => vec!["2a", "15", "95", "aa"],
            "Z" => vec!["2a", "2c", "ac", "aa"],
            _ => return Err(anyhow!("Unknown key for VirtualBox: {}", key)),
        };

        Ok(scancodes.into_iter().map(|s| s.to_string()).collect())
    }

    fn handle_key_combination(&self, combination: &str) -> Result<Vec<String>> {
        let parts: Vec<&str> = combination.split('+').collect();
        if parts.len() != 2 {
            return Err(anyhow!("Invalid key combination format: {}", combination));
        }

        let modifier = parts[0].trim().to_lowercase();
        let key = parts[1].trim().to_lowercase();

        let mut scancodes = Vec::new();

        // Add modifier press
        let modifier_press = match modifier.as_str() {
            "ctrl" | "control" => "1d",
            "shift" => "2a",
            "alt" => "38",
            _ => return Err(anyhow!("Unknown modifier: {}", modifier)),
        };
        scancodes.push(modifier_press.to_string());

        // Add key press and release
        let key_scancodes = self.key_to_scancodes(&key)?;
        scancodes.extend(key_scancodes);

        // Add modifier release
        let modifier_release = match modifier.as_str() {
            "ctrl" | "control" => "9d",
            "shift" => "aa",
            "alt" => "b8",
            _ => return Err(anyhow!("Unknown modifier: {}", modifier)),
        };
        scancodes.push(modifier_release.to_string());

        Ok(scancodes)
    }
}
