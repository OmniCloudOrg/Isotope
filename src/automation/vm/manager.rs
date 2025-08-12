use crate::utils::vm_metadata::VmMetadata;
use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{info, warn};
use uuid::Uuid;

use super::providers::{create_provider, VmProviderTrait};
use super::{NetworkConfig, VmConfig, VmInstance, VmProvider, VmState};
use crate::config::{Instruction, Stage};

pub struct VmManager {
    instances: HashMap<String, VmInstance>,
    providers: HashMap<String, Box<dyn VmProviderTrait>>,
    working_dir: PathBuf,
    default_config: VmConfig,
    configured_provider: VmProvider,
}

impl VmManager {
    pub fn new() -> Self {
        Self {
            instances: HashMap::new(),
            providers: HashMap::new(),
            working_dir: std::env::temp_dir().join("isotope-vms"),
            default_config: VmConfig::default(),
            configured_provider: VmProvider::VirtualBox, // Only VirtualBox is supported
        }
    }

    pub fn configure_from_stage(&mut self, stage: &Stage) -> Result<()> {
        info!("Configuring VM from init stage");

        let mut provider = VmProvider::VirtualBox; // Only VirtualBox is supported
        let mut memory_mb = 2048;
        let mut cpus = 2;
        let mut disk_size_gb = 20;
        let mut boot_wait = Duration::from_secs(10);
        let mut timeout = Duration::from_secs(1800);
        let mut additional_args = Vec::new();

        for instruction in &stage.instructions {
            if let Instruction::Vm { key, value } = instruction {
                match key.as_str() {
                    "provider" => {
                        // Only VirtualBox is supported now
                        if value != "virtualbox" {
                            return Err(anyhow!(
                                "Unsupported VM provider: {}. Only VirtualBox is supported.",
                                value
                            ));
                        }
                        provider = VmProvider::VirtualBox;
                    }
                    "memory" => {
                        memory_mb = self.parse_memory_size(value)?;
                    }
                    "cpus" => {
                        cpus = value
                            .parse()
                            .with_context(|| format!("Invalid CPU count: {}", value))?;
                    }
                    "disk" => {
                        disk_size_gb = self.parse_disk_size(value)?;
                    }
                    "boot-wait" => {
                        boot_wait = self.parse_duration(value)?;
                    }
                    "timeout" => {
                        timeout = self.parse_duration(value)?;
                    }
                    _ => {
                        additional_args.push(format!("--{}", key));
                        additional_args.push(value.clone());
                    }
                }
            }
        }

        self.default_config = VmConfig {
            memory_mb,
            cpus,
            disk_size_gb,
            boot_wait,
            timeout,
            additional_args,
            network_config: NetworkConfig::default(),
        };

        info!(
            "VM configured: VirtualBox with {}MB RAM, {} CPUs",
            memory_mb, cpus
        );
        self.configured_provider = provider;
        Ok(())
    }

    pub fn create_vm(&mut self) -> Result<VmInstance> {
        let vm_id = Uuid::new_v4().to_string();
        let vm_name = format!("isotope-vm-{}", &vm_id[..8]);
        let instance = VmInstance::new(
            vm_id.clone(),
            vm_name,
            self.configured_provider,
            self.default_config.clone(),
        );
        self.instances.insert(vm_id.clone(), instance.clone());
        
        // Clean up old VM metadata and VMs, then save new VM to .isometa
        if let Some(isotope_path) = std::env::args().find(|a| a.ends_with(".isotope")) {
            if let Ok(mut meta) = VmMetadata::load_from_current_dir() {
                // Get old VM info before removing from metadata
                if let Some(old_vm_entry) = meta.get_vm_for_isotope_file(std::path::Path::new(&isotope_path)) {
                    info!("Found old VM {} from previous build, will clean up metadata", old_vm_entry.vm_name);
                    // Note: We'll let VirtualBox handle the actual VM cleanup later
                    // For now, just log that we're replacing the old VM entry
                    warn!("Old VM {} will be replaced with new VM for fresh build", old_vm_entry.vm_name);
                }
                
                // Remove old VM from metadata and add new one
                let _ = meta.remove_vm(std::path::Path::new(&isotope_path));
                let _ = meta.add_or_update_vm(std::path::Path::new(&isotope_path), &instance);
                let _ = meta.save_to_current_dir();
                info!("Cleaned up old VM metadata and registered new VM: {}", instance.name);
            }
        }
        info!("Created VM instance: {}", instance.name);
        Ok(instance)
    }

