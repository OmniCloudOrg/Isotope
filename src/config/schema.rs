use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Root configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub project: ProjectInfo,
    pub source: SourceConfig,
    pub output: OutputConfig,
    #[serde(default)]
    pub build: BuildConfig,
    pub modifications: Vec<Modification>,
    #[serde(default)]
    pub test: Option<TestConfig>,
    #[serde(default)]
    pub gui_installation: Option<GuiInstallationConfig>,
    #[serde(default)]
    pub hooks: Option<HooksConfig>,
}

/// Project metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// Source ISO configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    #[serde(rename = "type")]
    pub source_type: String, // "iso", "dmg", etc.
    pub path: PathBuf,
    #[serde(default)]
    pub checksum: Option<ChecksumConfig>,
}

/// Checksum verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecksumConfig {
    #[serde(rename = "type")]
    pub checksum_type: String, // "sha256", "md5", etc.
    pub value: String,
}

/// Output ISO configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    pub path: PathBuf,
    #[serde(default = "default_iso_format")]
    pub format: String, // "iso9660", "dmg", etc.
    #[serde(default)]
    pub options: Option<OutputOptions>,
}

fn default_iso_format() -> String {
    "iso9660".to_string()
}

/// Output options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputOptions {
    #[serde(default = "default_bootable")]
    pub bootable: bool,
    #[serde(default)]
    pub compression: Option<String>, // "xz", "gzip", etc.
}

fn default_bootable() -> bool {
    true
}

/// Build configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuildConfig {
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
    #[serde(default = "default_cleanup")]
    pub cleanup: bool,
    #[serde(default = "default_verbosity")]
    pub verbosity: String,
    #[serde(default)]
    pub commands: Vec<String>,
}

fn default_cleanup() -> bool {
    true
}

fn default_verbosity() -> String {
    "info".to_string()
}

/// ISO modification
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Modification {
    #[serde(rename = "file_add")]
    FileAdd {
        source: PathBuf,
        destination: String,
        #[serde(default)]
        attributes: Option<FileAttributes>,
    },
    #[serde(rename = "file_modify")]
    FileModify {
        path: String,
        operations: Vec<FileOperation>,
    },
    #[serde(rename = "file_remove")]
    FileRemove {
        path: String,
    },
    #[serde(rename = "directory_add")]
    DirectoryAdd {
        source: PathBuf,
        destination: String,
    },
    #[serde(rename = "answer_file")]
    AnswerFile {
        template: PathBuf,
        destination: String,
        variables: HashMap<String, String>,
    },
    #[serde(rename = "binary_patch")]
    BinaryPatch {
        path: String,
        patches: Vec<BinaryPatchOperation>,
    },
    #[serde(rename = "boot_config")]
    BootConfig {
        target: String, // "isolinux", "grub", "any"
        parameters: BootParameters,
    },
}

/// File attributes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAttributes {
    #[serde(default)]
    pub permissions: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
}

/// File operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FileOperation {
    #[serde(rename = "replace")]
    Replace {
        pattern: String,
        replacement: String,
    },
    #[serde(rename = "append")]
    Append {
        content: String,
    },
    #[serde(rename = "regex_replace")]
    RegexReplace {
        pattern: String,
        replacement: String,
    },
}

/// Binary patch operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryPatchOperation {
    pub offset: String,
    pub original: String,
    pub replacement: String,
}

/// Boot configuration parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootParameters {
    pub timeout: u32,
    pub default_entry: String,
    pub entries: Vec<BootEntry>,
}

/// Boot entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootEntry {
    pub name: String,
    pub label: String,
    pub kernel_params: String,
}

/// VM testing configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestConfig {
    pub vm: VmConfig,
    #[serde(default = "default_boot_wait")]
    pub boot_wait: String,
    #[serde(default)]
    pub boot_keypress_sequence: Vec<KeypressSequence>,
    #[serde(default)]
    pub shutdown_command: Option<String>,
    #[serde(default)]
    pub ssh: Option<SshConfig>,
    #[serde(default)]
    pub winrm: Option<WinRmConfig>,
    #[serde(default)]
    pub provision: Vec<ProvisionStep>,
}

fn default_boot_wait() -> String {
    "10s".to_string()
}

/// VM configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmConfig {
    pub provider: String, // "qemu", "virtualbox", "vmware"
    #[serde(default = "default_memory")]
    pub memory: String,
    #[serde(default = "default_cpus")]
    pub cpus: u8,
    #[serde(default)]
    pub options: Vec<String>,
}

fn default_memory() -> String {
    "2G".to_string()
}

fn default_cpus() -> u8 {
    2
}

/// SSH configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConfig {
    pub username: String,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub private_key_path: Option<PathBuf>,
    #[serde(default = "default_ssh_timeout")]
    pub timeout: String,
}

fn default_ssh_timeout() -> String {
    "30m".to_string()
}

/// WinRM configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WinRmConfig {
    pub username: String,
    pub password: String,
    #[serde(default = "default_winrm_timeout")]
    pub timeout: String,
}

fn default_winrm_timeout() -> String {
    "60m".to_string()
}

/// Provisioning step
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProvisionStep {
    #[serde(rename = "shell")]
    Shell {
        #[serde(default)]
        script: Option<PathBuf>,
        #[serde(default)]
        inline: Option<Vec<String>>,
    },
    #[serde(rename = "powershell")]
    PowerShell {
        #[serde(default)]
        script: Option<PathBuf>,
        #[serde(default)]
        inline: Option<Vec<String>>,
    },
    #[serde(rename = "file")]
    File {
        source: PathBuf,
        destination: String,
    },
}

/// Keypress sequence configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeypressSequence {
    #[serde(default)]
    pub wait: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub key_text: Option<String>,
    #[serde(default)]
    pub key_command: Option<String>,
    #[serde(default)]
    pub repeat: Option<u32>,
    #[serde(default)]
    pub description: Option<String>,
}

/// GUI installation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuiInstallationConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub interactive_installation: Vec<InstallationStep>,
}

fn default_enabled() -> bool {
    false
}

/// Installation step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallationStep {
    pub description: String,
    pub detection: DetectionConfig,
    pub keypress_sequence: Vec<KeypressSequence>,
}

/// Detection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionConfig {
    #[serde(default)]
    pub wait_for_timeout: String,
    #[serde(default)]
    pub success_pattern: Option<String>,
    #[serde(default)]
    pub wait_for_login: Option<bool>,
    #[serde(default)]
    pub wait_for_desktop: Option<bool>,
}

/// Hooks configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    #[serde(default)]
    pub pre_extraction: Vec<String>,
    #[serde(default)]
    pub post_extraction: Vec<String>,
    #[serde(default)]
    pub pre_modification: Vec<String>,
    #[serde(default)]
    pub post_modification: Vec<String>,
    #[serde(default)]
    pub pre_packaging: Vec<String>,
    #[serde(default)]
    pub post_packaging: Vec<String>,
}