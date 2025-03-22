use anyhow::{Context, Result};
use log::{debug, info};
use sha2::{Sha256, Digest};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// Verify the checksum of a file
pub fn verify_checksum<P: AsRef<Path>>(file_path: P, algo: &str, expected: &str) -> Result<()> {
    let file_path = file_path.as_ref();
    debug!("Verifying {} checksum of file: {}", algo, file_path.display());
    
    let calculated = match algo.to_lowercase().as_str() {
        "sha256" => calculate_sha256(file_path)?,
        "md5" => {
            // Placeholder for MD5 calculation
            // In a real implementation, we would use the md5 crate
            return Err(anyhow::anyhow!("MD5 checksum calculation not implemented"));
        },
        _ => {
            return Err(anyhow::anyhow!("Unsupported checksum algorithm: {}", algo));
        }
    };
    
    debug!("Calculated checksum: {}", calculated);
    debug!("Expected checksum:   {}", expected);
    
    if calculated.to_lowercase() == expected.to_lowercase() {
        info!("Checksum verification passed");
        Ok(())
    } else {
        Err(anyhow::anyhow!("Checksum verification failed: expected {}, got {}", expected, calculated))
    }
}

/// Calculate SHA-256 checksum of a file
fn calculate_sha256<P: AsRef<Path>>(file_path: P) -> Result<String> {
    let file_path = file_path.as_ref();
    let mut file = File::open(file_path)
        .with_context(|| format!("Failed to open file for checksum calculation: {}", file_path.display()))?;
    
    let mut hasher = Sha256::new();
    let mut buffer = [0; 1024 * 1024]; // 1 MB buffer
    
    loop {
        let bytes_read = file.read(&mut buffer)
            .with_context(|| format!("Failed to read file: {}", file_path.display()))?;
        
        if bytes_read == 0 {
            break;
        }
        
        hasher.update(&buffer[..bytes_read]);
    }
    
    let result = hasher.finalize();
    Ok(format!("{:x}", result))
}