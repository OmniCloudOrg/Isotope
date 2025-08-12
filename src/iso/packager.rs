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

    pub fn create_live_iso(
        &self,
        snapshot_path: &Path,
        output_path: &Path,
        pack_stage: &Stage,
    ) -> Result<()> {
        info!(
            "Creating bootable image from VM disk: {}",
            snapshot_path.display()
        );

        // Check output format from pack stage
        let mut format = "raw".to_string();
        for instruction in &pack_stage.instructions {
            if let Instruction::Format { format: fmt } = instruction {
                format = fmt.clone();
            }
        }

        // Create output directory if it doesn't exist
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create output directory")?;
        }

        match format.as_str() {
            "raw" | "img" => {
                // Convert VM disk to raw IMG format using qemu-img
                self.convert_to_raw_img(snapshot_path, output_path)?;
            }
            "iso9660" => {
                warn!("ISO format requested, but creating raw IMG instead for better compatibility");
                // Change extension to .img for raw format
                let img_path = output_path.with_extension("img");
                self.convert_to_raw_img(snapshot_path, &img_path)?;
                info!("Created raw IMG instead of ISO for better VM compatibility: {}", img_path.display());
            }
            _ => {
                warn!("Unsupported format '{}', defaulting to raw IMG", format);
                let img_path = output_path.with_extension("img");
                self.convert_to_raw_img(snapshot_path, &img_path)?;
            }
        }

        info!("Bootable image created successfully: {}", output_path.display());
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

    fn copy_boot_files(&self, source_fs: &Path, iso_fs: &Path) -> Result<()> {
        info!("Copying boot files");

        let boot_source = source_fs.join("boot");
        if boot_source.exists() {
            // Copy kernel
            if let Ok(entries) = std::fs::read_dir(&boot_source) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();

                    if name_str.starts_with("vmlinuz") {
                        std::fs::copy(entry.path(), iso_fs.join("casper/vmlinuz"))?;
                        info!("Copied kernel: {}", name_str);
                    } else if name_str.starts_with("initrd") {
                        std::fs::copy(entry.path(), iso_fs.join("casper/initrd"))?;
                        info!("Copied initrd: {}", name_str);
                    }
                }
            }
        }

        Ok(())
    }

    fn create_squashfs(&self, source_fs: &Path, output_file: &Path) -> Result<()> {
        info!("Creating squashfs filesystem");

        let output = Command::new("mksquashfs")
            .args([
                source_fs.to_str().unwrap(),
                output_file.to_str().unwrap(),
                "-comp",
                "xz",
                "-e",
                "boot", // Exclude boot directory from squashfs
            ])
            .output()
            .context("Failed to create squashfs")?;

        if !output.status.success() {
            return Err(anyhow!(
                "mksquashfs failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        info!("Created squashfs: {}", output_file.display());
        Ok(())
    }

    fn create_filesystem_size(&self, squashfs_path: &Path, size_file: &Path) -> Result<()> {
        let metadata =
            std::fs::metadata(squashfs_path).context("Failed to get squashfs metadata")?;

        std::fs::write(size_file, metadata.len().to_string())
            .context("Failed to write filesystem.size")?;

        Ok(())
    }

    fn make_bootable(&self, iso_fs: &Path) -> Result<()> {
        info!("Making ISO bootable");

        // Create isolinux boot configuration
        self.create_isolinux_config(iso_fs)?;

        // Copy isolinux bootloader files
        self.copy_isolinux_files(iso_fs)?;

        Ok(())
    }

    fn create_isolinux_config(&self, iso_fs: &Path) -> Result<()> {
        let isolinux_dir = iso_fs.join("isolinux");
        std::fs::create_dir_all(&isolinux_dir)?;

        let config_content = r#"
DEFAULT live
TIMEOUT 10
PROMPT 0

LABEL live
  SAY Booting Live System...
  KERNEL /casper/vmlinuz
  APPEND initrd=/casper/initrd boot=casper quiet splash
"#;

        std::fs::write(isolinux_dir.join("isolinux.cfg"), config_content)
            .context("Failed to create isolinux.cfg")?;

        info!("Created isolinux configuration");
        Ok(())
    }

    fn copy_isolinux_files(&self, iso_fs: &Path) -> Result<()> {
        let isolinux_dir = iso_fs.join("isolinux");

        // Note: Direct VDI->IMG conversion doesn't need isolinux files

        // Try to find isolinux files from multiple sources
        let possible_sources = vec![
            // Common syslinux installation paths on Windows
            PathBuf::from("C:\\Program Files\\syslinux"),
            PathBuf::from("C:\\Program Files (x86)\\syslinux"),
            // Portable syslinux directory
            PathBuf::from(".\\syslinux"),
            PathBuf::from("syslinux"),
        ];

        let required_files = vec![
            ("isolinux.bin", "isolinux/isolinux.bin"),
            ("ldlinux.c32", "isolinux/ldlinux.c32"),
        ];

        let mut found_source = None;
        for source_dir in &possible_sources {
            if source_dir.exists() {
                // Check if all required files exist in this source
                let all_exist = required_files.iter().all(|(_, rel_path)| {
                    source_dir.join(rel_path).exists()
                        || source_dir.join(rel_path.replace("isolinux/", "")).exists()
                });

                if all_exist {
                    found_source = Some(source_dir);
                    break;
                }
            }
        }

        if let Some(source_dir) = found_source {
            info!("Found syslinux files in: {}", source_dir.display());

            // Copy the required files
            for (filename, rel_path) in &required_files {
                let source_file = if source_dir.join(rel_path).exists() {
                    source_dir.join(rel_path)
                } else {
                    source_dir.join(filename)
                };

                let dest_file = isolinux_dir.join(filename);
                std::fs::copy(&source_file, &dest_file).with_context(|| {
                    format!("Failed to copy {} from {}", filename, source_file.display())
                })?;

                debug!("Copied {}", filename);
            }

            // boot.cat is generated by mkisofs/genisoimage, create empty placeholder
            std::fs::write(isolinux_dir.join("boot.cat"), b"")?;
        } else {
            // Fallback: create minimal bootloader files
            warn!("Syslinux files not found, creating minimal bootable structure");

            // Create a minimal MBR boot record (simplified)
            let minimal_isolinux_bin = vec![0u8; 2048]; // 2KB minimal boot sector
            std::fs::write(isolinux_dir.join("isolinux.bin"), minimal_isolinux_bin)?;

            // Create empty ldlinux.c32 (required for newer isolinux)
            std::fs::write(isolinux_dir.join("ldlinux.c32"), b"")?;

            // boot.cat will be created by mkisofs
            std::fs::write(isolinux_dir.join("boot.cat"), b"")?;

            info!("Created minimal isolinux files (may not be fully functional)");
        }

        debug!("Isolinux files preparation complete");
        Ok(())
    }

}
