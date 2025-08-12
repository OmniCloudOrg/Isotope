#![allow(dead_code)]

use anyhow::{anyhow, Context, Result};
use ssh2::Session;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, timeout};
use tracing::{debug, error, info, trace, warn};

use crate::automation::keypress::{KeypressAction, KeypressExecutor};
use crate::automation::ocr::OcrEngine;
use crate::automation::vm::{VmInstance, VmManager};
use crate::config::{Instruction, Stage};
use crate::utils::template::TemplateEngine;

#[derive(Debug, Clone)]
pub struct SshCredentials {
    pub username: String,
    pub password: Option<String>,
    pub private_key: Option<PathBuf>,
}

pub struct PuppetManager {
    keypress_executor: KeypressExecutor,
    template_engine: TemplateEngine,
    environment_vars: HashMap<String, String>,
    ocr_engine: OcrEngine,
    ssh_credentials: Option<SshCredentials>,
    debug_steps_dir: PathBuf,
    step_counter: usize,
    ocr_debug_enabled: bool,
}

impl PuppetManager {
    pub fn new() -> Self {
        Self::new_with_ocr_debug(false)
    }

    pub fn new_with_ocr_debug(ocr_debug_enabled: bool) -> Self {
        let debug_dir = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("debug-steps");
        
        // Create debug-steps directory if it doesn't exist
        if !debug_dir.exists() {
            let _ = fs::create_dir_all(&debug_dir);
        }
        
        Self {
            keypress_executor: KeypressExecutor::new(),
            template_engine: TemplateEngine::new(),
            environment_vars: std::env::vars().collect(),
            ocr_engine: OcrEngine::new(),
            ssh_credentials: None,
            debug_steps_dir: debug_dir,
            step_counter: 0,
            ocr_debug_enabled,
        }
    }

    pub async fn execute_stage_instructions(
        &mut self,
        vm: &VmInstance,
        stage: &Stage,
        vm_manager: &VmManager,
    ) -> Result<()> {
        self.execute_stage_instructions_from_step(vm, stage, vm_manager, None)
            .await
    }

