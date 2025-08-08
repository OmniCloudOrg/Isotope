use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info};

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

    pub fn create_live_iso(&self, snapshot_path: &Path, output_path: &Path, pack_stage: &Stage) -> Result<()> {
        info!("Creating live ISO from snapshot: {}", snapshot_path.display());
        
        // Create temporary working directory
        std::fs::create_dir_all(&self.temp_dir)
            .context("Failed to create ISO working directory")?;

        // Extract VM snapshot/disk to filesystem
        let extracted_fs_path = self.temp_dir.join("extracted_fs");
        self.extract_vm_filesystem(snapshot_path, &extracted_fs_path)?;

        // Prepare ISO filesystem structure
        let iso_fs_path = self.temp_dir.join("iso_fs");
        self.prepare_iso_filesystem(&extracted_fs_path, &iso_fs_path)?;

        // Make it bootable
        self.make_bootable(&iso_fs_path)?;

        // Package into final ISO
        self.package_final_iso(&iso_fs_path, output_path, pack_stage)?;

        info!("Live ISO created successfully: {}", output_path.display());
        Ok(())
    }

    fn extract_vm_filesystem(&self, snapshot_path: &Path, output_path: &Path) -> Result<()> {
        info!("Extracting VM filesystem from: {}", snapshot_path.display());

        std::fs::create_dir_all(output_path)
            .context("Failed to create extraction directory")?;

        // Mount the VM disk image and extract its contents
        if snapshot_path.extension().and_then(|s| s.to_str()) == Some("qcow2") {
            self.extract_qcow2_filesystem(snapshot_path, output_path)?;
        } else {
            return Err(anyhow!("Unsupported disk format: {}", snapshot_path.display()));
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
                "-f", "qcow2",
                "-O", "raw", 
                qcow2_path.to_str().unwrap(),
                raw_path.to_str().unwrap()
            ])
            .output()
            .context("Failed to convert QCOW2 to raw")?;

        if !output.status.success() {
            return Err(anyhow!("QEMU convert failed: {}", 
                String::from_utf8_lossy(&output.stderr)));
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
                "-f", "--show", 
                "-o", &offset.to_string(),
                raw_path.to_str().unwrap()
            ])
            .output()
            .context("Failed to create loop device")?;

        if !loop_output.status.success() {
            return Err(anyhow!("Failed to create loop device"));
        }

        let loop_device = String::from_utf8_lossy(&loop_output.stdout).trim().to_string();
        info!("Created loop device: {}", loop_device);

        // Mount the filesystem
        let mount_output = Command::new("mount")
            .args([
                "-o", "ro", // Read-only
                &loop_device,
                output_path.to_str().unwrap()
            ])
            .output()
            .context("Failed to mount filesystem")?;

        if !mount_output.status.success() {
            // Cleanup loop device
            let _ = Command::new("losetup")
                .args(["-d", &loop_device])
                .output();
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
                copy_path.to_str().unwrap()
            ])
            .output();

        // Cleanup mount and loop device
        let _ = Command::new("umount").args([output_path.to_str().unwrap()]).output();
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
        
        debug!("Would extract {} to {}", raw_path.display(), output_path.display());
        
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
        self.create_filesystem_size(&iso_fs.join("casper/filesystem.squashfs"), 
                                  &iso_fs.join("casper/filesystem.size"))?;

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
                "-comp", "xz",
                "-e", "boot" // Exclude boot directory from squashfs
            ])
            .output()
            .context("Failed to create squashfs")?;

        if !output.status.success() {
            return Err(anyhow!("mksquashfs failed: {}", 
                String::from_utf8_lossy(&output.stderr)));
        }

        info!("Created squashfs: {}", output_file.display());
        Ok(())
    }

    fn create_filesystem_size(&self, squashfs_path: &Path, size_file: &Path) -> Result<()> {
        let metadata = std::fs::metadata(squashfs_path)
            .context("Failed to get squashfs metadata")?;
        
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

        // In a real implementation, these files would be copied from syslinux package
        // For now, create placeholder files
        std::fs::write(isolinux_dir.join("isolinux.bin"), b"placeholder")?;
        std::fs::write(isolinux_dir.join("boot.cat"), b"placeholder")?;

        debug!("Copied isolinux files (placeholder implementation)");
        Ok(())
    }

    fn package_final_iso(&self, iso_fs: &Path, output_path: &Path, pack_stage: &Stage) -> Result<()> {
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
            std::fs::create_dir_all(parent)
                .context("Failed to create output directory")?;
        }

        // Use mkisofs/genisoimage to create the final ISO
        let mut cmd = Command::new("mkisofs");
        cmd.args([
            "-r", // Rock Ridge extensions
            "-V", &volume_label,
            "-cache-inodes",
            "-J", // Joliet extensions
            "-l", // Allow full 31 character filenames
        ]);

        if bootable {
            cmd.args([
                "-b", "isolinux/isolinux.bin",
                "-c", "isolinux/boot.cat",
                "-no-emul-boot",
                "-boot-load-size", "4",
                "-boot-info-table",
            ]);
        }

        cmd.args([
            "-o", output_path.to_str().unwrap(),
            iso_fs.to_str().unwrap()
        ]);

        let output = cmd.output()
            .context("Failed to run mkisofs")?;

        if !output.status.success() {
            return Err(anyhow!("mkisofs failed: {}", 
                String::from_utf8_lossy(&output.stderr)));
        }

        info!("Final ISO created: {}", output_path.display());
        Ok(())
    }
}