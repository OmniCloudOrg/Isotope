use anyhow::Result;
use std::path::Path;
use tracing::info;

pub struct IsoTester {
    working_dir: std::path::PathBuf,
}

impl IsoTester {
    pub fn new(working_dir: std::path::PathBuf) -> Self {
        Self { working_dir }
    }

    pub async fn test_iso(&self, iso_path: &Path) -> Result<()> {
        info!("Testing ISO: {}", iso_path.display());

        // Implementation would test the ISO by booting it in a VM
        // and verifying it works as expected

        Ok(())
    }
}
