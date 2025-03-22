use anyhow::{Context, Result};
use log::{debug, info};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::config::schema::{BootParameters, FileAttributes, FileOperation, BinaryPatchOperation};
use crate::utils::fs::copy_directory;
use crate::utils::template::render_template;

/// Handles ISO content modifications
pub struct IsoModifier<'a> {
    extraction_dir: &'a Path,
}

impl<'a> IsoModifier<'a> {
    /// Create a new ISO modifier
    pub fn new(extraction_dir: &'a Path) -> Self {
        Self { extraction_dir }
    }
    
    /// Add a file to the ISO
    pub fn add_file(&self, source: &Path, destination: &str, attributes: Option<&FileAttributes>) -> Result<()> {
        debug!("Adding file: {} -> {}", source.display(), destination);
        
        // Calculate the full destination path
        let dest_path = self.get_full_path(destination);
        
        // Create parent directories if they don't exist
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }
        
        // Copy the file
        fs::copy(source, &dest_path)
            .with_context(|| format!("Failed to copy file: {} -> {}", source.display(), dest_path.display()))?;
        
        // Set file attributes if specified
        if let Some(attrs) = attributes {
            // This is a placeholder for setting permissions, owner, and group
            // In a real implementation, we would use platform-specific functions or crates
            if let Some(perms) = &attrs.permissions {
                debug!("Setting permissions: {}", perms);
                // Simulated permissions setting
            }
            
            if let Some(owner) = &attrs.owner {
                debug!("Setting owner: {}", owner);
                // Simulated owner setting
            }
            
            if let Some(group) = &attrs.group {
                debug!("Setting group: {}", group);
                // Simulated group setting
            }
        }
        
