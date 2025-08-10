pub mod parser;
pub mod validator;
pub mod converter;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsotopeSpec {
    pub from: String,
    pub checksum: Option<ChecksumInfo>,
    pub labels: HashMap<String, String>,
    pub stages: Vec<Stage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecksumInfo {
    pub algorithm: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stage {
    pub name: StageType,
    pub instructions: Vec<Instruction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StageType {
    Init,
    OsInstall,
    OsConfigure,
    Pack,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Instruction {
    // VM Configuration (init stage)
    Vm { key: String, value: String },
    
    // OS Installation (os_install stage)
    Wait { duration: String, condition: Option<String> },
    Press { key: String, repeat: Option<u32>, modifiers: Option<Vec<String>> },
    Type { text: String },
    
    // OS Configuration (os_configure stage) 
    Run { command: String },
    Copy { from: PathBuf, to: PathBuf },
    // SSH login configuration for remote operations  
    Login { username: String, password: Option<String>, private_key: Option<PathBuf> },
    
    // Packaging (pack stage)
    Export { path: PathBuf },
    Format { format: String },
    Bootable { enabled: bool },
    VolumeLabel { label: String },
}

impl IsotopeSpec {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read file: {}", path.as_ref().display()))?;
        
        parser::parse_isotope_spec(&content)
            .with_context(|| format!("Failed to parse Isotope spec: {}", path.as_ref().display()))
    }

    pub fn validate(&self) -> Result<()> {
        validator::validate_spec(self)
    }

    pub fn get_stage(&self, stage_type: &StageType) -> Option<&Stage> {
        self.stages.iter().find(|s| std::mem::discriminant(&s.name) == std::mem::discriminant(stage_type))
    }

    pub fn get_label(&self, key: &str) -> Option<&String> {
        self.labels.get(key)
    }
}