use anyhow::{Context, Result};
use log::debug;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Copy a directory recursively
pub fn copy_directory<P1: AsRef<Path>, P2: AsRef<Path>>(source: P1, destination: P2) -> Result<()> {
    let source = source.as_ref();
    let destination = destination.as_ref();
    
    debug!("Copying directory: {} -> {}", source.display(), destination.display());
    
    // Create the destination directory if it doesn't exist
    fs::create_dir_all(destination)
        .with_context(|| format!("Failed to create directory: {}", destination.display()))?;
    
    // Walk through the source directory
    for entry in WalkDir::new(source) {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();
        
        // Calculate the relative path from the source directory
        let relative_path = pathdiff::diff_paths(path, source)
            .ok_or_else(|| anyhow::anyhow!("Failed to calculate relative path"))?;
        
        // Calculate the destination path
        let dest_path = destination.join(&relative_path);
        
        if path.is_dir() {
            // Create directory in the destination
            fs::create_dir_all(&dest_path)
                .with_context(|| format!("Failed to create directory: {}", dest_path.display()))?;
        } else {
            // Copy file to the destination
            copy_file(path, &dest_path)
                .with_context(|| format!("Failed to copy file: {} -> {}", path.display(), dest_path.display()))?;
        }
    }
    
    debug!("Directory copied successfully");
    Ok(())
}

/// Copy a file with proper error handling
pub fn copy_file<P1: AsRef<Path>, P2: AsRef<Path>>(source: P1, destination: P2) -> Result<()> {
    let source = source.as_ref();
    let destination = destination.as_ref();
    
    // Create parent directories if they don't exist
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }
    
    // Open source file
    let mut source_file = File::open(source)
        .with_context(|| format!("Failed to open source file: {}", source.display()))?;
    
    // Create destination file
    let mut dest_file = File::create(destination)
        .with_context(|| format!("Failed to create destination file: {}", destination.display()))?;
    
    // Copy the content
    io::copy(&mut source_file, &mut dest_file)
        .with_context(|| format!("Failed to copy file content: {} -> {}", source.display(), destination.display()))?;
    
    Ok(())
}

/// Create temporary directory with a unique name
pub fn create_temp_dir() -> Result<PathBuf> {
    let temp_dir = std::env::temp_dir().join(format!("isotope-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir)
        .with_context(|| format!("Failed to create temporary directory: {}", temp_dir.display()))?;
    
    debug!("Created temporary directory: {}", temp_dir.display());
    Ok(temp_dir)
}

/// Remove directory recursively with proper error handling
pub fn remove_dir_all<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    
    debug!("Removing directory recursively: {}", path.display());
    
    fs::remove_dir_all(path)
        .with_context(|| format!("Failed to remove directory: {}", path.display()))?;
    
    debug!("Directory removed successfully");
    Ok(())
}

/// Check if a path exists
pub fn path_exists<P: AsRef<Path>>(path: P) -> bool {
    path.as_ref().exists()
}

/// Get canonical path
pub fn canonical_path<P: AsRef<Path>>(path: P) -> Result<PathBuf> {
    let path = path.as_ref();
    
    let canonical = fs::canonicalize(path)
        .with_context(|| format!("Failed to get canonical path: {}", path.display()))?;
    
    Ok(canonical)
}