    pub async fn attach_iso(&mut self, instance: &VmInstance, iso_path: &Path) -> Result<()> {
        info!(
            "Attaching ISO {} to VM {}",
            iso_path.display(),
            instance.name
        );

        if !iso_path.exists() {
            return Err(anyhow!("ISO file does not exist: {}", iso_path.display()));
        }

        let provider = self.get_provider(&instance.provider)?;

        let mut updated_instance = instance.clone();
        provider.attach_iso(&mut updated_instance, iso_path).await?;

        self.instances.insert(instance.id.clone(), updated_instance);
        Ok(())
    }

    pub async fn start_vm(&mut self, instance: &VmInstance) -> Result<()> {
        info!("Starting VM: {}", instance.name);

        let provider = self.get_provider(&instance.provider)?;

        let mut updated_instance = instance.clone();

        // Start the VM (it should already be created)
        provider
            .start_vm(&mut updated_instance)
            .await
            .context("Failed to start VM")?;

        self.instances.insert(instance.id.clone(), updated_instance);
        Ok(())
    }

    pub async fn wait_for_boot(&self, instance: &VmInstance) -> Result<()> {
        info!("Waiting for VM {} to boot", instance.name);

        let provider = self.get_provider(&instance.provider)?;

        // Wait for the boot-wait period first
        tokio::time::sleep(instance.config.boot_wait).await;

        // Check if VM is still running
        if !provider.is_running(instance).await? {
            return Err(anyhow!("VM {} stopped during boot", instance.name));
        }

        info!("VM {} boot completed", instance.name);
        Ok(())
    }

    pub async fn wait_for_boot_test(&self, instance: &VmInstance) -> Result<()> {
        info!("Testing VM boot for instance: {}", instance.name);

        let provider = self.get_provider(&instance.provider)?;

        // Wait for the boot-wait period
        tokio::time::sleep(instance.config.boot_wait).await;

        if provider.is_running(instance).await? {
            info!("VM boot test successful for: {}", instance.name);
            Ok(())
        } else {
            Err(anyhow!(
                "VM {} is not running after boot wait",
                instance.name
            ))
        }
    }

    pub async fn wait_for_shutdown(&self, instance: &VmInstance) -> Result<()> {
        info!("Waiting for VM {} to shutdown", instance.name);

        let provider = self.get_provider(&instance.provider)?;
        provider.wait_for_shutdown(instance).await
    }

    pub async fn shutdown_vm(&mut self, instance: &VmInstance) -> Result<()> {
        info!("Shutting down VM: {}", instance.name);

        let provider = self.get_provider(&instance.provider)?;

        let mut updated_instance = instance.clone();
        provider
            .stop_vm(&mut updated_instance)
            .await
            .context("Failed to stop VM")?;

        self.instances.insert(instance.id.clone(), updated_instance);
        Ok(())
    }

    pub async fn create_live_snapshot(&self, instance: &VmInstance) -> Result<()> {
        info!("Creating live snapshot for VM: {}", instance.name);

        let provider = self.get_provider(&instance.provider)?;
        provider
            .create_snapshot(instance, "live-snapshot")
            .await
            .context("Failed to create live snapshot")?;

        Ok(())
    }

    pub fn get_live_snapshot_path(&self) -> Result<PathBuf> {
        // Return path to the live snapshot that can be converted to ISO
        let snapshot_path = self.working_dir.join("live-snapshot.qcow2");

        if snapshot_path.exists() {
            Ok(snapshot_path)
        } else {
            Err(anyhow!("No live snapshot found"))
        }
    }

    pub fn get_vm_disk_path(&self, instance: &VmInstance) -> Result<PathBuf> {
        match instance.provider {
            crate::automation::vm::VmProvider::VirtualBox => {
                // VirtualBox creates disk files in current directory with VM name
                let disk_path = PathBuf::from(format!("{}.vdi", instance.name));
                if disk_path.exists() {
                    Ok(disk_path)
                } else {
                    Err(anyhow!("VM disk not found: {}", disk_path.display()))
                }
            }
        }
    }

