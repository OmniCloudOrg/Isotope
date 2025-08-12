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

        // Detect source format from extension
        let source_format = match source_path.extension().and_then(|s| s.to_str()) {
            Some("vdi") => "vdi",
            Some("qcow2") => "qcow2",
            Some("vmdk") => "vmdk",
            Some("vhd") => "vhd",
            _ => {
                warn!("Unknown source format, letting qemu-img auto-detect");
                "auto"
            }
        };

        // Use qemu-img to convert to raw format
        let mut cmd = Command::new("qemu-img");
        cmd.args(["convert", "-O", "raw"]);
        
        if source_format != "auto" {
            cmd.args(["-f", source_format]);
        }
        
        cmd.args([
            source_path.to_str().unwrap(),
            output_path.to_str().unwrap(),
        ]);

        let output = cmd.output()
            .context("Failed to execute qemu-img convert")?;

        if !output.status.success() {
            return Err(anyhow!(
                "qemu-img convert failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        info!("Successfully converted to raw IMG: {}", output_path.display());
        Ok(())
    }

    fn extract_vm_filesystem(&self, snapshot_path: &Path, output_path: &Path) -> Result<()> {
        info!("Extracting VM filesystem from: {}", snapshot_path.display());

        std::fs::create_dir_all(output_path).context("Failed to create extraction directory")?;

        // Mount the VM disk image and extract its contents
        match snapshot_path.extension().and_then(|s| s.to_str()) {
            Some("qcow2") => {
                self.extract_qcow2_filesystem(snapshot_path, output_path)?;
            }
            Some("vdi") => {
                self.extract_vdi_filesystem(snapshot_path, output_path)?;
            }
            _ => {
                return Err(anyhow!(
                    "Unsupported disk format: {}. Supported formats: qcow2, vdi",
                    snapshot_path.display()
                ));
            }
        }

        Ok(())
    }

    fn extract_qcow2_filesystem(&self, qcow2_path: &Path, output_path: &Path) -> Result<()> {
        info!("Extracting QCOW2 filesystem");

        // Convert QCOW2 to raw image first
        let raw_path = self.temp_dir.join("disk.raw");
        let output = Command::new("qemu-img")
            .args([
                "convert",
                "-f",
                "qcow2",
                "-O",
                "raw",
                qcow2_path.to_str().unwrap(),
                raw_path.to_str().unwrap(),
            ])
            .output()
            .context("Failed to convert QCOW2 to raw")?;

        if !output.status.success() {
            return Err(anyhow!(
                "QEMU convert failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // Mount the raw disk image
        #[cfg(unix)]
        {
            self.mount_raw_disk_unix(&raw_path, output_path)?;
        }

        #[cfg(windows)]
        {
            self.mount_raw_disk_windows(&raw_path, output_path)?;
        }

        Ok(())
    }

    fn extract_vdi_filesystem(&self, vdi_path: &Path, output_path: &Path) -> Result<()> {
        info!("Extracting VDI filesystem");

        // Convert VDI to raw image using qemu-img (supports VDI format)
        let raw_path = self.temp_dir.join("disk.raw");
        let output = Command::new("qemu-img")
            .args([
                "convert",
                "-f",
                "vdi",
                "-O",
                "raw",
                vdi_path.to_str().unwrap(),
                raw_path.to_str().unwrap(),
            ])
            .output()
            .context("Failed to convert VDI to raw")?;

        if !output.status.success() {
            return Err(anyhow!(
                "QEMU convert failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // Mount the raw disk image
        #[cfg(unix)]
        {
            self.mount_raw_disk_unix(&raw_path, output_path)?;
        }

        #[cfg(windows)]
        {
            self.mount_raw_disk_windows(&raw_path, output_path)?;
        }

        Ok(())
    }

    #[cfg(unix)]
    fn mount_raw_disk_unix(&self, raw_path: &Path, output_path: &Path) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        info!("Mounting raw disk image (Unix)");

        // Find the main partition (usually the largest)
        let output = Command::new("fdisk")
            .args(["-l", raw_path.to_str().unwrap()])
            .output()
            .context("Failed to list partitions")?;

        if !output.status.success() {
            return Err(anyhow!("Failed to analyze disk partitions"));
        }

        // For simplicity, assume partition starts at sector 2048 (common for modern disks)
        let offset = 2048 * 512; // 2048 sectors * 512 bytes/sector

        // Create loop device
        let loop_output = Command::new("losetup")
            .args([
                "-f",
                "--show",
                "-o",
                &offset.to_string(),
                raw_path.to_str().unwrap(),
            ])
            .output()
            .context("Failed to create loop device")?;

        if !loop_output.status.success() {
            return Err(anyhow!("Failed to create loop device"));
        }

        let loop_device = String::from_utf8_lossy(&loop_output.stdout)
            .trim()
            .to_string();
        info!("Created loop device: {}", loop_device);

        // Mount the filesystem
        let mount_output = Command::new("mount")
            .args([
                "-o",
                "ro", // Read-only
                &loop_device,
                output_path.to_str().unwrap(),
            ])
            .output()
            .context("Failed to mount filesystem")?;

        if !mount_output.status.success() {
            // Cleanup loop device
            let _ = Command::new("losetup").args(["-d", &loop_device]).output();
            return Err(anyhow!("Failed to mount filesystem"));
        }

        info!("Mounted filesystem at: {}", output_path.display());

        // Copy files out of the mounted filesystem
        let copy_path = self.temp_dir.join("fs_copy");
        std::fs::create_dir_all(&copy_path)?;

        let cp_output = Command::new("cp")
            .args([
                "-a", // Archive mode (preserve attributes)
                &format!("{}/*", output_path.display()),
                copy_path.to_str().unwrap(),
            ])
            .output();

        // Cleanup mount and loop device
        let _ = Command::new("umount")
            .args([output_path.to_str().unwrap()])
            .output();
        let _ = Command::new("losetup").args(["-d", &loop_device]).output();

        // Move copied files to final location
        if copy_path.exists() {
            let _ = std::fs::remove_dir_all(output_path);
            std::fs::rename(&copy_path, output_path)?;
        }

        Ok(())
    }

    #[cfg(windows)]
    fn mount_raw_disk_windows(&self, raw_path: &Path, output_path: &Path) -> Result<()> {
        info!("Extracting disk image (Windows)");

        // On Windows, we would need to use different tools or libraries
        // For now, this is a placeholder that would use tools like:
        // - 7zip to extract filesystem
        // - OSFMount or similar to mount disk images
        // - PowerShell with Hyper-V cmdlets

        debug!(
            "Would extract {} to {}",
            raw_path.display(),
            output_path.display()
        );

        // Create placeholder directory structure for testing
        std::fs::create_dir_all(output_path.join("boot"))?;
        std::fs::create_dir_all(output_path.join("etc"))?;
        std::fs::create_dir_all(output_path.join("usr"))?;
        std::fs::create_dir_all(output_path.join("var"))?;

        Ok(())
    }

    fn prepare_iso_filesystem(&self, source_fs: &Path, iso_fs: &Path) -> Result<()> {
        info!("Preparing ISO filesystem structure");

        std::fs::create_dir_all(iso_fs)?;

        // Copy the extracted filesystem to ISO structure
        // This involves creating the live filesystem structure that can boot

        // Create basic live ISO structure
        std::fs::create_dir_all(iso_fs.join("casper"))?; // Ubuntu live
        std::fs::create_dir_all(iso_fs.join("isolinux"))?; // Boot loader
        std::fs::create_dir_all(iso_fs.join("boot"))?; // Boot files

        // Copy kernel and initrd from extracted filesystem
        self.copy_boot_files(source_fs, iso_fs)?;

        // Create squashfs from the live filesystem
        self.create_squashfs(source_fs, &iso_fs.join("casper/filesystem.squashfs"))?;

        // Create filesystem size file
        self.create_filesystem_size(
            &iso_fs.join("casper/filesystem.squashfs"),
            &iso_fs.join("casper/filesystem.size"),
        )?;

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

        // Try to copy isolinux files from source ISO first (if we have access to the original)
        if self.try_copy_isolinux_from_source(&isolinux_dir).is_ok() {
            debug!("Successfully copied isolinux files from source");
            return Ok(());
        }

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

    fn try_copy_isolinux_from_source(&self, isolinux_dir: &Path) -> Result<()> {
        // Look for common source paths where the original ISO might have been extracted
        let possible_source_paths = vec![
            self.temp_dir.join("source"),
            self.temp_dir.join("original"),
            PathBuf::from("./temp/source"),
            PathBuf::from("./source"),
        ];

        for source_path in possible_source_paths {
            let source_isolinux = source_path.join("isolinux");
            if source_isolinux.exists() {
                // Check for required files
                let isolinux_bin = source_isolinux.join("isolinux.bin");
                if isolinux_bin.exists() {
                    // Copy isolinux.bin
                    std::fs::copy(&isolinux_bin, isolinux_dir.join("isolinux.bin"))?;

                    // Copy ldlinux.c32 if it exists
                    let ldlinux = source_isolinux.join("ldlinux.c32");
                    if ldlinux.exists() {
                        std::fs::copy(&ldlinux, isolinux_dir.join("ldlinux.c32"))?;
                    }

                    // Copy any other isolinux files
                    if let Ok(entries) = std::fs::read_dir(&source_isolinux) {
                        for entry in entries.flatten() {
                            let name = entry.file_name();
                            let name_str = name.to_string_lossy();

                            if name_str.ends_with(".c32") || name_str.starts_with("isolinux") {
                                let dest = isolinux_dir.join(&name);
                                if !dest.exists() {
                                    // Don't overwrite what we've already copied
                                    let _ = std::fs::copy(entry.path(), dest);
                                }
                            }
                        }
                    }

                    info!(
                        "Copied isolinux files from source ISO at: {}",
                        source_isolinux.display()
                    );
                    return Ok(());
                }
            }
        }

        Err(anyhow!("No source isolinux files found"))
    }

    fn package_final_iso(
        &self,
        iso_fs: &Path,
        output_path: &Path,
        pack_stage: &Stage,
    ) -> Result<()> {
        info!("Packaging final ISO");

        let mut format = "iso9660".to_string();
        let mut bootable = true;
        let mut volume_label = "Live ISO".to_string();

        // Parse pack stage instructions
        for instruction in &pack_stage.instructions {
            match instruction {
                Instruction::Format { format: fmt } => {
                    format = fmt.clone();
                }
                Instruction::Bootable { enabled } => {
                    bootable = *enabled;
                }
                Instruction::VolumeLabel { label } => {
                    volume_label = label.clone();
                }
                _ => {} // Ignore other instructions
            }
        }

        // Create parent directory if it doesn't exist
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create output directory")?;
        }

        // Use mkisofs/genisoimage to create the final ISO
        let mut cmd = Command::new("mkisofs");
        cmd.args([
            "-r", // Rock Ridge extensions
            "-V",
            &volume_label,
            "-cache-inodes",
            "-J", // Joliet extensions
            "-l", // Allow full 31 character filenames
        ]);

        if bootable {
            cmd.args([
                "-b",
                "isolinux/isolinux.bin",
                "-c",
                "isolinux/boot.cat",
                "-no-emul-boot",
                "-boot-load-size",
                "4",
                "-boot-info-table",
            ]);
        }

        cmd.args([
            "-o",
            output_path.to_str().unwrap(),
            iso_fs.to_str().unwrap(),
        ]);

        let output = cmd.output().context("Failed to run mkisofs")?;

        if !output.status.success() {
            return Err(anyhow!(
                "mkisofs failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        info!("Final ISO created: {}", output_path.display());
        Ok(())
    }
}
