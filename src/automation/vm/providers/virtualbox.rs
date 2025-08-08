use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tracing::{debug, info};

use crate::automation::vm::{VmInstance, VmState};
use super::VmProviderTrait;

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
}

#[async_trait]
impl VmProviderTrait for VirtualBoxProvider {
    async fn create_vm(&self, instance: &mut VmInstance) -> Result<()> {
        info!("Creating VirtualBox VM: {}", instance.name);

        // Create VM
        let output = self.vboxmanage_cmd()
            .args([
                "createvm",
                "--name", &instance.name,
                "--ostype", "Linux_64", // Default, could be configurable
                "--register"
            ])
            .output()
            .context("Failed to execute VBoxManage createvm")?;

        if !output.status.success() {
            return Err(anyhow!("Failed to create VirtualBox VM: {}", 
                String::from_utf8_lossy(&output.stderr)));
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
            let output = self.vboxmanage_cmd()
                .args(["modifyvm", &instance.name, key, value])
                .output()
                .context("Failed to configure VM")?;

            if !output.status.success() {
                return Err(anyhow!("Failed to configure VM setting {}: {}", 
                    key, String::from_utf8_lossy(&output.stderr)));
            }
        }

        // Create and attach disk
        let disk_path = format!("{}.vdi", instance.name);
        
        let output = self.vboxmanage_cmd()
            .args([
                "createmedium",
                "disk",
                "--filename", &disk_path,
                "--size", &(instance.config.disk_size_gb * 1024).to_string(), // Convert to MB
                "--format", "VDI"
            ])
            .output()
            .context("Failed to create VM disk")?;

        if !output.status.success() {
            return Err(anyhow!("Failed to create VirtualBox disk: {}", 
                String::from_utf8_lossy(&output.stderr)));
        }

        // Attach disk to VM
        let output = self.vboxmanage_cmd()
            .args([
                "storagectl", &instance.name,
                "--name", "SATA Controller",
                "--add", "sata",
                "--controller", "IntelAHCI"
            ])
            .output()
            .context("Failed to add SATA controller")?;

        if !output.status.success() {
            return Err(anyhow!("Failed to add SATA controller: {}", 
                String::from_utf8_lossy(&output.stderr)));
        }

        let output = self.vboxmanage_cmd()
            .args([
                "storageattach", &instance.name,
                "--storagectl", "SATA Controller",
                "--port", "0",
                "--device", "0",
                "--type", "hdd",
                "--medium", &disk_path
            ])
            .output()
            .context("Failed to attach disk")?;

        if !output.status.success() {
            return Err(anyhow!("Failed to attach disk: {}", 
                String::from_utf8_lossy(&output.stderr)));
        }

        instance.set_state(VmState::Stopped);
        Ok(())
    }

