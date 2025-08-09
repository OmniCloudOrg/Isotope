use anyhow::{anyhow, Context, Result};
use clap::error;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::io::{Read, Write};
use tokio::time::{sleep, timeout};
use tracing::{debug, error, info, trace, warn};
use ssh2::Session;

use crate::automation::keypress::{KeypressExecutor, KeypressAction};
use crate::automation::vm::{VmInstance, VmManager};
use crate::automation::ocr::OcrEngine;
use crate::config::{Instruction, Stage};
use crate::utils::template::TemplateEngine;

#[derive(Debug, Clone)]
pub struct SshCredentials {
    pub username: String,
    pub password: Option<String>,
    pub private_key: Option<PathBuf>,
    pub host: String,
    pub port: u16,
}

pub struct PuppetManager {
    keypress_executor: KeypressExecutor,
    template_engine: TemplateEngine,
    environment_vars: HashMap<String, String>,
    ocr_engine: OcrEngine,
    ssh_credentials: Option<SshCredentials>,
}

impl PuppetManager {
    pub fn new() -> Self {
        Self {
            keypress_executor: KeypressExecutor::new(),
            template_engine: TemplateEngine::new(),
            environment_vars: std::env::vars().collect(),
            ocr_engine: OcrEngine::new(),
            ssh_credentials: None,
        }
    }

    pub async fn execute_stage_instructions(&mut self, vm: &VmInstance, stage: &Stage, vm_manager: &VmManager) -> Result<()> {
        self.execute_stage_instructions_from_step(vm, stage, vm_manager, None).await
    }

    pub async fn execute_stage_instructions_from_step(&mut self, vm: &VmInstance, stage: &Stage, vm_manager: &VmManager, continue_from_step: Option<usize>) -> Result<()> {
        info!("Executing puppet instructions for stage: {:?}", stage.name);

        let start_from = if let Some(step) = continue_from_step {
            if step == 0 {
                return Err(anyhow!("Step numbers are 1-based, cannot continue from step 0"));
            }
            let index = step - 1; // Convert to 0-based index
            if index >= stage.instructions.len() {
                return Err(anyhow!("Cannot continue from step {}, stage only has {} instructions", step, stage.instructions.len()));
            }
            info!("Continuing from step {}/{}", step, stage.instructions.len());
            index
        } else {
            0
        };

        for (i, instruction) in stage.instructions.iter().enumerate().skip(start_from) {
            info!("Executing instruction {}/{}: {:?}", i + 1, stage.instructions.len(), instruction);
            
            match instruction {
                // OS Installation instructions (keypress automation)
                Instruction::Wait { duration, condition } => {
                    self.execute_wait_instruction(vm, duration, condition.as_ref(), vm_manager).await?;
                }
                Instruction::Press { key, repeat } => {
                    self.execute_press_instruction(vm, key, *repeat, vm_manager).await?;
                }
                Instruction::Type { text } => {
                    self.execute_type_instruction(vm, text, vm_manager).await?;
                }
                
                // OS Configuration instructions (live OS commands)
                Instruction::Run { command } => {
                    self.execute_run_instruction(vm, command).await?;
                }
                Instruction::Copy { from, to } => {
                    self.execute_copy_instruction(vm, from, to).await?;
                }
                Instruction::Login { username, password, private_key, host, port } => {
                    let ssh_host = host.clone().unwrap_or_else(|| "127.0.0.1".to_string());
                    let ssh_port = port.unwrap_or(vm.config.network_config.ssh_port);
                    
                    self.ssh_credentials = Some(SshCredentials {
                        username: username.clone(),
                        password: password.clone(),
                        private_key: private_key.clone(),
                        host: ssh_host,
                        port: ssh_port,
                    });
                    
                    info!("SSH credentials configured for {}@{}:{}", username, 
                          self.ssh_credentials.as_ref().unwrap().host,
                          self.ssh_credentials.as_ref().unwrap().port);
                }
                
                _ => {
                    warn!("Ignoring unsupported instruction in puppet execution: {:?}", instruction);
                }
            }
        }

        info!("Completed puppet execution for stage");
        Ok(())
    }

