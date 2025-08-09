use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use tokio::process::Command as TokioCommand;
use tokio::time::{sleep, timeout};
use tracing::{debug, info, warn};
use image::{DynamicImage, ImageBuffer, Rgb};

use crate::automation::vm::{VmInstance, VmState, NetworkAdapterType};
use super::VmProviderTrait;

pub struct QemuProvider {
    working_dir: PathBuf,
}

impl QemuProvider {
    pub fn new() -> Self {
        Self {
            working_dir: std::env::temp_dir().join("isotope-qemu"),
        }
    }

    fn get_disk_path(&self, instance: &VmInstance) -> PathBuf {
        self.working_dir.join(format!("{}.qcow2", instance.name))
    }

    fn get_monitor_socket(&self, instance: &VmInstance) -> PathBuf {
        self.working_dir.join(format!("{}-monitor.sock", instance.name))
    }

    async fn send_monitor_command(&self, instance: &VmInstance, command: &str) -> Result<String> {
        #[cfg(unix)]
        {
            use tokio::net::UnixStream;
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            let socket_path = self.get_monitor_socket(instance);
            if !socket_path.exists() {
                return Err(anyhow!("QEMU monitor socket not found"));
            }

            let mut stream = UnixStream::connect(socket_path).await
                .context("Failed to connect to QEMU monitor")?;

            stream.write_all(format!("{}\n", command).as_bytes()).await?;
            
            let mut response = String::new();
            stream.read_to_string(&mut response).await?;
            
            Ok(response)
        }
        
        #[cfg(windows)]
        {
            // On Windows, use named pipes or TCP connection
            warn!("QEMU monitor commands not fully implemented on Windows");
            Ok(String::new())
        }
    }
}

#[async_trait]
impl VmProviderTrait for QemuProvider {
    async fn create_vm(&self, instance: &mut VmInstance) -> Result<()> {
        info!("Creating QEMU VM: {}", instance.name);

        std::fs::create_dir_all(&self.working_dir)
            .context("Failed to create QEMU working directory")?;

        let disk_path = self.get_disk_path(instance);
        
        // Create VM disk if it doesn't exist
        if !disk_path.exists() {
            let output = Command::new("qemu-img")
                .args([
                    "create",
                    "-f", "qcow2",
                    disk_path.to_str().unwrap(),
                    &format!("{}G", instance.config.disk_size_gb)
                ])
                .output()
                .context("Failed to execute qemu-img")?;

            if !output.status.success() {
                return Err(anyhow!("Failed to create QEMU disk: {}", 
                    String::from_utf8_lossy(&output.stderr)));
            }

            info!("Created VM disk: {}", disk_path.display());
        }

        instance.set_disk_path(disk_path);
        instance.set_state(VmState::Stopped);
        
        Ok(())
    }

    async fn start_vm(&self, instance: &mut VmInstance) -> Result<()> {
        info!("Starting QEMU VM: {}", instance.name);

        if instance.is_running() {
            return Ok(()); // Already running
        }

        let disk_path = instance.disk_path.as_ref()
            .ok_or_else(|| anyhow!("VM disk path not set"))?;

        let monitor_socket = self.get_monitor_socket(instance);
        let console_log = self.get_console_log_path(instance);
        
        let mut cmd = TokioCommand::new("qemu-system-x86_64");
        cmd.args([
            "-m", &instance.config.memory_mb.to_string(),
            "-smp", &instance.config.cpus.to_string(),
            "-drive", &format!("file={},format=qcow2", disk_path.display()),
            "-monitor", &format!("unix:{},server,nowait", monitor_socket.display()),
            "-serial", &format!("file:{}", console_log.display()),
            "-daemonize",
            "-display", "none", // Headless
        ]);

        // Add ISO if attached
        if let Some(iso_path) = &instance.iso_path {
            cmd.args(["-cdrom", iso_path.to_str().unwrap()]);
        }

        // Add network configuration
        match instance.config.network_config.adapter_type {
            NetworkAdapterType::NAT => {
                if instance.config.network_config.enable_ssh {
                    cmd.args([
                        "-netdev", &format!("user,id=net0,hostfwd=tcp::{}-:22", 
                            instance.config.network_config.ssh_port),
                        "-device", "e1000,netdev=net0"
                    ]);
                } else {
                    cmd.args(["-netdev", "user,id=net0", "-device", "e1000,netdev=net0"]);
                }
            }
            _ => {
                // Other network types would be implemented here
                cmd.args(["-netdev", "user,id=net0", "-device", "e1000,netdev=net0"]);
            }
        }

        // Add additional arguments
        for arg in &instance.config.additional_args {
            cmd.arg(arg);
        }

        instance.set_state(VmState::Starting);

        let output = cmd.output().await
            .context("Failed to start QEMU VM")?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            instance.set_state(VmState::Error(error_msg.to_string()));
            return Err(anyhow!("Failed to start QEMU VM: {}", error_msg));
        }

