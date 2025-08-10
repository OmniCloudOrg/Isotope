use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

use crate::automation::vm::VmInstance;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmMetadataEntry {
    pub vm_name: String,
    pub vm_id: String,
    pub isotope_file: PathBuf,
    pub created_at: String, // ISO 8601 timestamp
    pub last_used: String,  // ISO 8601 timestamp
    pub provider: String,
    pub ssh_port: Option<u16>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct VmMetadata {
    pub vms: HashMap<String, VmMetadataEntry>, // Key: absolute path to .isotope file
}

impl VmMetadata {
    const METADATA_FILE: &'static str = ".isometa";

    pub fn load_from_current_dir() -> Result<Self> {
        let metadata_path = Path::new(Self::METADATA_FILE);

        if !metadata_path.exists() {
            debug!("No .isometa file found, starting with empty metadata");
            return Ok(Self::default());
        }

        let content = fs::read_to_string(metadata_path)
            .with_context(|| format!("Failed to read {}", Self::METADATA_FILE))?;

        let metadata: VmMetadata =
            serde_json::from_str(&content).with_context(|| "Failed to parse .isometa file")?;

        debug!("Loaded VM metadata with {} entries", metadata.vms.len());
        Ok(metadata)
    }

    pub fn save_to_current_dir(&self) -> Result<()> {
        let content =
            serde_json::to_string_pretty(self).context("Failed to serialize VM metadata")?;

        fs::write(Self::METADATA_FILE, content)
            .with_context(|| format!("Failed to write {}", Self::METADATA_FILE))?;

        debug!("Saved VM metadata with {} entries", self.vms.len());
        Ok(())
    }

    pub fn get_vm_for_isotope_file(&self, isotope_path: &Path) -> Option<&VmMetadataEntry> {
        let abs_path = match isotope_path.canonicalize() {
            Ok(path) => path,
            Err(_) => return None,
        };

        self.vms.get(&abs_path.to_string_lossy().to_string())
    }

    pub fn add_or_update_vm(
        &mut self,
        isotope_path: &Path,
        vm_instance: &VmInstance,
    ) -> Result<()> {
        let abs_path = isotope_path.canonicalize().with_context(|| {
            format!(
                "Failed to resolve absolute path for {}",
                isotope_path.display()
            )
        })?;
        let now = chrono::Utc::now().to_rfc3339();
        let key = abs_path.to_string_lossy().to_string();
        let entry = VmMetadataEntry {
            vm_name: vm_instance.name.clone(),
            vm_id: vm_instance.id.clone(),
            isotope_file: abs_path.clone(),
            created_at: if self.vms.contains_key(&key) {
                self.vms.get(&key).unwrap().created_at.clone()
            } else {
                now.clone()
            },
            last_used: now,
            provider: format!("{:?}", vm_instance.provider),
            ssh_port: Some(vm_instance.config.network_config.ssh_port),
        };
        info!(
            "Tracking VM {} for isotope file {}",
            vm_instance.name,
            abs_path.display()
        );
        self.vms.insert(key, entry);
        Ok(())
    }

    pub fn remove_vm(&mut self, isotope_path: &Path) -> Result<()> {
        let abs_path = isotope_path.canonicalize().with_context(|| {
            format!(
                "Failed to resolve absolute path for {}",
                isotope_path.display()
            )
        })?;

        let key = abs_path.to_string_lossy().to_string();
        if let Some(entry) = self.vms.remove(&key) {
            info!("Removed VM {} from metadata", entry.vm_name);
        }

        Ok(())
    }

    pub fn cleanup_stale_entries(&mut self) {
        let mut to_remove = Vec::new();

        for (key, entry) in &self.vms {
            // Remove entries where the isotope file no longer exists
            if !entry.isotope_file.exists() {
                to_remove.push(key.clone());
            }
        }

        for key in to_remove {
            if let Some(entry) = self.vms.remove(&key) {
                info!(
                    "Cleaned up stale entry for VM {} (isotope file no longer exists)",
                    entry.vm_name
                );
            }
        }
    }

    pub fn list_tracked_vms(&self) -> Vec<&VmMetadataEntry> {
        self.vms.values().collect()
    }
}
