use anyhow::{Context, Result};
use log::{debug, info, warn};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

use crate::config::schema::{Config, Modification};
use crate::core::modifier::IsoModifier;
use crate::iso::{extract, package};
use crate::utils::checksum::verify_checksum;
use crate::utils::fs::copy_directory;
use crate::utils::template::process_templates;

/// Responsible for the ISO building process
pub struct IsoBuilder {
    config: Config,
    work_dir: Option<PathBuf>,
    temp_dir: Option<TempDir>,
}

impl IsoBuilder {
    /// Create a new ISO builder with the given configuration
    pub fn new(config: Config) -> Self {
        Self {
            config,
            work_dir: None,
            temp_dir: None,
        }
    }
    
    /// Execute the build process
    pub fn build(&mut self) -> Result<()> {
        info!("Starting ISO build process");
        
        // Initialize working directory
        self.setup_working_directory()
            .context("Failed to set up working directory")?;
        
        // Run pre-extraction hooks
        self.run_hooks("pre_extraction")
            .context("Failed to run pre-extraction hooks")?;
        
        // Extract the source ISO
        let extraction_dir = self.extract_source_iso()
            .context("Failed to extract source ISO")?;
        
        // Run post-extraction hooks
        self.run_hooks("post_extraction")
            .context("Failed to run post-extraction hooks")?;
        
        // Run pre-modification hooks
        self.run_hooks("pre_modification")
            .context("Failed to run pre-modification hooks")?;
        
        // Apply modifications
        self.apply_modifications(&extraction_dir)
            .context("Failed to apply modifications")?;
        
        // Run post-modification hooks
        self.run_hooks("post_modification")
            .context("Failed to run post-modification hooks")?;
        
        // Run pre-packaging hooks
        self.run_hooks("pre_packaging")
            .context("Failed to run pre-packaging hooks")?;
        
        // Package the modified ISO
        self.package_iso(&extraction_dir)
            .context("Failed to package ISO")?;
        
        // Run post-packaging hooks
        self.run_hooks("post_packaging")
            .context("Failed to run post-packaging hooks")?;
        
        // Clean up temporary files
        self.cleanup()
            .context("Failed to clean up temporary files")?;
        
        info!("ISO build completed successfully");
        Ok(())
    }
    
    /// Set up the working directory
    fn setup_working_directory(&mut self) -> Result<()> {
        if let Some(work_dir) = &self.config.build.working_dir {
            debug!("Using specified working directory: {}", work_dir.display());
            std::fs::create_dir_all(work_dir)
                .context("Failed to create working directory")?;
            self.work_dir = Some(work_dir.clone());
        } else {
            debug!("Creating temporary working directory");
            let temp_dir = TempDir::new()
                .context("Failed to create temporary directory")?;
            debug!("Temporary working directory: {}", temp_dir.path().display());
            self.work_dir = Some(temp_dir.path().to_path_buf());
            self.temp_dir = Some(temp_dir);
        }
        
        Ok(())
    }
    
    /// Extract the source ISO
    fn extract_source_iso(&self) -> Result<PathBuf> {
        let work_dir = self.work_dir.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Working directory not initialized"))?;
        
        let extraction_dir = work_dir.join("extracted");
        std::fs::create_dir_all(&extraction_dir)
            .context("Failed to create extraction directory")?;
        
        info!("Extracting source ISO: {}", self.config.source.path.display());
        
        if !self.config.source.path.exists() {
            return Err(anyhow::anyhow!("Source ISO file does not exist: {}", 
                self.config.source.path.display()));
        }

        // Verify the source ISO checksum
        if let Some(checksum) = &self.config.source.checksum {
            verify_checksum(&self.config.source.path, &checksum.checksum_type, &checksum.value)
                .context("Failed to verify source ISO checksum")?;
        } else {
            warn!("No checksum specified for source ISO - skipping verification");
        }
        
        // Extract the ISO
        extract::extract_iso(&self.config.source.path, &extraction_dir)
            .context("Failed to extract ISO")?;
        
        info!("Source ISO extracted to: {}", extraction_dir.display());
        Ok(extraction_dir)
    }
    