    async fn start_vm(&self, instance: &mut VmInstance) -> Result<()> {
        info!("Starting VirtualBox VM: {}", instance.name);

        if instance.is_running() {
            return Ok(());
        }

        instance.set_state(VmState::Starting);

        let output = self.vboxmanage_cmd()
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
        let output = self.vboxmanage_cmd()
            .args(["controlvm", &instance.name, "acpipowerbutton"])
            .output()
            .context("Failed to send ACPI power button")?;

        if output.status.success() {
            // Wait for graceful shutdown
            if timeout(Duration::from_secs(30), self.wait_for_shutdown(instance)).await.is_ok() {
                instance.set_state(VmState::Stopped);
                return Ok(());
            }
        }

        // Force power off
        let output = self.vboxmanage_cmd()
            .args(["controlvm", &instance.name, "poweroff"])
            .output()
            .context("Failed to power off VM")?;

        if !output.status.success() {
            return Err(anyhow!("Failed to power off VM: {}", 
                String::from_utf8_lossy(&output.stderr)));
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
        let output = self.vboxmanage_cmd()
            .args(["unregistervm", &instance.name, "--delete"])
            .output()
            .context("Failed to delete VirtualBox VM")?;

        if !output.status.success() {
            return Err(anyhow!("Failed to delete VM: {}", 
                String::from_utf8_lossy(&output.stderr)));
        }

        Ok(())
    }

    async fn attach_iso(&self, instance: &mut VmInstance, iso_path: &Path) -> Result<()> {
        info!("Attaching ISO to VirtualBox VM: {}", iso_path.display());

        if !iso_path.exists() {
            return Err(anyhow!("ISO file does not exist: {}", iso_path.display()));
        }

        // Create IDE controller if it doesn't exist
        let _ = self.vboxmanage_cmd()
            .args([
                "storagectl", &instance.name,
                "--name", "IDE Controller",
                "--add", "ide"
            ])
            .output();

        // Attach ISO
        let output = self.vboxmanage_cmd()
            .args([
                "storageattach", &instance.name,
                "--storagectl", "IDE Controller",
                "--port", "1",
                "--device", "0",
                "--type", "dvddrive",
                "--medium", iso_path.to_str().unwrap()
            ])
            .output()
            .context("Failed to attach ISO")?;

        if !output.status.success() {
            return Err(anyhow!("Failed to attach ISO: {}", 
                String::from_utf8_lossy(&output.stderr)));
        }

        instance.set_iso_path(iso_path.to_path_buf());
        Ok(())
    }

    async fn detach_iso(&self, instance: &mut VmInstance) -> Result<()> {
        info!("Detaching ISO from VirtualBox VM");

        let output = self.vboxmanage_cmd()
            .args([
                "storageattach", &instance.name,
                "--storagectl", "IDE Controller",
                "--port", "1",
                "--device", "0",
                "--medium", "none"
            ])
            .output()
            .context("Failed to detach ISO")?;

        if !output.status.success() {
            return Err(anyhow!("Failed to detach ISO: {}", 
                String::from_utf8_lossy(&output.stderr)));
        }

        instance.iso_path = None;
        Ok(())
    }

    async fn create_snapshot(&self, instance: &VmInstance, snapshot_name: &str) -> Result<()> {
        info!("Creating VirtualBox snapshot: {}", snapshot_name);

        let output = self.vboxmanage_cmd()
            .args([
                "snapshot", &instance.name,
                "take", snapshot_name,
                "--description", &format!("Isotope snapshot: {}", snapshot_name)
            ])
            .output()
            .context("Failed to create snapshot")?;

        if !output.status.success() {
            return Err(anyhow!("Failed to create snapshot: {}", 
                String::from_utf8_lossy(&output.stderr)));
        }

        Ok(())
    }

    async fn restore_snapshot(&self, instance: &mut VmInstance, snapshot_name: &str) -> Result<()> {
        info!("Restoring VirtualBox snapshot: {}", snapshot_name);

        // VM must be stopped to restore snapshot
        if !instance.is_stopped() {
            self.stop_vm(instance).await?;
        }

        let output = self.vboxmanage_cmd()
            .args([
                "snapshot", &instance.name,
                "restore", snapshot_name
            ])
            .output()
            .context("Failed to restore snapshot")?;

        if !output.status.success() {
            return Err(anyhow!("Failed to restore snapshot: {}", 
                String::from_utf8_lossy(&output.stderr)));
        }

        Ok(())
    }

    async fn is_running(&self, instance: &VmInstance) -> Result<bool> {
        let output = self.vboxmanage_cmd()
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
        }).await
        .context("Timeout waiting for VM shutdown")?
    }

    async fn send_keys(&self, instance: &VmInstance, keys: &[String]) -> Result<()> {
        debug!("Sending keys to VirtualBox VM: {:?}", keys);

        for key in keys {
            // Convert key to VirtualBox scancode format
            let scancodes = self.key_to_scancodes(key)?;
            
            let output = self.vboxmanage_cmd()
                .args([
                    "controlvm", &instance.name,
                    "keyboardputscancode"
                ])
                .args(scancodes.iter().map(|s| s.as_str()))
                .output()
                .context("Failed to send keyboard input")?;

            if !output.status.success() {
                return Err(anyhow!("Failed to send key '{}': {}", 
                    key, String::from_utf8_lossy(&output.stderr)));
            }

            sleep(Duration::from_millis(50)).await;
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "virtualbox"
    }
}

impl VirtualBoxProvider {
    fn key_to_scancodes(&self, key: &str) -> Result<Vec<String>> {
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
            _ => return Err(anyhow!("Unknown key for VirtualBox: {}", key)),
        };

        Ok(scancodes.into_iter().map(|s| s.to_string()).collect())
    }
}