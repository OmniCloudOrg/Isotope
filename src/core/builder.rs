use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn, debug};

use crate::automation::{puppet::PuppetManager, vm::{VmManager, VmInstance}};
use crate::config::{IsotopeSpec, StageType};
use crate::iso::{extractor::IsoExtractor, packager::IsoPackager};
use crate::utils::{checksum::ChecksumVerifier, fs::FileSystemManager};

pub struct Builder {
    spec: IsotopeSpec,
    working_dir: PathBuf,
    output_path: Option<PathBuf>,
    vm_manager: Arc<Mutex<VmManager>>,
    puppet_manager: Arc<Mutex<PuppetManager>>,
    iso_extractor: IsoExtractor,
    iso_packager: IsoPackager,
    fs_manager: FileSystemManager,
    checksum_verifier: ChecksumVerifier,
}

impl Builder {
    pub fn new(spec: IsotopeSpec) -> Self {
        let working_dir = std::env::temp_dir().join(format!("isotope-{}", uuid::Uuid::new_v4()));
        
        Self {
            spec,
            working_dir: working_dir.clone(),
            output_path: None,
            vm_manager: Arc::new(Mutex::new(VmManager::new())),
            puppet_manager: Arc::new(Mutex::new(PuppetManager::new())),
            iso_extractor: IsoExtractor::new(),
            iso_packager: IsoPackager::new(),
            fs_manager: FileSystemManager::new(working_dir),
            checksum_verifier: ChecksumVerifier::new(),
        }
    }

    pub fn set_output_path(&mut self, path: PathBuf) {
        self.output_path = Some(path);
    }

    pub async fn build(&self) -> Result<()> {
        info!("Starting ISO build process");
        
        // Create working directory
        self.fs_manager.create_working_directory()
            .context("Failed to create working directory")?;

        // Step 1: Validate and prepare source ISO
        let source_iso_path = self.prepare_source_iso().await?;

        // Step 2: Execute init stage (VM setup)
        self.execute_init_stage().await?;

        // Step 3: Execute os_install stage (automated installation in VM)
        let vm_instance = self.execute_os_install_stage(&source_iso_path).await?;

        // Step 4: Execute os_configure stage (live OS configuration)
        self.execute_os_configure_stage(vm_instance).await?;

        // Step 5: Execute pack stage (create final ISO)
        self.execute_pack_stage().await?;

        // Cleanup
        self.cleanup().await?;

        info!("ISO build completed successfully");
        Ok(())
    }

    pub async fn test(&self) -> Result<()> {
        info!("Starting ISO test process");
        
        // Create working directory
        self.fs_manager.create_working_directory()
            .context("Failed to create working directory")?;

        // Prepare source ISO
        let source_iso_path = self.prepare_source_iso().await?;

        // Execute init stage only
        self.execute_init_stage().await?;

        // Test the VM boot process
        self.test_vm_boot(&source_iso_path).await?;

        // Cleanup
        self.cleanup().await?;

        info!("ISO test completed successfully");
        Ok(())
    }

    async fn prepare_source_iso(&self) -> Result<PathBuf> {
        info!("Preparing source ISO: {}", self.spec.from);
        
        let source_path = Path::new(&self.spec.from);
        if !source_path.exists() {
            return Err(anyhow::anyhow!("Source ISO file does not exist: {}", self.spec.from));
        }

        // Verify checksum if provided
        if let Some(checksum_info) = &self.spec.checksum {
            info!("Verifying checksum...");
            self.checksum_verifier.verify_file(source_path, &checksum_info.algorithm, &checksum_info.value)
                .context("Checksum verification failed")?;
        }

        Ok(source_path.to_path_buf())
    }

    async fn execute_init_stage(&self) -> Result<()> {
        info!("Executing init stage");
        
        if let Some(init_stage) = self.spec.get_stage(&StageType::Init) {
            let mut vm_manager = self.vm_manager.lock().await;
            vm_manager.configure_from_stage(init_stage)
                .context("Failed to configure VM from init stage")?;
        } else {
            warn!("No init stage found, using default VM configuration");
        }

        Ok(())
    }

    async fn execute_os_install_stage(&self, source_iso_path: &Path) -> Result<Option<VmInstance>> {
        info!("Executing os_install stage");
        
        if let Some(os_install_stage) = self.spec.get_stage(&StageType::OsInstall) {
            // Start VM with source ISO
            let mut vm_manager = self.vm_manager.lock().await;
            let vm_instance = vm_manager.create_vm()
                .context("Failed to create VM instance")?;

            vm_manager.attach_iso(&vm_instance, source_iso_path).await
                .context("Failed to attach source ISO to VM")?;

            vm_manager.start_vm(&vm_instance).await
                .context("Failed to start VM")?;

            // Execute puppet automation
            let mut puppet_manager = self.puppet_manager.lock().await;
            puppet_manager.execute_stage_instructions(&vm_instance, os_install_stage, &vm_manager).await
                .context("Failed to execute OS installation instructions")?;

            Ok(Some(vm_instance))
        } else {
            warn!("No os_install stage found, skipping automated installation");
            Ok(None)
        }
    }

