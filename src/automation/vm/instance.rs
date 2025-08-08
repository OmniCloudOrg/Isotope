use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmInstance {
    pub id: String,
    pub name: String,
    pub provider: VmProvider,
    pub config: VmConfig,
    pub state: VmState,
    pub disk_path: Option<PathBuf>,
    pub iso_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VmProvider {
    Qemu,
    VirtualBox,
    VMware,
    HyperV,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmConfig {
    pub memory_mb: u64,
    pub cpus: u32,
    pub disk_size_gb: u64,
    pub boot_wait: Duration,
    pub timeout: Duration,
    pub additional_args: Vec<String>,
    pub network_config: NetworkConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub adapter_type: NetworkAdapterType,
    pub enable_ssh: bool,
    pub ssh_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkAdapterType {
    NAT,
    Bridged,
    HostOnly,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VmState {
    Created,
    Starting,
    Running,
    Stopping,
    Stopped,
    Suspended,
    Error(String),
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            memory_mb: 2048,
            cpus: 2,
            disk_size_gb: 20,
            boot_wait: Duration::from_secs(10),
            timeout: Duration::from_secs(1800),
            additional_args: Vec::new(),
            network_config: NetworkConfig::default(),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            adapter_type: NetworkAdapterType::NAT,
            enable_ssh: true,
            ssh_port: 22,
        }
    }
}

impl VmInstance {
    pub fn new(id: String, name: String, provider: VmProvider, config: VmConfig) -> Self {
        Self {
            id,
            name,
            provider,
            config,
            state: VmState::Created,
            disk_path: None,
            iso_path: None,
        }
    }

    pub fn set_disk_path(&mut self, path: PathBuf) {
        self.disk_path = Some(path);
    }

    pub fn set_iso_path(&mut self, path: PathBuf) {
        self.iso_path = Some(path);
    }

    pub fn set_state(&mut self, state: VmState) {
        self.state = state;
    }

    pub fn is_running(&self) -> bool {
        self.state == VmState::Running
    }

    pub fn is_stopped(&self) -> bool {
        matches!(self.state, VmState::Stopped | VmState::Created)
    }

    pub fn has_error(&self) -> bool {
        matches!(self.state, VmState::Error(_))
    }
}