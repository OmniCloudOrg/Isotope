use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

pub struct FileSystemManager {
    working_dir: PathBuf,
}

impl FileSystemManager {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }

    pub fn create_working_directory(&self) -> Result<()> {
        info!("Creating working directory: {}", self.working_dir.display());

        if self.working_dir.exists() {
            warn!("Working directory already exists, cleaning up first");
            self.cleanup()?;
        }

        std::fs::create_dir_all(&self.working_dir).with_context(|| {
            format!(
                "Failed to create working directory: {}",
                self.working_dir.display()
            )
        })?;

        Ok(())
    }

    pub fn cleanup(&self) -> Result<()> {
        if self.working_dir.exists() {
            info!(
                "Cleaning up working directory: {}",
                self.working_dir.display()
            );

            // On Windows, files might be locked, so we try multiple times
            let mut attempts = 0;
            let max_attempts = 3;

            loop {
                match std::fs::remove_dir_all(&self.working_dir) {
                    Ok(()) => {
                        debug!("Successfully cleaned up working directory");
                        break;
                    }
                    Err(e) => {
                        attempts += 1;
                        if attempts >= max_attempts {
                            return Err(anyhow::anyhow!(
                                "Failed to cleanup working directory after {} attempts: {}",
                                max_attempts,
                                e
                            ));
                        }
                        warn!("Cleanup attempt {} failed, retrying: {}", attempts, e);
                        std::thread::sleep(std::time::Duration::from_millis(1000));
                    }
                }
            }
        }

        Ok(())
    }

    pub fn get_working_dir(&self) -> &Path {
        &self.working_dir
    }

    pub fn create_subdirectory(&self, name: &str) -> Result<PathBuf> {
        let subdir = self.working_dir.join(name);
        std::fs::create_dir_all(&subdir)
            .with_context(|| format!("Failed to create subdirectory: {}", subdir.display()))?;
        Ok(subdir)
    }

    pub fn copy_file(&self, from: &Path, to: &Path) -> Result<()> {
        debug!("Copying file: {} -> {}", from.display(), to.display());

        if !from.exists() {
            return Err(anyhow::anyhow!(
                "Source file does not exist: {}",
                from.display()
            ));
        }

        // Create parent directory if it doesn't exist
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create parent directory: {}", parent.display())
            })?;
        }

        std::fs::copy(from, to)
            .with_context(|| format!("Failed to copy {} to {}", from.display(), to.display()))?;

        Ok(())
    }

    pub fn copy_directory(&self, from: &Path, to: &Path) -> Result<()> {
        info!("Copying directory: {} -> {}", from.display(), to.display());

        if !from.exists() {
            return Err(anyhow::anyhow!(
                "Source directory does not exist: {}",
                from.display()
            ));
        }

        self.copy_dir_recursive(from, to)
    }

    fn copy_dir_recursive(&self, from: &Path, to: &Path) -> Result<()> {
        std::fs::create_dir_all(to)
            .with_context(|| format!("Failed to create directory: {}", to.display()))?;

        for entry in std::fs::read_dir(from)
            .with_context(|| format!("Failed to read directory: {}", from.display()))?
        {
            let entry = entry?;
            let entry_type = entry.file_type()?;
            let source_path = entry.path();
            let dest_path = to.join(entry.file_name());

            if entry_type.is_dir() {
                self.copy_dir_recursive(&source_path, &dest_path)?;
            } else {
                self.copy_file(&source_path, &dest_path)?;
            }
        }

        Ok(())
    }

    pub fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        debug!("Writing file: {}", path.display());

        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create parent directory: {}", parent.display())
            })?;
        }

        std::fs::write(path, content)
            .with_context(|| format!("Failed to write file: {}", path.display()))?;

        Ok(())
    }

    pub fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        debug!("Reading file: {}", path.display());

        std::fs::read(path).with_context(|| format!("Failed to read file: {}", path.display()))
    }

    pub fn file_exists(&self, path: &Path) -> bool {
        path.exists() && path.is_file()
    }

    pub fn directory_exists(&self, path: &Path) -> bool {
        path.exists() && path.is_dir()
    }

    pub fn get_file_size(&self, path: &Path) -> Result<u64> {
        let metadata = std::fs::metadata(path)
            .with_context(|| format!("Failed to get metadata for: {}", path.display()))?;
        Ok(metadata.len())
    }

    pub fn make_executable(&self, path: &Path) -> Result<()> {
        debug!("Making file executable: {}", path.display());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(path)?;
            let mut permissions = metadata.permissions();
            // Add execute permissions for owner, group, and others
            permissions.set_mode(permissions.mode() | 0o111);
            std::fs::set_permissions(path, permissions)
                .with_context(|| format!("Failed to set permissions for: {}", path.display()))?;
        }

        #[cfg(windows)]
        {
            // On Windows, .exe and .bat files are inherently executable
            // No additional action needed
            debug!(
                "File permissions not modified on Windows: {}",
                path.display()
            );
        }

        Ok(())
    }

    pub fn get_temp_path(&self, name: &str) -> PathBuf {
        self.working_dir.join(name)
    }

    pub fn ensure_directory_empty(&self, path: &Path) -> Result<()> {
        if path.exists() {
            debug!("Clearing directory: {}", path.display());

            for entry in std::fs::read_dir(path)? {
                let entry = entry?;
                let entry_path = entry.path();

                if entry_path.is_dir() {
                    std::fs::remove_dir_all(&entry_path).with_context(|| {
                        format!("Failed to remove directory: {}", entry_path.display())
                    })?;
                } else {
                    std::fs::remove_file(&entry_path).with_context(|| {
                        format!("Failed to remove file: {}", entry_path.display())
                    })?;
                }
            }
        } else {
            std::fs::create_dir_all(path)
                .with_context(|| format!("Failed to create directory: {}", path.display()))?;
        }

        Ok(())
    }
}