    /// Apply modifications to the extracted ISO
    fn apply_modifications(&self, extraction_dir: &Path) -> Result<()> {
        info!("Applying {} modifications to ISO", self.config.modifications.len());
        
        let modifier = IsoModifier::new(extraction_dir);
        
        for (i, modification) in self.config.modifications.iter().enumerate() {
            debug!("Applying modification {}/{}: {:?}", i + 1, self.config.modifications.len(), modification);
            
            match modification {
                Modification::FileAdd { source, destination, attributes } => {
                    modifier.add_file(source, destination, attributes.as_ref())
                        .context(format!("Failed to add file: {} -> {}", source.display(), destination))?;
                }
                Modification::FileModify { path, operations } => {
                    modifier.modify_file(path, operations)
                        .context(format!("Failed to modify file: {}", path))?;
                }
                Modification::FileRemove { path } => {
                    modifier.remove_file(path)
                        .context(format!("Failed to remove file: {}", path))?;
                }
                Modification::DirectoryAdd { source, destination } => {
                    modifier.add_directory(source, destination)
                        .context(format!("Failed to add directory: {} -> {}", source.display(), destination))?;
                }
                Modification::AnswerFile { template, destination, variables } => {
                    modifier.add_answer_file(template, destination, variables)
                        .context(format!("Failed to add answer file: {} -> {}", template.display(), destination))?;
                }
                Modification::BinaryPatch { path, patches } => {
                    modifier.apply_binary_patches(path, patches)
                        .context(format!("Failed to apply binary patches to: {}", path))?;
                }
                Modification::BootConfig { target, parameters } => {
                    modifier.configure_boot(target, parameters)
                        .context(format!("Failed to configure boot for target: {}", target))?;
                }
            }
        }
        
        info!("All modifications applied successfully");
        Ok(())
    }
    
    /// Package the modified ISO
    fn package_iso(&self, extraction_dir: &Path) -> Result<()> {
        let output_path = &self.config.output.path;
        
        info!("Packaging ISO to: {}", output_path.display());
        
        // Create parent directory if it doesn't exist
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create output directory")?;
        }
        
        // Package the ISO
        package::create_iso(
            extraction_dir,
            output_path,
            &self.config.output.format,
            self.config.output.options.as_ref()
        ).context("Failed to create ISO")?;
        
        info!("ISO packaged successfully");
        Ok(())
    }
    
    /// Run hook scripts
    fn run_hooks(&self, hook_type: &str) -> Result<()> {
        if let Some(hooks) = &self.config.hooks {
            let scripts = match hook_type {
                "pre_extraction" => &hooks.pre_extraction,
                "post_extraction" => &hooks.post_extraction,
                "pre_modification" => &hooks.pre_modification,
                "post_modification" => &hooks.post_modification,
                "pre_packaging" => &hooks.pre_packaging,
                "post_packaging" => &hooks.post_packaging,
                _ => return Err(anyhow::anyhow!("Unknown hook type: {}", hook_type)),
            };
            
            if !scripts.is_empty() {
                info!("Running {} {} hooks", scripts.len(), hook_type);
                
                for (i, script) in scripts.iter().enumerate() {
                    debug!("Running hook {}/{}: {}", i + 1, scripts.len(), script);
                    
                    // Execute the script
                    let status = Command::new(script)
                        .current_dir(self.work_dir.as_ref().unwrap())
                        .status()
                        .with_context(|| format!("Failed to execute hook script: {}", script))?;
                    
                    if !status.success() {
                        return Err(anyhow::anyhow!("Hook script failed: {}", script));
                    }
                }
                
                info!("All {} hooks completed successfully", hook_type);
            }
        }
        
        Ok(())
    }
    
    /// Clean up temporary files
    fn cleanup(&mut self) -> Result<()> {
        if self.config.build.cleanup {
            info!("Cleaning up temporary files");
            
            // The TempDir will be automatically deleted when it goes out of scope
            if self.temp_dir.take().is_some() {
                debug!("Temporary directory removed");
            }
        } else {
            info!("Skipping cleanup as requested");
            
            // If we're not cleaning up, we need to keep the TempDir from being deleted
            if let Some(temp_dir) = self.temp_dir.take() {
                let path = temp_dir.into_path();
                debug!("Temporary directory retained at: {}", path.display());
            }
        }
        
        Ok(())
    }
}