    pub fn get_or_create_configured_vm(&mut self) -> Result<VmInstance> {
        // Try to find an existing VM instance with the same configuration
        for instance in self.instances.values() {
            if instance.provider == self.configured_provider
                && instance.config.memory_mb == self.default_config.memory_mb
                && instance.config.cpus == self.default_config.cpus
            {
                info!("Reusing existing VM instance: {}", instance.name);
                return Ok(instance.clone());
            }
        }

        // If no existing VM found, create a new one
        info!("No compatible existing VM found, creating new instance");
        self.create_vm()
    }

    pub fn get_instance(&self, instance_id: &str) -> Option<&VmInstance> {
        self.instances.get(instance_id)
    }

    pub async fn cleanup_all(&mut self) -> Result<()> {
        info!("Cleaning up all VM instances");

        let instance_ids: Vec<String> = self.instances.keys().cloned().collect();

        for instance_id in instance_ids {
            if let Some(instance) = self.instances.get(&instance_id) {
                let provider = self.get_provider(&instance.provider)?;

                if instance.is_running() {
                    if let Err(e) = provider.stop_vm(&mut instance.clone()).await {
                        warn!("Failed to stop VM {}: {}", instance.name, e);
                    }
                }

                if let Err(e) = provider.delete_vm(&mut instance.clone()).await {
                    warn!("Failed to delete VM {}: {}", instance.name, e);
                }
            }
        }

        self.instances.clear();
        Ok(())
    }

    pub async fn send_keys_to_vm(&self, instance: &VmInstance, keys: &[String]) -> Result<()> {
        let provider = self.get_provider(&instance.provider)?;
        provider.send_keys(instance, keys).await
    }

    pub async fn capture_screen(&self, instance: &VmInstance) -> Result<image::DynamicImage> {
        let provider = self.get_provider(&instance.provider)?;
        provider.capture_screen(instance).await
    }

    pub async fn get_console_output(&self, instance: &VmInstance) -> Result<String> {
        let provider = self.get_provider(&instance.provider)?;
        provider.get_console_output(instance).await
    }

    pub fn get_provider(&self, provider_type: &VmProvider) -> Result<Box<dyn VmProviderTrait>> {
        Ok(create_provider(provider_type))
    }

    // Utility parsing methods

    fn parse_memory_size(&self, size: &str) -> Result<u64> {
        let size_lower = size.to_lowercase();
        if size_lower.ends_with('g') || size_lower.ends_with("gb") {
            let num: u64 = size_lower
                .trim_end_matches("gb")
                .trim_end_matches('g')
                .parse()?;
            Ok(num * 1024) // Convert GB to MB
        } else if size_lower.ends_with('m') || size_lower.ends_with("mb") {
            let num: u64 = size_lower
                .trim_end_matches("mb")
                .trim_end_matches('m')
                .parse()?;
            Ok(num)
        } else {
            Err(anyhow!("Invalid memory size format: {}", size))
        }
    }

    fn parse_disk_size(&self, size: &str) -> Result<u64> {
        let size_lower = size.to_lowercase();
        if size_lower.ends_with('g') || size_lower.ends_with("gb") {
            let num: u64 = size_lower
                .trim_end_matches("gb")
                .trim_end_matches('g')
                .parse()?;
            Ok(num)
        } else if size_lower.ends_with('t') || size_lower.ends_with("tb") {
            let num: u64 = size_lower
                .trim_end_matches("tb")
                .trim_end_matches('t')
                .parse()?;
            Ok(num * 1024) // Convert TB to GB
        } else {
            Err(anyhow!("Invalid disk size format: {}", size))
        }
    }

    fn parse_duration(&self, duration: &str) -> Result<Duration> {
        let duration_lower = duration.to_lowercase();
        if duration_lower.ends_with('s') {
            let secs: u64 = duration_lower.trim_end_matches('s').parse()?;
            Ok(Duration::from_secs(secs))
        } else if duration_lower.ends_with('m') {
            let mins: u64 = duration_lower.trim_end_matches('m').parse()?;
            Ok(Duration::from_secs(mins * 60))
        } else if duration_lower.ends_with('h') {
            let hours: u64 = duration_lower.trim_end_matches('h').parse()?;
            Ok(Duration::from_secs(hours * 3600))
        } else if duration_lower.ends_with("ms") {
            let millis: u64 = duration_lower.trim_end_matches("ms").parse()?;
            Ok(Duration::from_millis(millis))
        } else {
            Err(anyhow!("Invalid duration format: {}", duration))
        }
    }
}