    async fn execute_os_configure_stage(&self, vm_instance: Option<VmInstance>) -> Result<()> {
        info!("Executing os_configure stage");
        
        if let Some(os_configure_stage) = self.spec.get_stage(&StageType::OsConfigure) {
            let mut vm_manager = self.vm_manager.lock().await;
            
            let vm_instance = if let Some(existing_instance) = vm_instance {
                info!("Reusing VM instance from os_install stage: {}", existing_instance.name);
                existing_instance
            } else {
                info!("No VM instance from os_install, creating new one");
                let instance = vm_manager.get_or_create_configured_vm()
                    .context("Failed to get configured VM")?;
                
                vm_manager.start_vm(&instance).await
                    .context("Failed to start configured VM")?;

                // Wait for OS boot
                vm_manager.wait_for_boot(&instance).await
                    .context("Failed to wait for OS boot")?;
                
                instance
            };

            // Execute configuration instructions
            let mut puppet_manager = self.puppet_manager.lock().await;
            puppet_manager.execute_stage_instructions(&vm_instance, os_configure_stage, &vm_manager).await
                .context("Failed to execute OS configuration instructions")?;

            // Create live OS snapshot
            vm_manager.create_live_snapshot(&vm_instance).await
                .context("Failed to create live OS snapshot")?;

            vm_manager.shutdown_vm(&vm_instance).await
                .context("Failed to shutdown VM after configuration")?;

        } else {
            warn!("No os_configure stage found, skipping OS configuration");
        }

        Ok(())
    }

    async fn execute_pack_stage(&self) -> Result<()> {
        info!("Executing pack stage");
        
        if let Some(pack_stage) = self.spec.get_stage(&StageType::Pack) {
            // Extract the configured VM disk/snapshot into ISO format
            let vm_manager = self.vm_manager.lock().await;
            let live_snapshot_path = vm_manager.get_live_snapshot_path()
                .context("No live snapshot available for packaging")?;

            // Convert snapshot to bootable ISO
            let output_path = self.get_final_output_path(pack_stage)?;
            self.iso_packager.create_live_iso(&live_snapshot_path, &output_path, pack_stage)
                .context("Failed to create final ISO")?;

            info!("ISO created successfully: {}", output_path.display());
        } else {
            return Err(anyhow::anyhow!("pack stage is required but not found"));
        }

        Ok(())
    }

    async fn test_vm_boot(&self, source_iso_path: &Path) -> Result<()> {
        info!("Testing VM boot with source ISO");
        
        let mut vm_manager = self.vm_manager.lock().await;
        let vm_instance = vm_manager.create_vm()
            .context("Failed to create test VM")?;

        vm_manager.attach_iso(&vm_instance, source_iso_path).await
            .context("Failed to attach ISO to test VM")?;

        vm_manager.start_vm(&vm_instance).await
            .context("Failed to start test VM")?;

        // Wait for successful boot (configurable timeout)
        vm_manager.wait_for_boot_test(&vm_instance).await
            .context("VM boot test failed")?;

        vm_manager.shutdown_vm(&vm_instance).await
            .context("Failed to shutdown test VM")?;

        info!("VM boot test completed successfully");
        Ok(())
    }

    fn get_final_output_path(&self, pack_stage: &crate::config::Stage) -> Result<PathBuf> {
        // Check if output path was provided via CLI
        if let Some(path) = &self.output_path {
            return Ok(path.clone());
        }

        // Look for EXPORT instruction in pack stage
        for instruction in &pack_stage.instructions {
            if let crate::config::Instruction::Export { path } = instruction {
                return Ok(path.clone());
            }
        }

        // Fall back to default based on spec name
        let default_name = self.spec.get_label("name")
            .map(|s| format!("{}.iso", s))
            .unwrap_or_else(|| "output.iso".to_string());
        
        Ok(PathBuf::from(default_name))
    }

    async fn cleanup(&self) -> Result<()> {
        info!("Cleaning up working directory");
        
        // Stop and cleanup VMs
        let mut vm_manager = self.vm_manager.lock().await;
        vm_manager.cleanup_all().await
            .context("Failed to cleanup VMs")?;

        // Remove working directory
        self.fs_manager.cleanup()
            .context("Failed to cleanup working directory")?;

        Ok(())
    }
}