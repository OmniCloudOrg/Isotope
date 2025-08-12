use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::automation::{
    puppet::PuppetManager,
    vm::{VmInstance, VmManager},
};
use crate::config::{IsotopeSpec, StageType};
use crate::iso::{extractor::IsoExtractor, packager::IsoPackager};
use crate::utils::{checksum::ChecksumVerifier, fs::FileSystemManager, VmMetadata};

pub struct Builder {
    spec: IsotopeSpec,
    spec_file_path: Option<PathBuf>,
    working_dir: PathBuf,
    output_path: Option<PathBuf>,
    continue_from_step: Option<usize>,
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
            spec_file_path: None,
            working_dir: working_dir.clone(),
            output_path: None,
            continue_from_step: None,
            vm_manager: Arc::new(Mutex::new(VmManager::new())),
            puppet_manager: Arc::new(Mutex::new(PuppetManager::new())),
            iso_extractor: IsoExtractor::new(),
            iso_packager: IsoPackager::new(),
            fs_manager: FileSystemManager::new(working_dir),
            checksum_verifier: ChecksumVerifier::new(),
        }
    }

    pub fn new_with_ocr_debug(spec: IsotopeSpec, ocr_debug: bool) -> Self {
        let working_dir = std::env::temp_dir().join(format!("isotope-{}", uuid::Uuid::new_v4()));

        Self {
            spec,
            spec_file_path: None,
            working_dir: working_dir.clone(),
            output_path: None,
            continue_from_step: None,
            vm_manager: Arc::new(Mutex::new(VmManager::new())),
            puppet_manager: Arc::new(Mutex::new(PuppetManager::new_with_ocr_debug(ocr_debug))),
            iso_extractor: IsoExtractor::new(),
            iso_packager: IsoPackager::new(),
            fs_manager: FileSystemManager::new(working_dir),
            checksum_verifier: ChecksumVerifier::new(),
        }
    }

    pub fn set_output_path(&mut self, path: PathBuf) {
        self.output_path = Some(path);
    }

    pub fn set_continue_from_step(&mut self, step: usize) {
        self.continue_from_step = Some(step);
    }

    pub fn set_spec_file_path(&mut self, path: PathBuf) {
        self.spec_file_path = Some(path);
    }

    fn get_stage_step_mapping(&self, target_step: usize) -> Result<(StageType, usize)> {
        let mut current_step = 1;

        // Check os_install stage
        if let Some(os_install_stage) = self.spec.get_stage(&StageType::OsInstall) {
            let stage_end = current_step + os_install_stage.instructions.len() - 1;
            if target_step >= current_step && target_step <= stage_end {
                let step_in_stage = target_step - current_step + 1;
                return Ok((StageType::OsInstall, step_in_stage));
            }
            current_step += os_install_stage.instructions.len();
        }

        // Check os_configure stage
        if let Some(os_configure_stage) = self.spec.get_stage(&StageType::OsConfigure) {
            let stage_end = current_step + os_configure_stage.instructions.len() - 1;
            if target_step >= current_step && target_step <= stage_end {
                let step_in_stage = target_step - current_step + 1;
                return Ok((StageType::OsConfigure, step_in_stage));
            }
            current_step += os_configure_stage.instructions.len();
        }

        Err(anyhow!(
            "Step {} is out of range. Total steps available: {}",
            target_step,
            current_step - 1
        ))
    }

    fn print_step_summary(&self) {
        let mut current_step = 1;

        info!("Step summary:");

        if let Some(os_install_stage) = self.spec.get_stage(&StageType::OsInstall) {
            info!(
                "  Steps {}-{}: os_install stage ({} instructions)",
                current_step,
                current_step + os_install_stage.instructions.len() - 1,
                os_install_stage.instructions.len()
            );
            current_step += os_install_stage.instructions.len();
        }

        if let Some(os_configure_stage) = self.spec.get_stage(&StageType::OsConfigure) {
            info!(
                "  Steps {}-{}: os_configure stage ({} instructions)",
                current_step,
                current_step + os_configure_stage.instructions.len() - 1,
                os_configure_stage.instructions.len()
            );
            current_step += os_configure_stage.instructions.len();
        }

        info!("Total steps: {}", current_step - 1);
    }

    fn get_existing_vm_from_metadata(&self) -> Result<Option<VmInstance>> {
        let Some(spec_file_path) = &self.spec_file_path else {
            return Ok(None);
        };

        let metadata = VmMetadata::load_from_current_dir()?;

        if let Some(vm_entry) = metadata.get_vm_for_isotope_file(spec_file_path) {
            info!(
                "Found existing VM {} for this isotope file",
                vm_entry.vm_name
            );

            // Create a VmInstance from the metadata - we'll assume it exists for now
            // The actual VM status check will happen when we try to use it
            let config = crate::automation::vm::VmConfig::default();
            
            let vm_instance = VmInstance::new(
                vm_entry.vm_id.clone(),
                vm_entry.vm_name.clone(),
                vm_entry
                    .provider
                    .parse()
                    .map_err(|_| anyhow!("Invalid provider: {}", vm_entry.provider))?,
                config,
            );

            info!("Will attempt to reuse existing VM {}", vm_entry.vm_name);
            return Ok(Some(vm_instance));
        }

        Ok(None)
    }

    fn save_vm_metadata(&self, vm_instance: &VmInstance) -> Result<()> {
        let Some(spec_file_path) = &self.spec_file_path else {
            return Ok(()); // No spec file path, can't save metadata
        };

        let mut metadata = VmMetadata::load_from_current_dir().unwrap_or_default();

        metadata.cleanup_stale_entries();
        metadata.add_or_update_vm(spec_file_path, vm_instance)?;
        metadata.save_to_current_dir()?;

        Ok(())
    }

    async fn ensure_vm_running(
        &self,
        vm_manager: &mut VmManager,
        vm_instance: &VmInstance,
    ) -> Result<()> {
        let is_running = vm_manager
            .get_provider(&vm_instance.provider)?
            .is_running(vm_instance)
            .await
            .unwrap_or(false);

        if is_running {
            info!("VM {} is already running", vm_instance.name);
        } else {
            info!("Starting VM {}", vm_instance.name);
            vm_manager
                .start_vm(vm_instance)
                .await
                .context("Failed to start VM")?;

            // Wait for OS boot
            vm_manager
                .wait_for_boot(vm_instance)
                .await
                .context("Failed to wait for OS boot")?;
        }

        Ok(())
    }


    pub async fn build(&self) -> Result<()> {
        info!("Starting ISO build process");

        // Show step summary for user reference
        self.print_step_summary();

        if let Some(step) = self.continue_from_step {
            let (stage, step_in_stage) = self.get_stage_step_mapping(step)?;
            info!(
                "Continuing from step {} (stage: {:?}, step {} within stage)",
                step, stage, step_in_stage
            );
        }

        // Create working directory
        self.fs_manager
            .create_working_directory()
            .context("Failed to create working directory")?;

        // Step 1: Validate and prepare source ISO
        let source_iso_path = self.prepare_source_iso().await?;

        // Step 2: Execute init stage (VM setup)
        self.execute_init_stage().await?;

        // Step 3: Execute os_install stage (automated installation in VM)
        let vm_instance = self.execute_os_install_stage(&source_iso_path).await?;

        // Step 4: Execute os_configure stage (live OS configuration)
        let final_vm_instance = self.execute_os_configure_stage(vm_instance).await?;

        // Step 5: Execute pack stage (create final ISO)
        self.execute_pack_stage(final_vm_instance).await?;

        // Cleanup
        self.cleanup().await?;

        info!("ISO build completed successfully");
        Ok(())
    }

    pub async fn test(&self) -> Result<()> {
        info!("Starting ISO test process");

        // Create working directory
        self.fs_manager
            .create_working_directory()
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
            return Err(anyhow::anyhow!(
                "Source ISO file does not exist: {}",
                self.spec.from
            ));
        }

        // Verify checksum if provided
        if let Some(checksum_info) = &self.spec.checksum {
            info!("Verifying checksum...");
            self.checksum_verifier
                .verify_file(source_path, &checksum_info.algorithm, &checksum_info.value)
                .context("Checksum verification failed")?;
        }

        Ok(source_path.to_path_buf())
    }

    async fn execute_init_stage(&self) -> Result<()> {
        info!("Executing init stage");

        if let Some(init_stage) = self.spec.get_stage(&StageType::Init) {
            let mut vm_manager = self.vm_manager.lock().await;
            vm_manager
                .configure_from_stage(init_stage)
                .context("Failed to configure VM from init stage")?;
        } else {
            warn!("No init stage found, using default VM configuration");
        }

        Ok(())
    }

    async fn execute_os_install_stage(&self, source_iso_path: &Path) -> Result<Option<VmInstance>> {
        info!("Executing os_install stage");

        if let Some(os_install_stage) = self.spec.get_stage(&StageType::OsInstall) {
            let mut vm_manager = self.vm_manager.lock().await;

            // Check if we should reuse an existing VM (only when using --continue-from)
            let vm_instance = if self.continue_from_step.is_some() {
                if let Some(existing_vm) = self.get_existing_vm_from_metadata()? {
                    info!("Reusing existing VM {} for --continue-from", existing_vm.name);
                    existing_vm
                } else {
                    return Err(anyhow!(
                        "Cannot continue: no existing VM found in metadata. Run without --continue-from first."
                    ));
                }
            } else {
                info!("Creating new VM (not continuing from previous build)");
                vm_manager
                    .create_vm()
                    .context("Failed to create VM instance")?
            };

            // Check if VM is already running when continuing
            let is_already_running = if self.continue_from_step.is_some() {
                vm_manager
                    .get_provider(&vm_instance.provider)?
                    .is_running(&vm_instance)
                    .await
                    .unwrap_or(false)
            } else {
                false
            };

            if is_already_running {
                info!(
                    "VM {} is already running, skipping start and ISO attachment",
                    vm_instance.name
                );
            } else {
                info!("Starting VM {} and attaching ISO", vm_instance.name);

                vm_manager
                    .attach_iso(&vm_instance, source_iso_path)
                    .await
                    .context("Failed to attach source ISO to VM")?;

                vm_manager
                    .start_vm(&vm_instance)
                    .await
                    .context("Failed to start VM")?;
            }

            // Get the updated VM instance (SSH port may have been updated during attach_iso)
            let updated_vm_instance = vm_manager
                .get_instance(&vm_instance.id)
                .ok_or_else(|| anyhow!("VM instance not found after setup"))?
                .clone();

            // Execute puppet automation
            let mut puppet_manager = self.puppet_manager.lock().await;

            // Check if we need to continue from a specific step in this stage
            let continue_from = if let Some(target_step) = self.continue_from_step {
                match self.get_stage_step_mapping(target_step)? {
                    (StageType::OsInstall, step_in_stage) => {
                        info!("Continuing os_install stage from step {}", step_in_stage);
                        Some(step_in_stage)
                    }
                    (other_stage, _) => {
                        info!(
                            "Target step {} is in {:?} stage, skipping os_install",
                            target_step, other_stage
                        );
                        return Ok(Some(updated_vm_instance)); // Skip this stage entirely
                    }
                }
            } else {
                None
            };

            puppet_manager
                .execute_stage_instructions_from_step(
                    &updated_vm_instance,
                    os_install_stage,
                    &vm_manager,
                    continue_from,
                )
                .await
                .context("Failed to execute OS installation instructions")?;

            // Save VM metadata for future --continue runs
            self.save_vm_metadata(&updated_vm_instance)?;

            Ok(Some(updated_vm_instance))
        } else {
            warn!("No os_install stage found, skipping automated installation");
            Ok(None)
        }
    }

    async fn execute_os_configure_stage(&self, vm_instance: Option<VmInstance>) -> Result<Option<VmInstance>> {
        info!("Executing os_configure stage");

        if let Some(os_configure_stage) = self.spec.get_stage(&StageType::OsConfigure) {
            let mut vm_manager = self.vm_manager.lock().await;

            let vm_instance = if let Some(existing_instance) = vm_instance {
                info!(
                    "Reusing VM instance from os_install stage: {}",
                    existing_instance.name
                );
                existing_instance
            } else {
                // Check if we can reuse an existing VM when continuing directly to os_configure
                if let Some(target_step) = self.continue_from_step {
                    if let Ok((StageType::OsConfigure, _)) =
                        self.get_stage_step_mapping(target_step)
                    {
                        if let Some(existing_vm) = self.get_existing_vm_from_metadata()? {
                            info!(
                                "Reusing existing VM {} for --continue in os_configure stage",
                                existing_vm.name
                            );
                            // Ensure the existing VM is running
                            self.ensure_vm_running(&mut vm_manager, &existing_vm)
                                .await?;
                            existing_vm
                        } else {
                            info!("No existing VM found for --continue, creating new one");
                            let instance = vm_manager
                                .get_or_create_configured_vm()
                                .context("Failed to get configured VM")?;

                            self.ensure_vm_running(&mut vm_manager, &instance).await?;
                            instance
                        }
                    } else {
                        info!("No VM instance from os_install, creating new one");
                        let instance = vm_manager
                            .get_or_create_configured_vm()
                            .context("Failed to get configured VM")?;

                        self.ensure_vm_running(&mut *vm_manager, &instance).await?;
                        instance
                    }
                } else {
                    info!("No VM instance from os_install, creating new one");
                    let instance = vm_manager
                        .get_or_create_configured_vm()
                        .context("Failed to get configured VM")?;

                    self.ensure_vm_running(&mut *vm_manager, &instance).await?;
                    instance
                }
            };

            // Execute configuration instructions
            let mut puppet_manager = self.puppet_manager.lock().await;

            // Check if we need to continue from a specific step in this stage
            let continue_from = if let Some(target_step) = self.continue_from_step {
                match self.get_stage_step_mapping(target_step)? {
                    (StageType::OsConfigure, step_in_stage) => {
                        info!("Continuing os_configure stage from step {}", step_in_stage);
                        Some(step_in_stage)
                    }
                    (StageType::OsInstall, _) => {
                        // If target step is in os_install, we should execute os_configure normally
                        None
                    }
                    (other_stage, _) => {
                        info!(
                            "Target step {} is in {:?} stage, skipping os_configure",
                            target_step, other_stage
                        );
                        return Ok(None); // Skip this stage entirely
                    }
                }
            } else {
                None
            };

            puppet_manager
                .execute_stage_instructions_from_step(
                    &vm_instance,
                    os_configure_stage,
                    &vm_manager,
                    continue_from,
                )
                .await
                .context("Failed to execute OS configuration instructions")?;

            // Create live OS snapshot
            vm_manager
                .create_live_snapshot(&vm_instance)
                .await
                .context("Failed to create live OS snapshot")?;

            vm_manager
                .shutdown_vm(&vm_instance)
                .await
                .context("Failed to shutdown VM after configuration")?;

            Ok(Some(vm_instance))
        } else {
            warn!("No os_configure stage found, skipping OS configuration");
            Ok(vm_instance)
        }
    }

    async fn execute_pack_stage(&self, vm_instance: Option<VmInstance>) -> Result<()> {
        info!("Executing pack stage");

        if let Some(pack_stage) = self.spec.get_stage(&StageType::Pack) {
            let vm_manager = self.vm_manager.lock().await;
            
            // Try to get VM disk path or fallback to snapshot
            let disk_path = if let Some(ref instance) = vm_instance {
                match vm_manager.get_vm_disk_path(instance) {
                    Ok(disk_path) => {
                        info!("Using VM disk for packaging: {}", disk_path.display());
                        disk_path
                    }
                    Err(e) => {
                        warn!("Failed to get VM disk path: {}, trying snapshot", e);
                        vm_manager
                            .get_live_snapshot_path()
                            .context("No VM disk or live snapshot available for packaging")?
                    }
                }
            } else {
                info!("No VM instance provided, trying to use live snapshot");
                vm_manager
                    .get_live_snapshot_path()
                    .context("No VM disk or live snapshot available for packaging")?
            };

            // Convert VDI disk to bootable IMG
            let output_path = self.get_final_output_path(pack_stage)?;
            self.iso_packager
                .create_bootable_image(&disk_path, &output_path, pack_stage)
                .context("Failed to create bootable IMG")?;

            info!("Bootable IMG created successfully: {}", output_path.display());
        } else {
            return Err(anyhow::anyhow!("pack stage is required but not found"));
        }

        Ok(())
    }

    async fn test_vm_boot(&self, source_iso_path: &Path) -> Result<()> {
        info!("Testing VM boot with source ISO");

        let mut vm_manager = self.vm_manager.lock().await;
        let vm_instance = vm_manager.create_vm().context("Failed to create test VM")?;

        vm_manager
            .attach_iso(&vm_instance, source_iso_path)
            .await
            .context("Failed to attach ISO to test VM")?;

        vm_manager
            .start_vm(&vm_instance)
            .await
            .context("Failed to start test VM")?;

        // Wait for successful boot (configurable timeout)
        vm_manager
            .wait_for_boot_test(&vm_instance)
            .await
            .context("VM boot test failed")?;

        vm_manager
            .shutdown_vm(&vm_instance)
            .await
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
        let default_name = self
            .spec
            .get_label("name")
            .map(|s| format!("{}.iso", s))
            .unwrap_or_else(|| "output.iso".to_string());

        Ok(PathBuf::from(default_name))
    }

    async fn cleanup(&self) -> Result<()> {
        info!("Cleaning up working directory");

        // Stop and cleanup VMs
        let mut vm_manager = self.vm_manager.lock().await;
        vm_manager
            .cleanup_all()
            .await
            .context("Failed to cleanup VMs")?;

        // Remove working directory
        self.fs_manager
            .cleanup()
            .context("Failed to cleanup working directory")?;

        Ok(())
    }
}
