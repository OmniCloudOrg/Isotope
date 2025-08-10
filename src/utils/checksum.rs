use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256, Sha512};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use tracing::{debug, info};

pub struct ChecksumVerifier;

impl ChecksumVerifier {
    pub fn new() -> Self {
        Self
    }

    pub fn verify_file(&self, file_path: &Path, algorithm: &str, expected: &str) -> Result<()> {
        info!("Verifying checksum for: {}", file_path.display());
        debug!("Algorithm: {}, Expected: {}", algorithm, expected);

        let calculated = self.calculate_checksum(file_path, algorithm)?;

        if calculated.to_lowercase() == expected.to_lowercase() {
            info!("✓ Checksum verification passed");
            Ok(())
        } else {
            Err(anyhow!(
                "Checksum mismatch for {}\nExpected: {}\nCalculated: {}",
                file_path.display(),
                expected,
                calculated
            ))
        }
    }

    pub fn calculate_checksum(&self, file_path: &Path, algorithm: &str) -> Result<String> {
        let file = File::open(file_path)
            .with_context(|| format!("Failed to open file: {}", file_path.display()))?;

        let mut reader = BufReader::new(file);
        let mut buffer = vec![0; 8192]; // 8KB buffer

        match algorithm.to_lowercase().as_str() {
            "sha256" => {
                let mut hasher = Sha256::new();
                loop {
                    let bytes_read = reader
                        .read(&mut buffer)
                        .context("Failed to read file data")?;
                    if bytes_read == 0 {
                        break;
                    }
                    hasher.update(&buffer[..bytes_read]);
                }
                Ok(format!("{:x}", hasher.finalize()))
            }
            "sha512" => {
                let mut hasher = Sha512::new();
                loop {
                    let bytes_read = reader
                        .read(&mut buffer)
                        .context("Failed to read file data")?;
                    if bytes_read == 0 {
                        break;
                    }
                    hasher.update(&buffer[..bytes_read]);
                }
                Ok(format!("{:x}", hasher.finalize()))
            }
            "md5" => {
                // MD5 is not recommended for security, but included for compatibility
                #[cfg(feature = "md5")]
                {
                    use md5::{Digest, Md5};
                    let mut hasher = Md5::new();
                    loop {
                        let bytes_read = reader
                            .read(&mut buffer)
                            .context("Failed to read file data")?;
                        if bytes_read == 0 {
                            break;
                        }
                        hasher.update(&buffer[..bytes_read]);
                    }
                    Ok(format!("{:x}", hasher.finalize()))
                }
                #[cfg(not(feature = "md5"))]
                {
                    Err(anyhow!(
                        "MD5 support not enabled. Use sha256 or sha512 instead."
                    ))
                }
            }
            _ => Err(anyhow!("Unsupported checksum algorithm: {}", algorithm)),
        }
    }

    pub fn generate_checksum_file(&self, file_path: &Path, algorithm: &str) -> Result<()> {
        let checksum = self.calculate_checksum(file_path, algorithm)?;
        let checksum_filename = format!(
            "{}.{}",
            file_path.file_name().unwrap().to_string_lossy(),
            algorithm.to_lowercase()
        );
        let checksum_path = file_path.parent().unwrap().join(checksum_filename);

        let content = format!(
            "{}  {}\n",
            checksum,
            file_path.file_name().unwrap().to_string_lossy()
        );
        std::fs::write(&checksum_path, content).with_context(|| {
            format!("Failed to write checksum file: {}", checksum_path.display())
        })?;

        info!("Generated checksum file: {}", checksum_path.display());
        Ok(())
    }

    pub fn verify_checksum_file(&self, checksum_file: &Path) -> Result<()> {
        info!("Verifying checksum file: {}", checksum_file.display());

        let content = std::fs::read_to_string(checksum_file).with_context(|| {
            format!("Failed to read checksum file: {}", checksum_file.display())
        })?;

        let base_dir = checksum_file.parent().unwrap();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let parts: Vec<&str> = line.splitn(2, "  ").collect();
            if parts.len() != 2 {
                return Err(anyhow!("Invalid checksum file format in line: {}", line));
            }

            let expected_checksum = parts[0];
            let filename = parts[1];
            let file_path = base_dir.join(filename);

            if !file_path.exists() {
                return Err(anyhow!("File not found: {}", file_path.display()));
            }

            // Determine algorithm from checksum length
            let algorithm = match expected_checksum.len() {
                32 => "md5",
                64 => "sha256",
                128 => "sha512",
                _ => {
                    return Err(anyhow!(
                        "Cannot determine checksum algorithm from length: {}",
                        expected_checksum.len()
                    ))
                }
            };

            self.verify_file(&file_path, algorithm, expected_checksum)?;
        }

        info!("All checksums verified successfully");
        Ok(())
    }
}
