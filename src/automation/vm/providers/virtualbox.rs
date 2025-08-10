use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use image::DynamicImage;
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tracing::{debug, info, trace, warn};

use super::VmProviderTrait;
use crate::automation::library_keyboard_input::LibraryBasedKeyboardMapper;
use crate::automation::vm::{VmInstance, VmState};
use crate::utils::net;

pub struct VirtualBoxProvider {
    keyboard_mapper: LibraryBasedKeyboardMapper,
}

impl VirtualBoxProvider {
    pub fn new() -> Self {
        Self {
            keyboard_mapper: LibraryBasedKeyboardMapper::new(),
        }
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
        Ok(output_str.contains(&format!("\"{}\"", vm_name)))
    }
}

#[async_trait]
impl VmProviderTrait for VirtualBoxProvider {
    fn get_ssh_endpoint(&self, instance: &VmInstance) -> (String, u16) {
        // For VirtualBox, we use port forwarding which maps localhost:HOST_PORT -> VM:22
        // Always query VirtualBox directly to get the actual forwarded port
        match tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                self.get_ssh_port_from_vbox(&instance.name).await
            })
        }) {
            Ok(Some(port)) => {
                tracing::info!("VirtualBox SSH endpoint: 127.0.0.1:{} (queried from VBox)", port);
                ("127.0.0.1".to_string(), port)
            }
            Ok(None) => {
                tracing::warn!("No SSH port forwarding found for VM {}, falling back to default 22", instance.name);
                ("127.0.0.1".to_string(), 22)
            }
            Err(e) => {
                tracing::error!("Failed to query SSH port from VirtualBox for VM {}: {}, falling back to default 22", instance.name, e);
                ("127.0.0.1".to_string(), 22)
            }
        }
    }
    async fn create_vm(&self, instance: &mut VmInstance) -> Result<()> {
        info!("Creating VirtualBox VM: {}", instance.name);

        // Check if VM already exists
        if self.vm_exists(&instance.name).await? {
            info!(
                "VirtualBox VM {} already exists, checking SSH port forwarding",
                instance.name
            );
            
            // Check if SSH port forwarding exists
            if let Some(actual_port) = self.get_ssh_port_from_vbox(&instance.name).await? {
                instance.config.network_config.ssh_port = actual_port;
                info!("Found existing SSH port forwarding: {}", actual_port);
            } else {
                // No port forwarding exists, find a free port and set it up
                let ssh_host_port = net::find_free_port()
                    .ok_or_else(|| anyhow!("No free port found for SSH forwarding"))?;
                
                info!("No SSH port forwarding found, setting up port forwarding to port {}", ssh_host_port);
                
                // Update the instance config with the found port
                instance.config.network_config.ssh_port = ssh_host_port;
                
                // Set up port forwarding for SSH
                let output = self
                    .vboxmanage_cmd()
                    .args([
                        "modifyvm",
                        &instance.name,
                        "--natpf1",
                        &format!("ssh,tcp,,{},,22", ssh_host_port),
                    ])
                    .output()
                    .context("Failed to set up port forwarding for SSH")?;
                
                if !output.status.success() {
                    return Err(anyhow!(
                        "Failed to set up port forwarding: {}",
                        String::from_utf8_lossy(&output.stderr)
                    ));
                } else {
                    info!("Successfully set up SSH port forwarding: {}", ssh_host_port);
                }
            }
            
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
                &format!("ssh,tcp,,{},,22", ssh_host_port),
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
                &format!("Isotope snapshot: {}", snapshot_name),
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

        // Keys are already converted to scancodes by our KeyboardMapper
        // So we can send them directly to VirtualBox
        let output = self
            .vboxmanage_cmd()
            .args(["controlvm", &instance.name, "keyboardputscancode"])
            .args(keys.iter().map(|s| s.as_str()))
            .output()
            .context("Failed to send keyboard input")?;

        if !output.status.success() {
            return Err(anyhow!(
                "Failed to send keys: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
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
    /// Get SSH port forwarding from VirtualBox VM configuration
    pub async fn get_ssh_port_from_vbox(&self, vm_name: &str) -> Result<Option<u16>> {
        let output = self
            .vboxmanage_cmd()
            .args(["showvminfo", vm_name, "--machinereadable"])
            .output()
            .context("Failed to get VM info")?;

        if !output.status.success() {
            return Ok(None);
        }

        let output_str = String::from_utf8_lossy(&output.stdout);
        
        // Look for NAT port forwarding rule for SSH
        // Format: Forwarding(0)="ssh,tcp,,PORT,,22"
        for line in output_str.lines() {
            if line.contains("Forwarding(") && line.contains("ssh,tcp") && line.contains(",,22") {
                // Extract the host port from the line
                // Example: Forwarding(0)="ssh,tcp,,20000,,22"
                if let Some(start) = line.find(",,") {
                    if let Some(end) = line[start+2..].find(",,") {
                        let port_str = &line[start+2..start+2+end];
                        if let Ok(port) = port_str.parse::<u16>() {
                            return Ok(Some(port));
                        }
                    }
                }
            }
        }
        
        Ok(None)
    }

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
                cmd.args([&format!("{} {}", value1, value2)]);
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
