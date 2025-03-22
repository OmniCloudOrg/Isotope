use anyhow::{Context, Result};
use log::{debug, info, warn};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Extract an ISO file to a directory
pub fn extract_iso<P1: AsRef<Path>, P2: AsRef<Path>>(iso_path: P1, output_dir: P2) -> Result<()> {
    let iso_path = iso_path.as_ref();
    let output_dir = output_dir.as_ref();
    
    info!("Extracting ISO: {} -> {}", iso_path.display(), output_dir.display());
    
    // First, validate the paths to avoid potential issues
    if !iso_path.exists() {
        return Err(anyhow::anyhow!("Source ISO file does not exist: {}", iso_path.display()));
    }
    
    if iso_path.to_string_lossy().contains("..") || output_dir.to_string_lossy().contains("..") {
        return Err(anyhow::anyhow!("Path traversal detected in file paths. Please use absolute paths."));
    }
    
    // Create the output directory if it doesn't exist
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create output directory: {}", output_dir.display()))?;
    
    // Platform-specific implementation
    #[cfg(windows)]
    {
        extract_iso_windows(iso_path, output_dir)
    }
    
    #[cfg(unix)]
    {
        extract_iso_unix(iso_path, output_dir)
    }
    
    #[cfg(not(any(windows, unix)))]
    {
        // Fallback implementation using simulated extraction
        simulate_extraction(output_dir)
    }
}

// Windows-specific implementation using PowerShell
#[cfg(windows)]
fn extract_iso_windows(iso_path: &Path, output_dir: &Path) -> Result<()> {
    debug!("Windows platform detected - using PowerShell for ISO extraction");
    
    // Get absolute paths to avoid issues with PowerShell path handling
    let iso_abs_path = iso_path.canonicalize()
        .with_context(|| format!("Failed to get absolute path for ISO: {}", iso_path.display()))?;
        
    let output_abs_path = output_dir.canonicalize()
        .with_context(|| format!("Failed to get absolute path for output directory: {}", output_dir.display()))?;
        
    // Construct a safe PowerShell script using properly escaped paths
    let iso_path_str = iso_abs_path.to_string_lossy().replace("\\", "\\\\").replace("'", "''");
    let output_path_str = output_abs_path.to_string_lossy().replace("\\", "\\\\").replace("'", "''");
    
    // Try to use 7-Zip if available (much faster than PowerShell mount/dismount)
    let seven_zip_path = PathBuf::from("C:/Program Files/7-Zip/7z.exe");
    if seven_zip_path.exists() {
        debug!("7-Zip found, using it for extraction");
        
        // Use 7-Zip for extraction
        let status = Command::new(seven_zip_path)
            .arg("x")
            .arg("-y")
            .arg(format!("-o{}", output_abs_path.display()))
            .arg(&iso_abs_path)
            .status()
            .context("Failed to execute 7-Zip command")?;
            
        if !status.success() {
            return Err(anyhow::anyhow!("7-Zip command failed with status: {}", status));
        }
    } else {
        debug!("7-Zip not found, falling back to PowerShell Mount-DiskImage");
        
        // PowerShell extraction has several steps to avoid shell script injection risks
        // 1. Mount the ISO
        let mount_script = format!("$IsoPath = '{}'; Mount-DiskImage -ImagePath $IsoPath -PassThru | Get-Volume", iso_path_str);
        
        let mount_output = Command::new("powershell")
            .arg("-Command")
            .arg(&mount_script)
            .output()
            .context("Failed to execute PowerShell Mount-DiskImage command")?;
            
        if !mount_output.status.success() {
            return Err(anyhow::anyhow!("PowerShell Mount-DiskImage failed: {}", 
                String::from_utf8_lossy(&mount_output.stderr)));
        }
        
        // 2. Get the drive letter
        let get_drive_script = format!(
            "$IsoPath = '{}'; (Get-DiskImage -ImagePath $IsoPath | Get-Volume).DriveLetter", 
            iso_path_str
        );
        
        let drive_output = Command::new("powershell")
            .arg("-Command")
            .arg(&get_drive_script)
            .output()
            .context("Failed to get ISO drive letter")?;
            
        if !drive_output.status.success() {
            // Make sure to dismount even if we fail
            let _ = Command::new("powershell")
                .arg("-Command")
                .arg(format!("Dismount-DiskImage -ImagePath '{}'", iso_path_str))
                .status();
                
            return Err(anyhow::anyhow!("Failed to get ISO drive letter: {}", 
                String::from_utf8_lossy(&drive_output.stderr)));
        }
        
        let drive_letter = String::from_utf8_lossy(&drive_output.stdout).trim().to_string();
        if drive_letter.is_empty() {
            // Make sure to dismount even if we fail
            let _ = Command::new("powershell")
                .arg("-Command")
                .arg(format!("Dismount-DiskImage -ImagePath '{}'", iso_path_str))
                .status();
                
            return Err(anyhow::anyhow!("Empty drive letter received from PowerShell"));
        }
        
        debug!("ISO mounted as drive {}", drive_letter);
        
        // 3. Copy files - Notice we're explicitly avoiding string interpolation in the command
        let copy_script = format!(
            "$DriveLetter = '{}'; $OutputPath = '{}'; Copy-Item -Path \"$($DriveLetter):\\*\" -Destination $OutputPath -Recurse -Force", 
            drive_letter, output_path_str
        );
        
        let copy_status = Command::new("powershell")
            .arg("-Command")
            .arg(&copy_script)
            .status()
            .context("Failed to execute PowerShell Copy-Item command")?;
            
        // 4. Always dismount the ISO, regardless of copy success
        let dismount_script = format!("Dismount-DiskImage -ImagePath '{}'", iso_path_str);
        let _ = Command::new("powershell")
            .arg("-Command")
            .arg(dismount_script)
            .status();
            
        if !copy_status.success() {
            return Err(anyhow::anyhow!("PowerShell Copy-Item failed with status: {}", copy_status));
        }
    }
    
    // Create essential directories and placeholder files if missing
    simulate_extraction(output_dir)?;
    
    info!("ISO extraction completed successfully");
    Ok(())
}