    async fn execute_wait_instruction(&self, vm: &VmInstance, duration: &str, condition: Option<&String>, vm_manager: &VmManager) -> Result<()> {
        let wait_duration = self.parse_duration(duration)?;
        
        if let Some(condition_text) = condition {
            info!("Waiting up to {} for condition: {}", duration, condition_text);
            
            // Wait with condition checking
            let result = timeout(wait_duration, async {
                self.wait_for_condition(vm, condition_text, vm_manager).await
            }).await;

            match result {
                Ok(Ok(())) => {
                    info!("Condition '{}' met successfully", condition_text);
                }
                Ok(Err(e)) => {
                    return Err(anyhow!("Error while waiting for condition '{}': {}", condition_text, e));
                }
                Err(_) => {
                    error!("Timeout waiting for condition '{}', assuming we made a mistake and halting execution", condition_text);
                    return Err(anyhow!("Timeout waiting for condition '{}', assuming we made a mistake and halting execution", condition_text));
                }
            }
        } else {
            info!("Waiting for {}", duration);
            sleep(wait_duration).await;
        }

        Ok(())
    }

    async fn execute_press_instruction(&mut self, vm: &VmInstance, key: &str, repeat: Option<u32>, vm_manager: &VmManager) -> Result<()> {
        let repeat_count = repeat.unwrap_or(1);
        
        for i in 0..repeat_count {
            if repeat_count > 1 {
                debug!("Pressing key '{}' ({}/{})", key, i + 1, repeat_count);
            } else {
                debug!("Pressing key '{}'", key);
            }
            
            let action = self.parse_key_action(key)?;
            self.keypress_executor.execute_action(vm, &action, vm_manager).await?;
            
            // Small delay between repeated keypresses
            if i < repeat_count - 1 {
                sleep(Duration::from_millis(100)).await;
            }
        }

        Ok(())
    }

    async fn execute_type_instruction(&mut self, vm: &VmInstance, text: &str, vm_manager: &VmManager) -> Result<()> {
        // Process template variables in text
        let processed_text = self.template_engine.render_string(text, &self.environment_vars)?;
        
        debug!("Typing text: {}", processed_text);
        
        let action = KeypressAction::TypeText(processed_text);
        self.keypress_executor.execute_action(vm, &action, vm_manager).await?;

        Ok(())
    }

    async fn execute_run_instruction(&mut self, vm: &VmInstance, command: &str) -> Result<()> {
        // Process template variables in command
        let processed_command = self.template_engine.render_string(command, &self.environment_vars)?;
        
        info!("Running command in live OS: {}", processed_command);
        
        // Execute command via SSH/remote connection
        self.execute_remote_command(vm, &processed_command).await
            .context("Failed to execute remote command")?;

        Ok(())
    }

    async fn execute_copy_instruction(&mut self, vm: &VmInstance, from: &Path, to: &Path) -> Result<()> {
        info!("Copying file {} to VM path {}", from.display(), to.display());

        if !from.exists() {
            return Err(anyhow!("Source file does not exist: {}", from.display()));
        }

        // Copy file to VM via SCP/remote copy
        self.copy_file_to_vm(vm, from, to).await
            .context("Failed to copy file to VM")?;

        Ok(())
    }

    async fn wait_for_condition(&self, vm: &VmInstance, condition: &str, vm_manager: &VmManager) -> Result<()> {
        // Just wait for the exact text the user specified - no hardcoded logic
        self.wait_for_screen_text(vm, condition, vm_manager).await
    }


