pub mod qemu;
pub mod virtualbox;
pub mod vmware;
pub mod hyperv;

use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;

use crate::automation::vm::VmInstance;

#[async_trait]
pub trait VmProviderTrait: Send + Sync {
    async fn create_vm(&self, instance: &mut VmInstance) -> Result<()>;
    async fn start_vm(&self, instance: &mut VmInstance) -> Result<()>;
    async fn stop_vm(&self, instance: &mut VmInstance) -> Result<()>;
    async fn delete_vm(&self, instance: &mut VmInstance) -> Result<()>;
    async fn attach_iso(&self, instance: &mut VmInstance, iso_path: &Path) -> Result<()>;
    async fn detach_iso(&self, instance: &mut VmInstance) -> Result<()>;
    async fn create_snapshot(&self, instance: &VmInstance, snapshot_name: &str) -> Result<()>;
    async fn restore_snapshot(&self, instance: &mut VmInstance, snapshot_name: &str) -> Result<()>;
    async fn is_running(&self, instance: &VmInstance) -> Result<bool>;
    async fn wait_for_shutdown(&self, instance: &VmInstance) -> Result<()>;
    async fn send_keys(&self, instance: &VmInstance, keys: &[String]) -> Result<()>;
    fn name(&self) -> &'static str;
}

pub fn create_provider(provider_type: &crate::automation::vm::VmProvider) -> Box<dyn VmProviderTrait> {
    match provider_type {
        crate::automation::vm::VmProvider::Qemu => Box::new(qemu::QemuProvider::new()),
        crate::automation::vm::VmProvider::VirtualBox => Box::new(virtualbox::VirtualBoxProvider::new()),
        crate::automation::vm::VmProvider::VMware => Box::new(vmware::VMwareProvider::new()),
        crate::automation::vm::VmProvider::HyperV => Box::new(hyperv::HyperVProvider::new()),
    }
}