        debug!("File added successfully");
        Ok(())
    }
    
    /// Modify a file in the ISO
    pub fn modify_file(&self, path: &str, operations: &[FileOperation]) -> Result<()> {
        debug!("Modifying file: {}", path);
        
        // Calculate the full path
        let full_path = self.get_full_path(path);
        
        // Read the file content
        let mut content = String::new();
        File::open(&full_path)
            .with_context(|| format!("Failed to open file: {}", full_path.display()))?
            .read_to_string(&mut content)
            .with_context(|| format!("Failed to read file: {}", full_path.display()))?;
        
        // Apply operations
        for (i, operation) in operations.iter().enumerate() {
            debug!("Applying operation {}/{}", i + 1, operations.len());
            
            content = match operation {
                FileOperation::Replace { pattern, replacement } => {
                    debug!("Replacing '{}' with '{}'", pattern, replacement);
                    content.replace(pattern, replacement)
                },
                FileOperation::Append { content: append_content } => {
                    debug!("Appending '{}' to file", append_content);
                    format!("{}{}", content, append_content)
                },
                FileOperation::RegexReplace { pattern, replacement } => {
                    debug!("Regex replacing '{}' with '{}'", pattern, replacement);
                    
                    // This is a placeholder for regex replacement
                    // In a real implementation, we would use the regex crate
                    content.replace(pattern, replacement) // Simplified for now
                },
            };
        }
        
        // Write the modified content back to the file
        File::create(&full_path)
            .with_context(|| format!("Failed to create file: {}", full_path.display()))?
            .write_all(content.as_bytes())
            .with_context(|| format!("Failed to write to file: {}", full_path.display()))?;
        
        debug!("File modified successfully");
        Ok(())
    }
    
    /// Remove a file from the ISO
    pub fn remove_file(&self, path: &str) -> Result<()> {
        debug!("Removing file: {}", path);
        
        // Calculate the full path
        let full_path = self.get_full_path(path);
        
        // Remove the file
        fs::remove_file(&full_path)
            .with_context(|| format!("Failed to remove file: {}", full_path.display()))?;
        
        debug!("File removed successfully");
        Ok(())
    }
    
    /// Add a directory to the ISO
    pub fn add_directory(&self, source: &Path, destination: &str) -> Result<()> {
        debug!("Adding directory: {} -> {}", source.display(), destination);
        
        // Calculate the full destination path
        let dest_path = self.get_full_path(destination);
        
        // Create the destination directory if it doesn't exist
        fs::create_dir_all(&dest_path)
            .with_context(|| format!("Failed to create directory: {}", dest_path.display()))?;
        
        // Copy the directory contents
        copy_directory(source, &dest_path)
            .with_context(|| format!("Failed to copy directory: {} -> {}", source.display(), dest_path.display()))?;
        
        debug!("Directory added successfully");
        Ok(())
    }
    
    /// Add an answer file to the ISO
    pub fn add_answer_file(&self, template: &Path, destination: &str, variables: &HashMap<String, String>) -> Result<()> {
        debug!("Adding answer file: {} -> {}", template.display(), destination);
        
        // Calculate the full destination path
        let dest_path = self.get_full_path(destination);
        
        // Create parent directories if they don't exist
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }
        
        // Read the template file
        let mut template_content = String::new();
        File::open(template)
            .with_context(|| format!("Failed to open template file: {}", template.display()))?
            .read_to_string(&mut template_content)
            .with_context(|| format!("Failed to read template file: {}", template.display()))?;
        
        // Render the template with variables
        let rendered = render_template(&template_content, variables)
            .context("Failed to render template")?;
        
        // Write the rendered template to the destination
        File::create(&dest_path)
            .with_context(|| format!("Failed to create file: {}", dest_path.display()))?
            .write_all(rendered.as_bytes())
            .with_context(|| format!("Failed to write to file: {}", dest_path.display()))?;
        
        debug!("Answer file added successfully");
        Ok(())
    }
    
    /// Apply binary patches to a file
    pub fn apply_binary_patches(&self, path: &str, patches: &[BinaryPatchOperation]) -> Result<()> {
        debug!("Applying binary patches to file: {}", path);
        
        // Calculate the full path
        let full_path = self.get_full_path(path);
        
        // Read the file content
        let mut content = fs::read(&full_path)
            .with_context(|| format!("Failed to read file: {}", full_path.display()))?;
        
        // Apply patches
        for (i, patch) in patches.iter().enumerate() {
            debug!("Applying patch {}/{}", i + 1, patches.len());
            
            // Parse hex offset
            let offset = if patch.offset.starts_with("0x") || patch.offset.starts_with("0X") {
                u64::from_str_radix(&patch.offset[2..], 16)
                    .with_context(|| format!("Failed to parse hex offset: {}", patch.offset))?
            } else {
                patch.offset.parse::<u64>()
                    .with_context(|| format!("Failed to parse offset: {}", patch.offset))?
            };
            
            // Parse original bytes
            let original_bytes = parse_hex_bytes(&patch.original)
                .with_context(|| format!("Failed to parse original bytes: {}", patch.original))?;
            
            // Parse replacement bytes
            let replacement_bytes = parse_hex_bytes(&patch.replacement)
                .with_context(|| format!("Failed to parse replacement bytes: {}", patch.replacement))?;
            
            // Verify original bytes match
            let start = offset as usize;
            let end = start + original_bytes.len();
            
            if end > content.len() {
                return Err(anyhow::anyhow!("Offset out of bounds: {} + {} > {}", 
                    offset, original_bytes.len(), content.len()));
            }
            
            let actual_bytes = &content[start..end];
            if actual_bytes != original_bytes.as_slice() {
                return Err(anyhow::anyhow!("Original bytes don't match at offset {}: expected {:?}, found {:?}", 
                    offset, original_bytes, actual_bytes));
            }
            
            // Apply the patch
            if original_bytes.len() != replacement_bytes.len() {
                return Err(anyhow::anyhow!(
                    "Original and replacement byte lengths don't match: {} != {}", 
                    original_bytes.len(), replacement_bytes.len()));
            }
            
            for (j, &byte) in replacement_bytes.iter().enumerate() {
                content[start + j] = byte;
            }
        }
        
        // Write the modified content back to the file
        fs::write(&full_path, &content)
            .with_context(|| format!("Failed to write to file: {}", full_path.display()))?;
        
        debug!("Binary patches applied successfully");
        Ok(())
    }
    
    /// Configure boot options
    pub fn configure_boot(&self, target: &str, parameters: &BootParameters) -> Result<()> {
        debug!("Configuring boot for target: {}", target);
        
        // This is a placeholder for boot configuration
        // In a real implementation, we would:
        // 1. Detect the boot loader type
        // 2. Find the boot configuration file
        // 3. Modify it according to the specified parameters
        
        match target {
            "isolinux" => {
                // Configure ISOLINUX boot loader
                let isolinux_cfg = self.get_full_path("/isolinux/isolinux.cfg");
                
                if !isolinux_cfg.exists() {
                    return Err(anyhow::anyhow!("ISOLINUX configuration file not found: {}", isolinux_cfg.display()));
                }
                
                // Generate ISOLINUX configuration content
                let mut content = format!("DEFAULT {}\n", parameters.default_entry);
                content.push_str(&format!("TIMEOUT {}\n", parameters.timeout * 10)); // ISOLINUX uses tenths of a second
                
                // Add menu entries
                for entry in &parameters.entries {
                    content.push_str(&format!("\nLABEL {}\n", entry.name));
                    content.push_str(&format!("  MENU LABEL {}\n", entry.label));
                    content.push_str("  KERNEL /isolinux/vmlinuz\n");
                    content.push_str(&format!("  APPEND initrd=/isolinux/initrd.img {}\n", entry.kernel_params));
                }
                
                // Write the configuration
                fs::write(&isolinux_cfg, content)
                    .with_context(|| format!("Failed to write ISOLINUX configuration: {}", isolinux_cfg.display()))?;
                
                debug!("ISOLINUX boot configuration updated");
            },
            "grub" => {
                // Configure GRUB boot loader
                let grub_cfg = self.get_full_path("/boot/grub/grub.cfg");
                
                if !grub_cfg.exists() {
                    return Err(anyhow::anyhow!("GRUB configuration file not found: {}", grub_cfg.display()));
                }
                
                // Generate GRUB configuration content
                let mut content = format!("set default=\"{}\"\n", parameters.default_entry);
                content.push_str(&format!("set timeout={}\n", parameters.timeout));
                
                // Add menu entries
                for entry in &parameters.entries {
                    content.push_str(&format!("\nmenuentry \"{}\" {{\n", entry.label));
                    content.push_str("  linux /boot/vmlinuz");
                    content.push_str(&format!(" {}\n", entry.kernel_params));
                    content.push_str("  initrd /boot/initrd.img\n");
                    content.push_str("}\n");
                }
                
                // Write the configuration
                fs::write(&grub_cfg, content)
                    .with_context(|| format!("Failed to write GRUB configuration: {}", grub_cfg.display()))?;
                
                debug!("GRUB boot configuration updated");
            },
            "any" => {
                // Try to detect the boot loader type
                let isolinux_cfg = self.get_full_path("/isolinux/isolinux.cfg");
                let grub_cfg = self.get_full_path("/boot/grub/grub.cfg");
                
                if isolinux_cfg.exists() {
                    debug!("Detected ISOLINUX boot loader");
                    self.configure_boot("isolinux", parameters)?;
                } else if grub_cfg.exists() {
                    debug!("Detected GRUB boot loader");
                    self.configure_boot("grub", parameters)?;
                } else {
                    return Err(anyhow::anyhow!("Could not detect boot loader type"));
                }
            },
            _ => {
                return Err(anyhow::anyhow!("Unsupported boot target: {}", target));
            }
        }
        
        debug!("Boot configuration updated successfully");
        Ok(())
    }
    
    /// Get the full path within the extraction directory
    fn get_full_path(&self, path: &str) -> PathBuf {
        // Remove leading slash if present
        let clean_path = path.strip_prefix('/').unwrap_or(path);
        self.extraction_dir.join(clean_path)
    }
}

/// Parse a string of hex bytes (e.g., "45 67 AB CD") into a vector of bytes
fn parse_hex_bytes(hex_str: &str) -> Result<Vec<u8>> {
    let hex_str = hex_str.replace(' ', "");
    
    // Ensure we have an even number of hex digits
    if hex_str.len() % 2 != 0 {
        return Err(anyhow::anyhow!("Invalid hex string length: {}", hex_str.len()));
    }
    
    // Parse the hex bytes
    let mut bytes = Vec::with_capacity(hex_str.len() / 2);
    let mut i = 0;
    while i < hex_str.len() {
        let byte_str = &hex_str[i..i+2];
        let byte = u8::from_str_radix(byte_str, 16)
            .with_context(|| format!("Failed to parse hex byte: {}", byte_str))?;
        bytes.push(byte);
        i += 2;
    }
    
    Ok(bytes)
}