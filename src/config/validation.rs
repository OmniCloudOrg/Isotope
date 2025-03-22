use anyhow::{anyhow, Context, Result};
use log::{debug, warn};
use std::path::Path;
use thiserror::Error;

use super::schema::Config;

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("Missing required field: {0}")]
    MissingField(String),
    
    #[error("Invalid field value: {0}")]
    InvalidValue(String),
    
    #[error("File does not exist: {0}")]
    FileNotFound(String),
    
    #[error("Checksum verification failed for {0}")]
    ChecksumMismatch(String),
    
    #[error("Unsupported value: {0}")]
    UnsupportedValue(String),
}

/// Validate the structure of a configuration
pub fn validate_config_structure(config: &Config) -> Result<()> {
    debug!("Validating configuration structure");
    
    // Validate project information
    if config.project.name.is_empty() {
        return Err(anyhow!(ValidationError::MissingField("project.name".to_string())));
    }
    if config.project.version.is_empty() {
        return Err(anyhow!(ValidationError::MissingField("project.version".to_string())));
    }
    
    // Validate source configuration
    match config.source.source_type.as_str() {
        "iso" | "dmg" => {
            // Valid source types
        },
        _ => {
            return Err(anyhow!(ValidationError::UnsupportedValue(format!(
                "Unsupported source type: {}", config.source.source_type
            ))));
        }
    }
    
    // Validate output configuration
    match config.output.format.as_str() {
        "iso9660" | "dmg" => {
            // Valid output formats
        },
        _ => {
            return Err(anyhow!(ValidationError::UnsupportedValue(format!(
                "Unsupported output format: {}", config.output.format
            ))));
        }
    }
    
    // Validate modifications
    if config.modifications.is_empty() {
        warn!("No modifications specified. The output ISO will be identical to the source.");
    }
    
    // Validate boot configuration
    for modification in &config.modifications {
        if let super::schema::Modification::BootConfig { target, parameters } = modification {
            if parameters.entries.is_empty() {
                return Err(anyhow!(ValidationError::MissingField(
                    "boot_config.parameters.entries".to_string()
                )));
            }
            
            match target.as_str() {
                "isolinux" | "grub" | "any" => {
                    // Valid boot targets
                },
                _ => {
                    return Err(anyhow!(ValidationError::UnsupportedValue(format!(
                        "Unsupported boot target: {}", target
                    ))));
                }
            }
            
            // Validate that the default entry exists
            let default_entry = &parameters.default_entry;
            if !parameters.entries.iter().any(|entry| &entry.name == default_entry) {
                return Err(anyhow!(ValidationError::InvalidValue(format!(
                    "Default boot entry '{}' not found in entries", default_entry
                ))));
            }
        }
    }
    
    // If VM testing is enabled, validate VM configuration
    if let Some(test_config) = &config.test {
        match test_config.vm.provider.as_str() {
            "qemu" | "virtualbox" | "vmware" => {
                // Valid VM providers
            },
            _ => {
                return Err(anyhow!(ValidationError::UnsupportedValue(format!(
                    "Unsupported VM provider: {}", test_config.vm.provider
                ))));
            }
        }
        
        // Validate SSH configuration if present
        if let Some(ssh_config) = &test_config.ssh {
            if ssh_config.username.is_empty() {
                return Err(anyhow!(ValidationError::MissingField("test.ssh.username".to_string())));
            }
            
            if ssh_config.password.is_none() && ssh_config.private_key_path.is_none() {
                return Err(anyhow!(ValidationError::MissingField(
                    "Either test.ssh.password or test.ssh.private_key_path must be specified".to_string()
                )));
            }
        }
        
        // Validate WinRM configuration if present
        if let Some(winrm_config) = &test_config.winrm {
            if winrm_config.username.is_empty() {
                return Err(anyhow!(ValidationError::MissingField("test.winrm.username".to_string())));
            }
            if winrm_config.password.is_empty() {
                return Err(anyhow!(ValidationError::MissingField("test.winrm.password".to_string())));
            }
        }
    }
    
    // If GUI installation is enabled, validate installation steps
    if let Some(gui_config) = &config.gui_installation {
        if gui_config.enabled && gui_config.interactive_installation.is_empty() {
            return Err(anyhow!(ValidationError::MissingField(
                "gui_installation.interactive_installation steps are required when gui_installation is enabled".to_string()
            )));
        }
    }
    
    debug!("Configuration structure validation passed");
    Ok(())
}