    pub async fn execute_stage_instructions_from_step(
        &mut self,
        vm: &VmInstance,
        stage: &Stage,
        vm_manager: &VmManager,
        continue_from_step: Option<usize>,
    ) -> Result<()> {
        info!("Executing puppet instructions for stage: {:?}", stage.name);

        let start_from = if let Some(step) = continue_from_step {
            if step == 0 {
                return Err(anyhow!(
                    "Step numbers are 1-based, cannot continue from step 0"
                ));
            }
            let index = step - 1; // Convert to 0-based index
            if index >= stage.instructions.len() {
                return Err(anyhow!(
                    "Cannot continue from step {}, stage only has {} instructions",
                    step,
                    stage.instructions.len()
                ));
            }
            info!("Continuing from step {}/{}", step, stage.instructions.len());
            index
        } else {
            0
        };

        for (i, instruction) in stage.instructions.iter().enumerate().skip(start_from) {
            self.step_counter += 1;
            info!(
                "Executing instruction {}/{} (step {}): {:?}",
                i + 1,
                stage.instructions.len(),
                self.step_counter,
                instruction
            );

            // Capture pre-step screenshot
            self.capture_debug_screenshot(vm, "pre", self.step_counter, vm_manager).await?;

            match instruction {
                // OS Installation instructions (keypress automation)
                Instruction::Wait {
                    duration,
                    condition,
                } => {
                    self.execute_wait_instruction(vm, duration, condition.as_ref(), vm_manager)
                        .await?;
                }
                Instruction::Press {
                    key,
                    repeat,
                    modifiers,
                } => {
                    self.execute_press_instruction(vm, key, *repeat, modifiers, vm_manager)
                        .await?;
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
                Instruction::Login {
                    username,
                    password,
                    private_key,
                    ..
                } => {
                    self.ssh_credentials = Some(SshCredentials {
                        username: username.clone(),
                        password: password.clone(),
                        private_key: private_key.clone(),
                    });
                    info!("SSH credentials configured for {}", username);
                }

                _ => {
                    warn!(
                        "Ignoring unsupported instruction in puppet execution: {:?}",
                        instruction
                    );
                }
            }
            
            // Capture post-step screenshot
            self.capture_debug_screenshot(vm, "post", self.step_counter, vm_manager).await?;
        }

        info!("Completed puppet execution for stage");
        Ok(())
    }

    async fn execute_wait_instruction(
        &self,
        vm: &VmInstance,
        duration: &str,
        condition: Option<&String>,
        vm_manager: &VmManager,
    ) -> Result<()> {
        let wait_duration = self.parse_duration(duration)?;

        if let Some(condition_text) = condition {
            info!(
                "Waiting up to {} for condition: {}",
                duration, condition_text
            );

            // Wait with condition checking
            let result = timeout(wait_duration, async {
                self.wait_for_condition(vm, condition_text, vm_manager)
                    .await
            })
            .await;

            match result {
                Ok(Ok(())) => {
                    info!("Condition '{}' met successfully", condition_text);
                    // Capture notice frame when condition is satisfied
                    self.capture_debug_screenshot(vm, "notice", self.step_counter, vm_manager).await?;
                }
                Ok(Err(e)) => {
                    return Err(anyhow!(
                        "Error while waiting for condition '{}': {}",
                        condition_text,
                        e
                    ));
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

    async fn execute_press_instruction(
        &mut self,
        vm: &VmInstance,
        key: &str,
        repeat: Option<u32>,
        modifiers: &Option<Vec<String>>,
        vm_manager: &VmManager,
    ) -> Result<()> {
        let repeat_count = repeat.unwrap_or(1);

        // Check if this is a key combination with modifiers
        if let Some(modifier_list) = modifiers {
            if !modifier_list.is_empty() {
                for i in 0..repeat_count {
                    if repeat_count > 1 {
                        debug!(
                            "Pressing key combination '{:?}+{}' ({}/{})",
                            modifier_list,
                            key,
                            i + 1,
                            repeat_count
                        );
                    } else {
                        debug!("Pressing key combination '{:?}+{}'", modifier_list, key);
                    }

                    let action = KeypressAction::KeyCombo(modifier_list.clone(), key.to_string());
                    self.keypress_executor
                        .execute_action(vm, &action, vm_manager)
                        .await?;

                    // Small delay between repeated keypresses
                    if i < repeat_count - 1 {
                        sleep(Duration::from_millis(100)).await;
                    }
                }
                return Ok(());
            }
        }

        // Regular key press
        for i in 0..repeat_count {
            if repeat_count > 1 {
                debug!("Pressing key '{}' ({}/{})", key, i + 1, repeat_count);
            } else {
                debug!("Pressing key '{}'", key);
            }

            let action = self.parse_key_action(key)?;
            self.keypress_executor
                .execute_action(vm, &action, vm_manager)
                .await?;

            // Small delay between repeated keypresses
            if i < repeat_count - 1 {
                sleep(Duration::from_millis(100)).await;
            }
        }

        Ok(())
    }

    async fn execute_type_instruction(
        &mut self,
        vm: &VmInstance,
        text: &str,
        vm_manager: &VmManager,
    ) -> Result<()> {
        // Process template variables in text
        let processed_text = self
            .template_engine
            .render_string(text, &self.environment_vars)?;

        debug!("Typing text: {}", processed_text);

        let action = KeypressAction::TypeText(processed_text);
        self.keypress_executor
            .execute_action(vm, &action, vm_manager)
            .await?;

        Ok(())
    }

    async fn execute_run_instruction(&mut self, vm: &VmInstance, command: &str) -> Result<()> {
        // Process template variables in command
        let processed_command = self
            .template_engine
            .render_string(command, &self.environment_vars)?;
        info!("RUN: Executing command in live OS: {}", processed_command);
        // Execute command via SSH/remote connection
        match self.execute_remote_command(vm, &processed_command).await {
            Ok(_) => Ok(()),
            Err(e) => {
                let ssh_info = if let Some(creds) = &self.ssh_credentials {
                    // Get actual endpoint from provider to ensure accurate error reporting
                    let provider = crate::automation::vm::providers::create_provider(&vm.provider);
                    let (host, port) = provider.get_ssh_endpoint(vm);
                    format!(
                        "user='{}' host='{}' port='{}'",
                        creds.username, host, port
                    )
                } else {
                    "<no ssh credentials configured>".to_string()
                };
                error!(
                    "RUN: Command failed: {}\nError: {}\nSSH: {}",
                    processed_command, e, ssh_info
                );
                error!("RUN: Troubleshooting tips: Check if the VM is running, SSH is enabled, network is accessible, and credentials are correct.");
                Err(anyhow!(
                    "RUN failed: '{}': {}\nSSH: {}",
                    processed_command,
                    e,
                    ssh_info
                ))
            }
        }
    }

    async fn execute_copy_instruction(
        &mut self,
        vm: &VmInstance,
        from: &Path,
        to: &Path,
    ) -> Result<()> {
        info!(
            "COPY: Copying file {} to VM path {}",
            from.display(),
            to.display()
        );
        if !from.exists() {
            error!("COPY: Source file does not exist: {}", from.display());
            return Err(anyhow!(
                "COPY failed: Source file does not exist: {}",
                from.display()
            ));
        }
        // Copy file to VM via SCP/remote copy
        match self.copy_file_to_vm(vm, from, to).await {
            Ok(_) => Ok(()),
            Err(e) => {
                error!(
                    "COPY: Failed to copy {} to {}: {}",
                    from.display(),
                    to.display(),
                    e
                );
                Err(anyhow!(
                    "COPY failed: {} -> {}: {}",
                    from.display(),
                    to.display(),
                    e
                ))
            }
        }
    }

    async fn wait_for_condition(
        &self,
        vm: &VmInstance,
        condition: &str,
        vm_manager: &VmManager,
    ) -> Result<()> {
        // Just wait for the exact text the user specified - no hardcoded logic
        self.wait_for_screen_text(vm, condition, vm_manager).await
    }

    async fn wait_for_screen_text(
        &self,
        vm: &VmInstance,
        pattern: &str,
        vm_manager: &VmManager,
    ) -> Result<()> {
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
                            if self.ocr_debug_enabled && (attempts <= 3 || attempts % 10 == 0) {
                                trace!(
                                    "OCR extracted text (attempt {}): '{}'",
                                    attempts,
                                    extracted_text
                                );
                            }

                            // Check if pattern is found in the extracted text (case-insensitive)
                            if extracted_text
                                .to_lowercase()
                                .contains(&pattern.to_lowercase())
                            {
                                if self.ocr_debug_enabled {
                                    trace!(
                                        "Found screen text '{}' on VM {} (attempt {})",
                                        pattern,
                                        vm.name,
                                        attempts
                                    );
                                }
                                return Ok(());
                            } else if self.ocr_debug_enabled {
                                trace!(
                                    "Pattern '{}' not found in extracted text (attempt {})",
                                    pattern,
                                    attempts
                                );
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
                        if console_output
                            .to_lowercase()
                            .contains(&pattern.to_lowercase())
                        {
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
            return Err(anyhow!(
                "No SSH credentials configured. Use LOGIN instruction first."
            ));
        }
        
        // Get endpoint from provider
        let provider = crate::automation::vm::providers::create_provider(&vm.provider);
        let (host, port) = provider.get_ssh_endpoint(vm);
        
        info!("SSH connection details: {}:{}", host, port);
        
        let credentials = self.ssh_credentials.as_ref().unwrap().clone();
        let command_clone = command.to_string();
        tokio::task::spawn_blocking(move || {
            Self::ssh_execute_command_with_endpoint(&credentials, &host, port, &command_clone)
        })
        .await
        .context("Failed to spawn SSH command task")?
    }

    fn ssh_execute_command_with_endpoint(
        credentials: &SshCredentials,
        host: &str,
        port: u16,
        command: &str,
    ) -> Result<()> {
        // Attempt TCP connection with detailed error info
        let tcp = std::net::TcpStream::connect(format!("{}:{}", host, port))
            .context(format!("Failed to connect to VM via SSH at {}:{}", host, port))?;
            
        let mut sess = Session::new().context("Failed to create SSH session")?;
        sess.set_tcp_stream(tcp);
        sess.handshake()
            .context(format!("SSH handshake failed to {}:{}", host, port))?;
        // Try authentication methods in order of preference
        if let Some(ref private_key_path) = credentials.private_key {
            if private_key_path.exists() {
                sess.userauth_pubkey_file(&credentials.username, None, private_key_path, None)
                    .context("SSH private key authentication failed")?;
            } else {
                return Err(anyhow!(
                    "SSH private key file not found: {}",
                    private_key_path.display()
                ));
            }
        } else if let Some(ref password) = credentials.password {
            sess.userauth_password(&credentials.username, password)
                .context("SSH password authentication failed")?;
        } else {
            return Err(anyhow!(
                "No SSH credentials provided (need either private key or password)"
            ));
        }
        let mut channel = sess
            .channel_session()
            .context("Failed to create SSH channel")?;
        channel
            .exec(command)
            .context("Failed to execute command via SSH")?;
        let mut output = String::new();
        channel
            .read_to_string(&mut output)
            .context("Failed to read command output")?;
        let exit_status = channel
            .exit_status()
            .context("Failed to get command exit status")?;
        channel
            .wait_close()
            .context("Failed to close SSH channel")?;
        if exit_status == 0 {
            info!("Command executed successfully. Output: {}", output.trim());
        } else {
            return Err(anyhow!(
                "Command failed with exit status {}. Output: {}",
                exit_status,
                output.trim()
            ));
        }
        Ok(())
    }

    async fn copy_file_to_vm(&self, vm: &VmInstance, from: &Path, to: &Path) -> Result<()> {
        info!(
            "Copying {} to VM {} at {}",
            from.display(),
            vm.name,
            to.display()
        );

        if !from.exists() {
            return Err(anyhow!("Source file does not exist: {}", from.display()));
        }

        if self.ssh_credentials.is_none() {
            return Err(anyhow!(
                "No SSH credentials configured. Use LOGIN instruction first."
            ));
        }

        // Use tokio::task::spawn_blocking to run SSH/SCP in blocking context
        let credentials = self.ssh_credentials.as_ref().unwrap().clone();
        let from_path = from.to_path_buf();
        let to_path = to.to_path_buf();
        let provider = crate::automation::vm::providers::create_provider(&vm.provider);
        let (host, port) = provider.get_ssh_endpoint(vm);
        
        info!("SCP connection details: {}:{}", host, port);
        
        tokio::task::spawn_blocking(move || {
            Self::scp_copy_file_with_endpoint(&credentials, &host, port, &from_path, &to_path)
        })
        .await
        .context("Failed to spawn SCP file transfer task")?
    }

    fn scp_copy_file_with_endpoint(
        credentials: &SshCredentials,
        host: &str,
        port: u16,
        from: &Path,
        to: &Path,
    ) -> Result<()> {
        let tcp = std::net::TcpStream::connect(format!("{}:{}", host, port))
            .context(format!("Failed to connect to VM via SSH for file transfer at {}:{}", host, port))?;
        let mut sess = Session::new().context("Failed to create SSH session for file transfer")?;
        sess.set_tcp_stream(tcp);
        sess.handshake()
            .context(format!("SSH handshake failed for file transfer to {}:{}", host, port))?;
        // Try authentication methods in order of preference
        if let Some(ref private_key_path) = credentials.private_key {
            if private_key_path.exists() {
                sess.userauth_pubkey_file(&credentials.username, None, private_key_path, None)
                    .context("SSH private key authentication failed for file transfer")?;
            } else {
                return Err(anyhow!(
                    "SSH private key file not found: {}",
                    private_key_path.display()
                ));
            }
        } else if let Some(ref password) = credentials.password {
            sess.userauth_password(&credentials.username, password)
                .context("SSH password authentication failed for file transfer")?;
        } else {
            return Err(anyhow!("No SSH credentials provided for file transfer (need either private key or password)"));
        }
        // Read the source file
        let file_contents = std::fs::read(from).context("Failed to read source file")?;

        // Create the remote file using SCP
        let mut remote_file = sess
            .scp_send(to, 0o644, file_contents.len() as u64, None)
            .context("Failed to create remote file via SCP")?;

        remote_file
            .write_all(&file_contents)
            .context("Failed to write file contents via SCP")?;

        // Close the file and wait for completion
        remote_file
            .send_eof()
            .context("Failed to send EOF via SCP")?;
        remote_file
            .wait_eof()
            .context("Failed to wait for EOF via SCP")?;
        remote_file.close().context("Failed to close SCP channel")?;
        remote_file
            .wait_close()
            .context("Failed to wait for SCP channel close")?;

        info!(
            "File copied successfully to VM: {} -> {}",
            from.display(),
            to.display()
        );
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
                if parts.len() >= 2 {
                    // Last part is the key, everything else are modifiers
                    let base_key = parts.last().unwrap();
                    let modifiers: Vec<String> = parts[..parts.len() - 1]
                        .iter()
                        .map(|s| s.to_string())
                        .collect();
                    Ok(KeypressAction::KeyCombo(modifiers, base_key.to_string()))
                } else {
                    Err(anyhow!("Invalid key combination format: {}", key))
                }
            }

            // Single character keys
            single if single.len() == 1 => Ok(KeypressAction::Key(single.to_string())),

            _ => Err(anyhow!("Unknown key: {}", key)),
        }
    }

    fn parse_duration(&self, duration: &str) -> Result<Duration> {
        let duration_lower = duration.to_lowercase();
        if duration_lower.ends_with("s") {
            let secs: u64 = duration_lower
                .trim_end_matches("s")
                .parse()
                .context("Invalid seconds format")?;
            Ok(Duration::from_secs(secs))
        } else if duration_lower.ends_with("m") {
            let mins: u64 = duration_lower
                .trim_end_matches("m")
                .parse()
                .context("Invalid minutes format")?;
            Ok(Duration::from_secs(mins * 60))
        } else if duration_lower.ends_with("h") {
            let hours: u64 = duration_lower
                .trim_end_matches("h")
                .parse()
                .context("Invalid hours format")?;
            Ok(Duration::from_secs(hours * 3600))
        } else if duration_lower.ends_with("ms") {
            let millis: u64 = duration_lower
                .trim_end_matches("ms")
                .parse()
                .context("Invalid milliseconds format")?;
            Ok(Duration::from_millis(millis))
        } else {
            Err(anyhow!("Invalid duration format: {}", duration))
        }
    }

    /// Capture debug screenshot and generate OCR text file
    async fn capture_debug_screenshot(
        &self,
        vm: &VmInstance,
        prefix: &str, // "pre", "post", or "notice"
        step: usize,
        vm_manager: &VmManager,
    ) -> Result<()> {
        debug!("Capturing {} screenshot for step {}", prefix, step);
        
        match vm_manager.capture_screen(vm).await {
            Ok(image) => {
                // Generate timestamp for unique filename
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                    
                let filename_base = format!("{}-{}-{}", prefix, step, timestamp);
                let image_path = self.debug_steps_dir.join(format!("{}.png", filename_base));
                let text_path = self.debug_steps_dir.join(format!("{}.txt", filename_base));
                
                // Save screenshot
                if let Err(e) = image.save(&image_path) {
                    warn!("Failed to save debug screenshot {}: {}", image_path.display(), e);
                    return Ok(());
                }
                
                // Generate OCR text
                match self.ocr_engine.extract_text(&image).await {
                    Ok(ocr_text) => {
                        if let Err(e) = fs::write(&text_path, &ocr_text) {
                            warn!("Failed to save OCR text {}: {}", text_path.display(), e);
                        } else {
                            debug!(
                                "Saved debug files: {} and {}",
                                image_path.display(),
                                text_path.display()
                            );
                        }
                    }
                    Err(e) => {
                        warn!("Failed to extract OCR text for step {}: {}", step, e);
                        // Still save an empty text file to maintain file pairs
                        let _ = fs::write(&text_path, format!("[OCR Error: {}]", e));
                    }
                }
                
                info!("Debug screenshot captured: {}", image_path.display());
            }
            Err(e) => {
                warn!("Failed to capture screen for step {}: {}", step, e);
            }
        }
        
        Ok(())
    }
}