    async fn wait_for_screen_text(&self, vm: &VmInstance, pattern: &str, vm_manager: &VmManager) -> Result<()> {
        info!("Waiting for screen text '{}' on VM {}", pattern, vm.name);
        
        // No max attempts limit - let the outer timeout handle the duration
        let mut attempts = 0;
        
        loop {
            attempts += 1;
            debug!("Screen text detection attempt {}", attempts);
            
            // Capture the VM screen
            match vm_manager.capture_screen(vm).await {
                Ok(image) => {
                    // Extract all text to see what OCR is finding
                    match self.ocr_engine.extract_text(&image).await {
                        Ok(extracted_text) => {
                            if attempts <= 3 || attempts % 10 == 0 {
                                trace!("OCR extracted text (attempt {}): '{}'", attempts, extracted_text);
                            }
                            
                            // Check if pattern is found in the extracted text (case-insensitive)
                            if extracted_text.to_lowercase().contains(&pattern.to_lowercase()) {
                                trace!("Found screen text '{}' on VM {} (attempt {})", pattern, vm.name, attempts);
                                return Ok(());
                            } else {
                                trace!("Pattern '{}' not found in extracted text (attempt {})", pattern, attempts);
                            }
                        }
                        Err(e) => {
                            warn!("OCR error during text extraction: {}", e);
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to capture screen: {}", e);
                    // Try console output as fallback
                    if let Ok(console_output) = vm_manager.get_console_output(vm).await {
                        if console_output.to_lowercase().contains(&pattern.to_lowercase()) {
                            info!("Found pattern '{}' in console output", pattern);
                            return Ok(());
                        }
                    }
                }
            }
            
            // Wait before next attempt
            sleep(Duration::from_secs(2)).await;
        }
    }

    async fn execute_remote_command(&self, vm: &VmInstance, command: &str) -> Result<()> {
        info!("Executing remote command on VM {}: {}", vm.name, command);
        
        if self.ssh_credentials.is_none() {
            return Err(anyhow!("No SSH credentials configured. Use LOGIN instruction first."));
        }
        
        // Use tokio::task::spawn_blocking to run SSH in blocking context
        let credentials = self.ssh_credentials.as_ref().unwrap().clone();
        let command_clone = command.to_string();
        
        tokio::task::spawn_blocking(move || {
            Self::ssh_execute_command_with_credentials(&credentials, &command_clone)
        }).await
        .context("Failed to spawn SSH command task")?
        
    }
    
    fn ssh_execute_command_with_credentials(credentials: &SshCredentials, command: &str) -> Result<()> {
        let tcp = std::net::TcpStream::connect(format!("{}:{}", credentials.host, credentials.port))
            .context("Failed to connect to VM via SSH")?;
        
        let mut sess = Session::new()
            .context("Failed to create SSH session")?;
        sess.set_tcp_stream(tcp);
        sess.handshake()
            .context("SSH handshake failed")?;
        
        // Try authentication methods in order of preference
        if let Some(ref private_key_path) = credentials.private_key {
            if private_key_path.exists() {
                sess.userauth_pubkey_file(&credentials.username, None, private_key_path, None)
                    .context("SSH private key authentication failed")?;
            } else {
                return Err(anyhow!("SSH private key file not found: {}", private_key_path.display()));
            }
        } else if let Some(ref password) = credentials.password {
            sess.userauth_password(&credentials.username, password)
                .context("SSH password authentication failed")?;
        } else {
            return Err(anyhow!("No SSH credentials provided (need either private key or password)"));
        }
        
        let mut channel = sess.channel_session()
            .context("Failed to create SSH channel")?;
        
        channel.exec(command)
            .context("Failed to execute command via SSH")?;
        
        let mut output = String::new();
        channel.read_to_string(&mut output)
            .context("Failed to read command output")?;
        
        let exit_status = channel.exit_status()
            .context("Failed to get command exit status")?;
        
        channel.wait_close()
            .context("Failed to close SSH channel")?;
        
        if exit_status == 0 {
            info!("Command executed successfully. Output: {}", output.trim());
        } else {
            return Err(anyhow!("Command failed with exit status {}. Output: {}", exit_status, output.trim()));
        }
        
        Ok(())
    }

    async fn copy_file_to_vm(&self, vm: &VmInstance, from: &Path, to: &Path) -> Result<()> {
        info!("Copying {} to VM {} at {}", from.display(), vm.name, to.display());
        
        if !from.exists() {
            return Err(anyhow!("Source file does not exist: {}", from.display()));
        }
        
        if self.ssh_credentials.is_none() {
            return Err(anyhow!("No SSH credentials configured. Use LOGIN instruction first."));
        }
        
        // Use tokio::task::spawn_blocking to run SSH/SCP in blocking context
        let credentials = self.ssh_credentials.as_ref().unwrap().clone();
        let from_path = from.to_path_buf();
        let to_path = to.to_path_buf();
        
        tokio::task::spawn_blocking(move || {
            Self::scp_copy_file_with_credentials(&credentials, &from_path, &to_path)
        }).await
        .context("Failed to spawn SCP file transfer task")?
    }
    
    fn scp_copy_file_with_credentials(credentials: &SshCredentials, from: &Path, to: &Path) -> Result<()> {
        let tcp = std::net::TcpStream::connect(format!("{}:{}", credentials.host, credentials.port))
            .context("Failed to connect to VM via SSH for file transfer")?;
        
        let mut sess = Session::new()
            .context("Failed to create SSH session for file transfer")?;
        sess.set_tcp_stream(tcp);
        sess.handshake()
            .context("SSH handshake failed for file transfer")?;
        
        // Try authentication methods in order of preference
        if let Some(ref private_key_path) = credentials.private_key {
            if private_key_path.exists() {
                sess.userauth_pubkey_file(&credentials.username, None, private_key_path, None)
                    .context("SSH private key authentication failed for file transfer")?;
            } else {
                return Err(anyhow!("SSH private key file not found: {}", private_key_path.display()));
            }
        } else if let Some(ref password) = credentials.password {
            sess.userauth_password(&credentials.username, password)
                .context("SSH password authentication failed for file transfer")?;
        } else {
            return Err(anyhow!("No SSH credentials provided for file transfer (need either private key or password)"));
        }
        
        // Read the source file
        let file_contents = std::fs::read(from)
            .context("Failed to read source file")?;
        
        // Get file metadata for permissions
        let metadata = std::fs::metadata(from)
            .context("Failed to get source file metadata")?;
        
        // Create the remote file using SCP
        let mut remote_file = sess.scp_send(to, 0o644, file_contents.len() as u64, None)
            .context("Failed to create remote file via SCP")?;
        
        remote_file.write_all(&file_contents)
            .context("Failed to write file contents via SCP")?;
        
        // Close the file and wait for completion
        remote_file.send_eof()
            .context("Failed to send EOF via SCP")?;
        remote_file.wait_eof()
            .context("Failed to wait for EOF via SCP")?;
        remote_file.close()
            .context("Failed to close SCP channel")?;
        remote_file.wait_close()
            .context("Failed to wait for SCP channel close")?;
        
        info!("File copied successfully to VM: {} -> {}", from.display(), to.display());
        Ok(())
    }

    fn parse_key_action(&self, key: &str) -> Result<KeypressAction> {
        match key.to_lowercase().as_str() {
            "enter" => Ok(KeypressAction::Key("Return".to_string())),
            "tab" => Ok(KeypressAction::Key("Tab".to_string())),
            "space" => Ok(KeypressAction::Key("space".to_string())),
            "esc" | "escape" => Ok(KeypressAction::Key("Escape".to_string())),
            "up" => Ok(KeypressAction::Key("Up".to_string())),
            "down" => Ok(KeypressAction::Key("Down".to_string())),
            "left" => Ok(KeypressAction::Key("Left".to_string())),
            "right" => Ok(KeypressAction::Key("Right".to_string())),
            "f1" => Ok(KeypressAction::Key("F1".to_string())),
            "f2" => Ok(KeypressAction::Key("F2".to_string())),
            "f3" => Ok(KeypressAction::Key("F3".to_string())),
            "f4" => Ok(KeypressAction::Key("F4".to_string())),
            "f5" => Ok(KeypressAction::Key("F5".to_string())),
            "f6" => Ok(KeypressAction::Key("F6".to_string())),
            "f7" => Ok(KeypressAction::Key("F7".to_string())),
            "f8" => Ok(KeypressAction::Key("F8".to_string())),
            "f9" => Ok(KeypressAction::Key("F9".to_string())),
            "f10" => Ok(KeypressAction::Key("F10".to_string())),
            "f11" => Ok(KeypressAction::Key("F11".to_string())),
            "f12" => Ok(KeypressAction::Key("F12".to_string())),
            
            // Handle key combinations
            key if key.contains("+") => {
                let parts: Vec<&str> = key.split('+').collect();
                if parts.len() == 2 {
                    let modifier = parts[0];
                    let base_key = parts[1];
                    Ok(KeypressAction::KeyCombo(modifier.to_string(), base_key.to_string()))
                } else {
                    Err(anyhow!("Invalid key combination format: {}", key))
                }
            }
            
            // Single character keys
            single if single.len() == 1 => {
                Ok(KeypressAction::Key(single.to_string()))
            }
            
            _ => Err(anyhow!("Unknown key: {}", key))
        }
    }

    fn parse_duration(&self, duration: &str) -> Result<Duration> {
        let duration_lower = duration.to_lowercase();
        if duration_lower.ends_with("s") {
            let secs: u64 = duration_lower.trim_end_matches("s").parse()
                .context("Invalid seconds format")?;
            Ok(Duration::from_secs(secs))
        } else if duration_lower.ends_with("m") {
            let mins: u64 = duration_lower.trim_end_matches("m").parse()
                .context("Invalid minutes format")?;
            Ok(Duration::from_secs(mins * 60))
        } else if duration_lower.ends_with("h") {
            let hours: u64 = duration_lower.trim_end_matches("h").parse()
                .context("Invalid hours format")?;
            Ok(Duration::from_secs(hours * 3600))
        } else if duration_lower.ends_with("ms") {
            let millis: u64 = duration_lower.trim_end_matches("ms").parse()
                .context("Invalid milliseconds format")?;
            Ok(Duration::from_millis(millis))
        } else {
            Err(anyhow!("Invalid duration format: {}", duration))
        }
    }
}