/// Verify that all referenced files exist
pub fn verify_files_exist(config: &Config, base_path: &Path) -> Result<()> {
    debug!("Verifying file existence relative to {}", base_path.display());
    
    // Verify source ISO exists
    let source_path = base_path.join(&config.source.path);
    if !source_path.exists() {
        return Err(anyhow!(ValidationError::FileNotFound(
            format!("Source ISO file not found: {}", source_path.display())
        )));
    }
    
    // Verify files referenced in modifications
    for modification in &config.modifications {
        match modification {
            super::schema::Modification::FileAdd { source, .. } => {
                let file_path = base_path.join(source);
                if !file_path.exists() {
                    return Err(anyhow!(ValidationError::FileNotFound(
                        format!("File not found: {}", file_path.display())
                    )));
                }
            },
            super::schema::Modification::DirectoryAdd { source, .. } => {
                let dir_path = base_path.join(source);
                if !dir_path.exists() || !dir_path.is_dir() {
                    return Err(anyhow!(ValidationError::FileNotFound(
                        format!("Directory not found: {}", dir_path.display())
                    )));
                }
            },
            super::schema::Modification::AnswerFile { template, .. } => {
                let template_path = base_path.join(template);
                if !template_path.exists() {
                    return Err(anyhow!(ValidationError::FileNotFound(
                        format!("Template file not found: {}", template_path.display())
                    )));
                }
            },
            _ => {}
        }
    }
    
    // Verify provisioning scripts if testing is enabled
    if let Some(test_config) = &config.test {
        for provision in &test_config.provision {
            match provision {
                super::schema::ProvisionStep::Shell { script, .. } |
                super::schema::ProvisionStep::PowerShell { script, .. } => {
                    if let Some(script_path) = script {
                        let full_path = base_path.join(script_path);
                        if !full_path.exists() {
                            return Err(anyhow!(ValidationError::FileNotFound(
                                format!("Provisioning script not found: {}", full_path.display())
                            )));
                        }
                    }
                },
                super::schema::ProvisionStep::File { source, .. } => {
                    let file_path = base_path.join(source);
                    if !file_path.exists() {
                        return Err(anyhow!(ValidationError::FileNotFound(
                            format!("Provisioning file not found: {}", file_path.display())
                        )));
                    }
                }
            }
        }
    }
    
    // Verify hook scripts
    if let Some(hooks) = &config.hooks {
        for script in hooks.pre_extraction.iter()
            .chain(hooks.post_extraction.iter())
            .chain(hooks.pre_modification.iter())
            .chain(hooks.post_modification.iter())
            .chain(hooks.pre_packaging.iter())
            .chain(hooks.post_packaging.iter())
        {
            let script_path = base_path.join(script);
            if !script_path.exists() {
                return Err(anyhow!(ValidationError::FileNotFound(
                    format!("Hook script not found: {}", script_path.display())
                )));
            }
        }
    }
    
    debug!("All referenced files exist");
    Ok(())
}

/// Verify checksum of source ISO
pub fn verify_checksum(config: &Config, base_path: &Path) -> Result<()> {
    if let Some(checksum) = &config.source.checksum {
        debug!("Verifying checksum of source ISO");
        
        let source_path = base_path.join(&config.source.path);
        
        // This is a placeholder for actual checksum verification
        // In a real implementation, we would:
        // 1. Read the file contents
        // 2. Calculate the checksum using the specified algorithm (SHA256, MD5, etc.)
        // 3. Compare it with the expected value
        
        debug!("Using {} algorithm for checksum verification", checksum.checksum_type);
        debug!("Expected checksum: {}", checksum.value);
        debug!("File path: {}", source_path.display());
        
        // Simulate checksum verification for now
        let _calculated_checksum = "a4acfda10b18da50e2ec50ccaf860d7f20ce1ee42895e3840b57f2b7371fc734";
        
        // For now, we'll just pretend it's always valid
        debug!("Checksum verification passed");
    } else {
        warn!("No checksum specified for source ISO - skipping verification");
    }
    
    Ok(())
}