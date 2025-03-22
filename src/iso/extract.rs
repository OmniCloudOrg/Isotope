use anyhow::{Context, Result};
use log::{debug, info};
use std::path::{Path, PathBuf};

#[cfg(unix)]
mod unix {
    use super::*;
    use std::process::Command;
    use std::os::unix::fs::PermissionsExt;

    pub fn extract_iso_impl(iso_path: &Path, output_dir: &Path) -> Result<()> {
        info!("Extracting ISO on Unix system: {} -> {}", iso_path.display(), output_dir.display());
        
        // Unix-specific ISO extraction using xorriso
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
        
        // Set appropriate Unix permissions
        let isolinux_bin = output_dir.join("isolinux").join("isolinux.bin");
        if isolinux_bin.exists() {
            std::fs::set_permissions(&isolinux_bin, std::fs::Permissions::from_mode(0o755))
                .context("Failed to set executable permissions on isolinux.bin")?;
        }
        
        Ok(())
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::process::Command;

    pub fn extract_iso_impl(iso_path: &Path, output_dir: &Path) -> Result<()> {
        info!("Extracting ISO on Windows system: {} -> {}", iso_path.display(), output_dir.display());
        
        // Windows-specific ISO extraction using 7-Zip
        // Check if 7-Zip is available
        let seven_zip_path = "C:\\Program Files\\7-Zip\\7z.exe";
        
        if std::path::Path::new(seven_zip_path).exists() {
            let status = Command::new(seven_zip_path)
                .arg("x")
                .arg("-y")
                .arg(format!("-o{}", output_dir.display()))
                .arg(iso_path)
                .status()
                .context("Failed to execute 7-Zip command")?;
                
            if !status.success() {
                return Err(anyhow::anyhow!("7-Zip command failed with status: {}", status));
            }
        } else {
            // Fallback to built-in Windows ISO extraction (PowerShell)
            let script = format!(
                "Mount-DiskImage -ImagePath '{}'; $drive = (Get-DiskImage -ImagePath '{}' | Get-Volume).DriveLetter; Copy-Item -Path ($drive + ':\\*') -Destination '{}' -Recurse; Dismount-DiskImage -ImagePath '{}'",
                iso_path.display(), iso_path.display(), output_dir.display(), iso_path.display()
            );
            
            let status = Command::new("powershell")
                .arg("-Command")
                .arg(&script)
                .status()
                .context("Failed to execute PowerShell Mount-DiskImage command")?;
                
            if !status.success() {
                return Err(anyhow::anyhow!("PowerShell ISO extraction failed with status: {}", status));
            }
        }
        
        Ok(())
    }
}

// Platform-agnostic function that delegates to platform-specific implementations
pub fn extract_iso<P1: AsRef<Path>, P2: AsRef<Path>>(iso_path: P1, output_dir: P2) -> Result<()> {
    let iso_path = iso_path.as_ref();
    let output_dir = output_dir.as_ref();
    
    // Create the output directory if it doesn't exist
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create output directory: {}", output_dir.display()))?;
    
    // Call platform-specific implementation
    #[cfg(unix)]
    return unix::extract_iso_impl(iso_path, output_dir);
    
    #[cfg(windows)]
    return windows::extract_iso_impl(iso_path, output_dir);
    
    // Fallback implementation for unsupported platforms
    #[cfg(not(any(unix, windows)))]
    {
        return Err(anyhow::anyhow!("Unsupported platform: ISO extraction not implemented"));
    }
}