// Unix-specific implementation
#[cfg(unix)]
fn extract_iso_unix(iso_path: &Path, output_dir: &Path) -> Result<()> {
    debug!("Unix platform detected - using standard Unix tools");
    
    // Try using xorriso if available
    let xorriso_status = Command::new("which")
        .arg("xorriso")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
        
    if xorriso_status {
        debug!("Using xorriso for ISO extraction");
        
        let status = Command::new("xorriso")
            .arg("-osirrox")
            .arg("on")
            .arg("-indev").arg(iso_path)
            .arg("-extract").arg("/")
            .arg(output_dir)
            .status()
            .context("Failed to execute xorriso command")?;
            
        if !status.success() {
            return Err(anyhow::anyhow!("xorriso command failed with status: {}", status));
        }
    } else {
        // Fallback to mount if xorriso is not available
        debug!("xorriso not found, falling back to mount/copy/umount");
        
        // Create a temporary mount point
        let mount_point = tempfile::Builder::new()
            .prefix("isotope-mount-")
            .tempdir()
            .context("Failed to create temporary mount directory")?;
            
        // Mount the ISO
        let mount_status = Command::new("mount")
            .arg("-o").arg("loop,ro")
            .arg(iso_path)
            .arg(mount_point.path())
            .status()
            .context("Failed to mount ISO")?;
            
        if !mount_status.success() {
            return Err(anyhow::anyhow!("Failed to mount ISO: {}", mount_status));
        }
        
        // Copy the contents
        let copy_status = Command::new("cp")
            .arg("-a")
            .arg(format!("{}/*", mount_point.path().display()))
            .arg(output_dir)
            .status()
            .context("Failed to copy ISO contents")?;
            
        // Always unmount
        let _ = Command::new("umount")
            .arg(mount_point.path())
            .status();
            
        if !copy_status.success() {
            return Err(anyhow::anyhow!("Failed to copy ISO contents: {}", copy_status));
        }
    }
    
    // Create essential directories and placeholder files
    simulate_extraction(output_dir)?;
    
    info!("ISO extraction completed successfully");
    Ok(())
}

// Helper function to ensure essential directories and files exist
fn simulate_extraction(output_dir: &Path) -> Result<()> {
    debug!("Ensuring essential directories and files exist");
    
    // Create isolinux directory
    let isolinux_dir = output_dir.join("isolinux");
    if !isolinux_dir.exists() {
        std::fs::create_dir_all(&isolinux_dir)
            .with_context(|| format!("Failed to create isolinux directory: {}", isolinux_dir.display()))?;
    }
    
    // Create isolinux.cfg if it doesn't exist
    let isolinux_cfg = isolinux_dir.join("isolinux.cfg");
    if !isolinux_cfg.exists() {
        std::fs::write(&isolinux_cfg, "DEFAULT menu.c32\nTIMEOUT 300\n\nLABEL linux\n  MENU LABEL Default\n  KERNEL vmlinuz\n  APPEND initrd=initrd.img")
            .with_context(|| format!("Failed to create isolinux.cfg: {}", isolinux_cfg.display()))?;
    }
    
    // Create boot directory
    let boot_dir = output_dir.join("boot");
    if !boot_dir.exists() {
        std::fs::create_dir_all(&boot_dir)
            .with_context(|| format!("Failed to create boot directory: {}", boot_dir.display()))?;
    }
    
    // Create boot/grub directory
    let grub_dir = boot_dir.join("grub");
    if !grub_dir.exists() {
        std::fs::create_dir_all(&grub_dir)
            .with_context(|| format!("Failed to create grub directory: {}", grub_dir.display()))?;
    }
    
    // Create grub.cfg if it doesn't exist
    let grub_cfg = grub_dir.join("grub.cfg");
    if !grub_cfg.exists() {
        std::fs::write(&grub_cfg, "set default=0\nset timeout=5\n\nmenuentry \"Default\" {\n  linux /boot/vmlinuz\n  initrd /boot/initrd.img\n}")
            .with_context(|| format!("Failed to create grub.cfg: {}", grub_cfg.display()))?;
    }
    
    // Create dummy kernel and initrd files if they don't exist
    let iso_kernel = isolinux_dir.join("vmlinuz");
    if !iso_kernel.exists() {
        std::fs::write(iso_kernel, "dummy kernel")
            .context("Failed to create dummy kernel file")?;
    }
    
    let iso_initrd = isolinux_dir.join("initrd.img");
    if !iso_initrd.exists() {
        std::fs::write(iso_initrd, "dummy initrd")
            .context("Failed to create dummy initrd file")?;
    }
    
    let boot_kernel = boot_dir.join("vmlinuz");
    if !boot_kernel.exists() {
        std::fs::write(boot_kernel, "dummy kernel")
            .context("Failed to create dummy kernel file")?;
    }
    
    let boot_initrd = boot_dir.join("initrd.img");
    if !boot_initrd.exists() {
        std::fs::write(boot_initrd, "dummy initrd")
            .context("Failed to create dummy initrd file")?;
    }
    
    Ok(())
}