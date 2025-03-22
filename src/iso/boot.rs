use anyhow::{Context, Result};
use log::{debug, info};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::schema::BootParameters;

/// Bootloader type
pub enum BootloaderType {
    Isolinux,
    Grub,
    Systemd,
    Unknown,
}

/// Detect the bootloader type in an ISO extraction directory
pub fn detect_bootloader(extraction_dir: &Path) -> BootloaderType {
    if extraction_dir.join("isolinux").join("isolinux.cfg").exists() {
        BootloaderType::Isolinux
    } else if extraction_dir.join("boot").join("grub").join("grub.cfg").exists() {
        BootloaderType::Grub
    } else if extraction_dir.join("boot").join("loader").exists() {
        BootloaderType::Systemd
    } else {
        BootloaderType::Unknown
    }
}

/// Configure isolinux bootloader
pub fn configure_isolinux(extraction_dir: &Path, parameters: &BootParameters) -> Result<()> {
    let isolinux_cfg = extraction_dir.join("isolinux").join("isolinux.cfg");
    
    if !isolinux_cfg.exists() {
        return Err(anyhow::anyhow!("ISOLINUX configuration file not found: {}", isolinux_cfg.display()));
    }
    
    info!("Configuring ISOLINUX bootloader: {}", isolinux_cfg.display());
    
    // Generate ISOLINUX configuration content
    let mut content = format!("DEFAULT {}\n", parameters.default_entry);
    content.push_str(&format!("TIMEOUT {}\n", parameters.timeout * 10)); // ISOLINUX uses tenths of a second
    
    // Add menu entries
    for entry in &parameters.entries {
        content.push_str(&format!("\nLABEL {}\n", entry.name));
        content.push_str(&format!("  MENU LABEL {}\n", entry.label));
        content.push_str("  KERNEL vmlinuz\n");
        content.push_str(&format!("  APPEND initrd=initrd.img {}\n", entry.kernel_params));
    }
    
    // Write the configuration
    fs::write(&isolinux_cfg, content)
        .with_context(|| format!("Failed to write ISOLINUX configuration: {}", isolinux_cfg.display()))?;
    
    debug!("ISOLINUX configuration updated");
    Ok(())
}

/// Configure GRUB bootloader
pub fn configure_grub(extraction_dir: &Path, parameters: &BootParameters) -> Result<()> {
    let grub_cfg = extraction_dir.join("boot").join("grub").join("grub.cfg");
    
    if !grub_cfg.exists() {
        return Err(anyhow::anyhow!("GRUB configuration file not found: {}", grub_cfg.display()));
    }
    
    info!("Configuring GRUB bootloader: {}", grub_cfg.display());
    
    // Generate GRUB configuration content
    let mut content = format!("set default=\"{}\"\n", parameters.default_entry);
    content.push_str(&format!("set timeout={}\n", parameters.timeout));
    
    // Add menu entries
    for entry in &parameters.entries {
        content.push_str(&format!("\nmenuentry \"{}\" {{\n", entry.label));
        content.push_str("  linux /boot/vmlinuz");
        content.push_str(&format!(" {}\n", entry.kernel_params));
        content.push_str("  initrd /boot/initrd.img\n");
        content.push_str("}\n");
    }
    
    // Write the configuration
    fs::write(&grub_cfg, content)
        .with_context(|| format!("Failed to write GRUB configuration: {}", grub_cfg.display()))?;
    
    debug!("GRUB configuration updated");
    Ok(())
}

/// Configure systemd-boot bootloader
pub fn configure_systemd_boot(extraction_dir: &Path, parameters: &BootParameters) -> Result<()> {
    let loader_dir = extraction_dir.join("boot").join("loader");
    
    if !loader_dir.exists() {
        return Err(anyhow::anyhow!("systemd-boot loader directory not found: {}", loader_dir.display()));
    }
    
    info!("Configuring systemd-boot bootloader: {}", loader_dir.display());
    
    // Create loader.conf
    let loader_conf = loader_dir.join("loader.conf");
    let mut loader_content = format!("default {}.conf\n", parameters.default_entry);
    loader_content.push_str(&format!("timeout {}\n", parameters.timeout));
    
    fs::write(&loader_conf, loader_content)
        .with_context(|| format!("Failed to write systemd-boot loader.conf: {}", loader_conf.display()))?;
    
    // Create entries directory if it doesn't exist
    let entries_dir = loader_dir.join("entries");
    fs::create_dir_all(&entries_dir)
        .with_context(|| format!("Failed to create systemd-boot entries directory: {}", entries_dir.display()))?;
    
    // Create entry files
    for entry in &parameters.entries {
        let entry_file = entries_dir.join(format!("{}.conf", entry.name));
        let mut entry_content = format!("title {}\n", entry.label);
        entry_content.push_str("linux /boot/vmlinuz\n");
        entry_content.push_str("initrd /boot/initrd.img\n");
        entry_content.push_str(&format!("options {}\n", entry.kernel_params));
        
        fs::write(&entry_file, entry_content)
            .with_context(|| format!("Failed to write systemd-boot entry file: {}", entry_file.display()))?;
    }
    
    debug!("systemd-boot configuration updated");
    Ok(())
}

/// Configure bootloader based on the detected type
pub fn configure_bootloader(extraction_dir: &Path, parameters: &BootParameters) -> Result<()> {
    match detect_bootloader(extraction_dir) {
        BootloaderType::Isolinux => {
            configure_isolinux(extraction_dir, parameters)
        },
        BootloaderType::Grub => {
            configure_grub(extraction_dir, parameters)
        },
        BootloaderType::Systemd => {
            configure_systemd_boot(extraction_dir, parameters)
        },
        BootloaderType::Unknown => {
            Err(anyhow::anyhow!("Unknown bootloader type. Could not detect ISOLINUX, GRUB, or systemd-boot."))
        }
    }
}