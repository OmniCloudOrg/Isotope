use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info, warn};

use crate::config::{Instruction, Stage};

pub struct IsoPackager {
    temp_dir: PathBuf,
}

impl IsoPackager {
    pub fn new() -> Self {
        Self {
            temp_dir: std::env::temp_dir().join("isotope-iso-work"),
        }
    }

    pub fn create_bootable_image(
        &self,
        vdi_path: &Path,
        output_path: &Path,
        _pack_stage: &Stage,
    ) -> Result<()> {
        info!(
            "Creating bootable IMG from VDI disk: {}",
            vdi_path.display()
        );

        // Always create raw IMG format - this is what we support
        let img_path = output_path.with_extension("img");
        
        // Create output directory if it doesn't exist
        if let Some(parent) = img_path.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create output directory")?;
        }

        // Convert VDI to raw IMG using VBoxManage
        self.convert_to_raw_img(vdi_path, &img_path)?;

        info!("Bootable IMG created successfully: {}", img_path.display());
        Ok(())
    }

    fn convert_to_raw_img(&self, source_path: &Path, output_path: &Path) -> Result<()> {
        info!("Converting {} to raw IMG format", source_path.display());

        // We only support VDI files from VirtualBox
        let source_format = source_path.extension().and_then(|s| s.to_str()).unwrap_or("unknown");

        if source_format != "vdi" {
            return Err(anyhow!(
                "Unsupported disk format: {}. Only VDI files from VirtualBox are supported.",
                source_format
            ));
        }

        // Use VirtualBox VBoxManage to convert VDI to raw
        info!("Converting VDI to raw using VBoxManage");

        let output = Command::new("VBoxManage")
            .args([
                "clonemedium",
                "disk",
                source_path.to_str().unwrap(),
                output_path.to_str().unwrap(),
                "--format",
                "RAW",
            ])
            .output()
            .context("Failed to execute VBoxManage clonemedium")?;

        if !output.status.success() {
            return Err(anyhow!(
                "VBoxManage clonemedium failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        info!("Successfully converted VDI to raw IMG using VBoxManage");
        Ok(())
    }
}