        // Wait for VM to be responsive
        sleep(Duration::from_secs(5)).await;

        if self.is_running(instance).await? {
            instance.set_state(VmState::Running);
            info!("QEMU VM started successfully: {}", instance.name);
        } else {
            instance.set_state(VmState::Error("VM failed to start properly".to_string()));
            return Err(anyhow!("VM failed to start properly"));
        }

        Ok(())
    }

    async fn stop_vm(&self, instance: &mut VmInstance) -> Result<()> {
        info!("Stopping QEMU VM: {}", instance.name);

        if instance.is_stopped() {
            return Ok(()); // Already stopped
        }

        instance.set_state(VmState::Stopping);

        // Send shutdown command via monitor
        match self.send_monitor_command(instance, "system_powerdown").await {
            Ok(_) => {
                // Wait for graceful shutdown
                if timeout(Duration::from_secs(30), self.wait_for_shutdown(instance)).await.is_ok() {
                    instance.set_state(VmState::Stopped);
                    return Ok(());
                }
            }
            Err(_) => {
                debug!("Monitor command failed, trying forceful shutdown");
            }
        }

        // Forceful shutdown via monitor
        if let Err(_) = self.send_monitor_command(instance, "quit").await {
            warn!("Failed to send quit command to QEMU monitor");
        }

        instance.set_state(VmState::Stopped);
        Ok(())
    }

    async fn delete_vm(&self, instance: &mut VmInstance) -> Result<()> {
        info!("Deleting QEMU VM: {}", instance.name);

        // Stop VM first if running
        if !instance.is_stopped() {
            self.stop_vm(instance).await?;
        }

        // Remove disk file
        if let Some(disk_path) = &instance.disk_path {
            if disk_path.exists() {
                std::fs::remove_file(disk_path)
                    .context("Failed to remove VM disk file")?;
            }
        }

        // Remove monitor socket
        let monitor_socket = self.get_monitor_socket(instance);
        if monitor_socket.exists() {
            let _ = std::fs::remove_file(monitor_socket);
        }

        Ok(())
    }

    async fn attach_iso(&self, instance: &mut VmInstance, iso_path: &Path) -> Result<()> {
        info!("Attaching ISO to QEMU VM: {}", iso_path.display());

        if !iso_path.exists() {
            return Err(anyhow!("ISO file does not exist: {}", iso_path.display()));
        }

        instance.set_iso_path(iso_path.to_path_buf());

        // If VM is running, we'd need to hot-plug the ISO via monitor
        if instance.is_running() {
            let command = format!("change ide1-cd0 {}", iso_path.display());
            self.send_monitor_command(instance, &command).await
                .context("Failed to hot-plug ISO via QEMU monitor")?;
        }

        Ok(())
    }

    async fn detach_iso(&self, instance: &mut VmInstance) -> Result<()> {
        info!("Detaching ISO from QEMU VM: {}", instance.name);

        if instance.is_running() {
            self.send_monitor_command(instance, "change ide1-cd0 /dev/null").await
                .context("Failed to detach ISO via QEMU monitor")?;
        }

        instance.iso_path = None;
        Ok(())
    }

    async fn create_snapshot(&self, instance: &VmInstance, snapshot_name: &str) -> Result<()> {
        info!("Creating QEMU snapshot: {}", snapshot_name);

        let disk_path = instance.disk_path.as_ref()
            .ok_or_else(|| anyhow!("VM disk path not set"))?;

        if instance.is_running() {
            // Create live snapshot via monitor
            let command = format!("savevm {}", snapshot_name);
            self.send_monitor_command(instance, &command).await
                .context("Failed to create live snapshot")?;
        } else {
            // Create offline snapshot
            let output = Command::new("qemu-img")
                .args([
                    "snapshot",
                    "-c", snapshot_name,
                    disk_path.to_str().unwrap()
                ])
                .output()
                .context("Failed to create offline snapshot")?;

            if !output.status.success() {
                return Err(anyhow!("Failed to create snapshot: {}", 
                    String::from_utf8_lossy(&output.stderr)));
            }
        }

        Ok(())
    }

    async fn restore_snapshot(&self, instance: &mut VmInstance, snapshot_name: &str) -> Result<()> {
        info!("Restoring QEMU snapshot: {}", snapshot_name);

        let disk_path = instance.disk_path.as_ref()
            .ok_or_else(|| anyhow!("VM disk path not set"))?
            .clone();

        // VM must be stopped to restore snapshot
        if !instance.is_stopped() {
            self.stop_vm(instance).await?;
        }

        let output = Command::new("qemu-img")
            .args([
                "snapshot",
                "-a", snapshot_name,
                disk_path.to_str().unwrap()
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
        // Check if QEMU process is running by checking monitor socket
        let monitor_socket = self.get_monitor_socket(instance);
        Ok(monitor_socket.exists())
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
        debug!("Sending keys to QEMU VM: {:?}", keys);

        for key in keys {
            let command = format!("sendkey {}", key);
            self.send_monitor_command(instance, &command).await
                .context("Failed to send key via QEMU monitor")?;
            
            // Small delay between keystrokes
            sleep(Duration::from_millis(100)).await;
        }

        Ok(())
    }

    async fn capture_screen(&self, instance: &VmInstance) -> Result<DynamicImage> {
        debug!("Capturing screen from QEMU VM: {}", instance.name);

        let screenshot_path = self.working_dir.join(format!("{}-screenshot.ppm", instance.name));
        
        // Use QEMU monitor to take screenshot
        let command = format!("screendump {}", screenshot_path.display());
        self.send_monitor_command(instance, &command).await
            .context("Failed to capture screenshot via QEMU monitor")?;
        
        // Wait a moment for the file to be written
        sleep(Duration::from_millis(500)).await;
        
        if !screenshot_path.exists() {
            return Err(anyhow!("Screenshot file was not created"));
        }
        
        // Load the PPM image
        let image = image::open(&screenshot_path)
            .context("Failed to load screenshot image")?;
        
        // Clean up the temporary file
        let _ = std::fs::remove_file(&screenshot_path);
        
        Ok(image)
    }

    async fn get_console_output(&self, instance: &VmInstance) -> Result<String> {
        debug!("Getting console output from QEMU VM: {}", instance.name);
        
        let console_file = self.get_console_log_path(instance);
        
        // Try to read from console log file
        if console_file.exists() {
            match std::fs::read_to_string(&console_file) {
                Ok(content) => return Ok(content),
                Err(e) => {
                    debug!("Failed to read console file: {}", e);
                }
            }
        }
        
        // Try to get output via monitor info command
        match self.send_monitor_command(instance, "info registers").await {
            Ok(output) => {
                // Get additional system info
                let mut info_lines = vec![output];
                
                if let Ok(status) = self.send_monitor_command(instance, "info status").await {
                    info_lines.push(status);
                }
                
                if let Ok(block_info) = self.send_monitor_command(instance, "info block").await {
                    info_lines.push(block_info);
                }
                
                Ok(info_lines.join("\n---\n"))
            }
            Err(_) => {
                // Try reading from any existing log files in working directory
                self.read_qemu_logs(instance).await
            }
        }
    }

    fn name(&self) -> &'static str {
        "qemu"
    }
}

impl QemuProvider {
    fn get_console_log_path(&self, instance: &VmInstance) -> PathBuf {
        self.working_dir.join(format!("{}-console.log", instance.name))
    }
    
    async fn read_qemu_logs(&self, instance: &VmInstance) -> Result<String> {
        let log_patterns = [
            format!("{}.log", instance.name),
            format!("{}-serial.log", instance.name),
            format!("{}-console.log", instance.name),
        ];
        
        let mut all_logs = Vec::new();
        
        for pattern in &log_patterns {
            let log_path = self.working_dir.join(pattern);
            if log_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&log_path) {
                    all_logs.push(format!("=== {} ===\n{}", pattern, content));
                }
            }
        }
        
        if all_logs.is_empty() {
            Ok("No console logs available".to_string())
        } else {
            Ok(all_logs.join("\n\n"))
        }
    }
}