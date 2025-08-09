use anyhow::{anyhow, Context, Result};
use clap::error;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tracing::{debug, info, warn, error};

use crate::automation::keypress::{KeypressExecutor, KeypressAction};
use crate::automation::vm::{VmInstance, VmManager};
use crate::automation::ocr::OcrEngine;
use crate::config::{Instruction, Stage};
use crate::utils::template::TemplateEngine;

pub struct PuppetManager {
    keypress_executor: KeypressExecutor,
    template_engine: TemplateEngine,
    environment_vars: HashMap<String, String>,
    ocr_engine: OcrEngine,
}

impl PuppetManager {
    pub fn new() -> Self {
        Self {
            keypress_executor: KeypressExecutor::new(),
            template_engine: TemplateEngine::new(),
            environment_vars: std::env::vars().collect(),
            ocr_engine: OcrEngine::new(),
        }
    }

    pub async fn execute_stage_instructions(&mut self, vm: &VmInstance, stage: &Stage, vm_manager: &VmManager) -> Result<()> {
        info!("Executing puppet instructions for stage: {:?}", stage.name);

        for (i, instruction) in stage.instructions.iter().enumerate() {
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
        // Parse different condition types
        match condition {
            text if text.contains("login") => {
                self.wait_for_login_prompt(vm, vm_manager).await
            }
            text if text.contains("desktop") => {
                self.wait_for_desktop(vm, vm_manager).await
            }
            pattern => {
                self.wait_for_screen_text(vm, pattern, vm_manager).await
            }
        }
    }

    async fn wait_for_login_prompt(&self, vm: &VmInstance, vm_manager: &VmManager) -> Result<()> {
        info!("Waiting for login prompt on VM {}", vm.name);
        
        let max_attempts = 90; // Try for up to 3 minutes
        let mut attempts = 0;
        
        // Common login prompt patterns
        let login_patterns = ["login:", "username:", "password:", "sign in", "log in"];
        
        while attempts < max_attempts {
            attempts += 1;
            debug!("Login prompt detection attempt {}/{}", attempts, max_attempts);
            
            // First try screen capture (this will catch graphical login screens)
            if let Ok(image) = vm_manager.capture_screen(vm).await {
                for pattern in &login_patterns {
                    if let Ok(true) = self.ocr_engine.contains_text(&image, pattern).await {
                        info!("Found login prompt pattern '{}' via screen capture", pattern);
                        return Ok(());
                    }
                }
                
                // Also check for common visual login elements using OCR fallback patterns
                if let Ok(text) = self.ocr_engine.extract_text(&image).await {
                    let text_lower = text.to_lowercase();
                    if text_lower.contains("login") || text_lower.contains("username") || 
                       text_lower.contains("password") || text_lower.contains("sign in") {
                        info!("Found login-related text via OCR");
                        return Ok(());
                    }
                }
            }
            
            // Also check console output (for text-based login prompts)
            if let Ok(console_output) = vm_manager.get_console_output(vm).await {
                let console_lower = console_output.to_lowercase();
                for pattern in &login_patterns {
                    if console_lower.contains(pattern) {
                        info!("Found login prompt pattern '{}' in console output", pattern);
                        return Ok(());
                    }
                }
            }
            
            sleep(Duration::from_secs(2)).await;
        }
        
        Err(anyhow!("Timeout waiting for login prompt on VM {}", vm.name))
    }

    async fn wait_for_desktop(&self, vm: &VmInstance, vm_manager: &VmManager) -> Result<()> {
        info!("Waiting for desktop on VM {}", vm.name);
        
        let max_attempts = 120; // Try for up to 4 minutes
        let mut attempts = 0;
        
        // Desktop environment patterns
        let desktop_patterns = ["desktop", "taskbar", "start menu", "wallpaper"];
        
        while attempts < max_attempts {
            attempts += 1;
            debug!("Desktop detection attempt {}/{}", attempts, max_attempts);
            
            // Desktop environments are primarily graphical, so screen capture is essential
            if let Ok(image) = vm_manager.capture_screen(vm).await {
                // Use the OCR engine's desktop pattern detection
                if let Ok(text) = self.ocr_engine.extract_text(&image).await {
                    let text_lower = text.to_lowercase();
                    
                    // Check for desktop environment indicators
                    for pattern in &desktop_patterns {
                        if text_lower.contains(pattern) {
                            info!("Found desktop pattern '{}' via OCR", pattern);
                            return Ok(());
                        }
                    }
                    
                    // Look for common desktop elements
                    if text_lower.contains("menu") || text_lower.contains("file") || 
                       text_lower.contains("start") || text_lower.contains("time") ||
                       text.contains("AM") || text.contains("PM") {
                        info!("Found desktop environment indicators");
                        return Ok(());
                    }
                }
                
                // Use OCR engine's built-in desktop pattern detection
                if let Ok(true) = self.ocr_engine.contains_text(&image, "desktop").await {
                    info!("OCR engine detected desktop environment");
                    return Ok(());
                }
            }
            
            sleep(Duration::from_secs(2)).await;
        }
        
        Err(anyhow!("Timeout waiting for desktop environment on VM {}", vm.name))
    }

    async fn wait_for_screen_text(&self, vm: &VmInstance, pattern: &str, vm_manager: &VmManager) -> Result<()> {
        info!("Waiting for screen text '{}' on VM {}", pattern, vm.name);
        
        let max_attempts = 60; // Try for up to 60 attempts (about 2 minutes with delays)
        let mut attempts = 0;
        
        while attempts < max_attempts {
            attempts += 1;
            debug!("Screen text detection attempt {}/{}", attempts, max_attempts);
            
            // Capture the VM screen
            match vm_manager.capture_screen(vm).await {
                Ok(image) => {
                    // Extract all text to see what OCR is finding
                    match self.ocr_engine.extract_text(&image).await {
                        Ok(extracted_text) => {
                            if attempts <= 3 || attempts % 10 == 0 {
                                info!("OCR extracted text (attempt {}): '{}'", attempts, extracted_text);
                            }
                            
                            // Check if pattern is found in the extracted text (case-insensitive)
                            if extracted_text.to_lowercase().contains(&pattern.to_lowercase()) {
                                info!("Found screen text '{}' on VM {} (attempt {})", pattern, vm.name, attempts);
                                return Ok(());
                            } else {
                                debug!("Pattern '{}' not found in extracted text (attempt {})", pattern, attempts);
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
        
        Err(anyhow!("Timeout waiting for screen text '{}' on VM {}", pattern, vm.name))
    }

    async fn execute_remote_command(&self, vm: &VmInstance, command: &str) -> Result<()> {
        info!("Executing remote command on VM {}: {}", vm.name, command);
        
        // Implementation would use SSH or other remote execution method
        // This is a simplified placeholder
        debug!("Would execute: {}", command);
        
        // Simulate command execution time
        sleep(Duration::from_millis(500)).await;
        
        Ok(())
    }

    async fn copy_file_to_vm(&self, vm: &VmInstance, from: &Path, to: &Path) -> Result<()> {
        info!("Copying {} to VM {} at {}", from.display(), vm.name, to.display());
        
        // Implementation would use SCP or shared folders
        // This is a simplified placeholder
        debug!("Would copy file from {} to {}", from.display(), to.display());
        
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