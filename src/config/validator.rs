use anyhow::{anyhow, Result};
use log::{debug, info};
use std::path::Path;

use crate::config::Stage;

use super::{Instruction, IsotopeSpec, StageType};

pub fn validate_spec(spec: &IsotopeSpec) -> Result<()> {
    // Validate FROM instruction
    if spec.from.is_empty() {
        return Err(anyhow!("FROM instruction is required"));
    }

    // Validate that at least one stage exists
    if spec.stages.is_empty() {
        return Err(anyhow!("At least one stage is required"));
    }

    // Validate each stage
    for stage in &spec.stages {
        validate_stage(stage)?;
    }

    // Validate stage-specific requirements
    validate_stage_requirements(spec)?;

    Ok(())
}

fn validate_stage(stage: &Stage) -> Result<()> {
    match stage.name {
        StageType::Init => validate_init_stage(stage),
        StageType::OsInstall => validate_os_install_stage(stage),
        StageType::OsConfigure => validate_os_configure_stage(stage),
        StageType::Pack => validate_pack_stage(stage),
    }
}

fn validate_init_stage(stage: &Stage) -> Result<()> {
    let mut has_vm_provider = false;
    let mut has_vm_memory = false;

    for instruction in &stage.instructions {
        match instruction {
            Instruction::Vm { key, value } => {
                match key.as_str() {
                    "provider" => {
                        has_vm_provider = true;
                        if !["qemu", "virtualbox", "vmware", "hyperv"].contains(&value.as_str()) {
                            return Err(anyhow!("Invalid VM provider: {}. Supported: qemu, virtualbox, vmware, hyperv", value));
                        }
                    }
                    "memory" => {
                        has_vm_memory = true;
                        if !is_valid_memory_size(value) {
                            return Err(anyhow!("Invalid memory size: {}", value));
                        }
                    }
                    "cpus" => {
                        if value.parse::<u32>().is_err() {
                            return Err(anyhow!("Invalid CPU count: {}", value));
                        }
                    }
                    "disk" => {
                        if !is_valid_disk_size(value) {
                            return Err(anyhow!("Invalid disk size: {}", value));
                        }
                    }
                    "boot-wait" => {
                        if !is_valid_duration(value) {
                            return Err(anyhow!("Invalid boot-wait duration: {}", value));
                        }
                    }
                    "timeout" => {
                        if !is_valid_duration(value) {
                            return Err(anyhow!("Invalid timeout duration: {}", value));
                        }
                    }
                    _ => {} // Allow other VM parameters
                }
            }
            _ => {
                return Err(anyhow!("Invalid instruction in init stage: {:?}", instruction));
            }
        }
    }

    if !has_vm_provider {
        return Err(anyhow!("VM provider is required in init stage"));
    }

    if !has_vm_memory {
        return Err(anyhow!("VM memory is required in init stage"));
    }

    Ok(())
}

fn validate_os_install_stage(stage: &Stage) -> Result<()> {
    for instruction in &stage.instructions {
        match instruction {
            Instruction::Wait { duration, .. } => {
                if !is_valid_duration(duration) {
                    return Err(anyhow!("Invalid wait duration: {}", duration));
                }
            }
            Instruction::Press { key, .. } => {
                if key.is_empty() {
                    return Err(anyhow!("Press instruction requires a key"));
                }
            }
            Instruction::Type { text } => {
                if text.is_empty() {
                    return Err(anyhow!("Type instruction requires text"));
                }
            }
            _ => {
                return Err(anyhow!("Invalid instruction in os_install stage: {:?}", instruction));
            }
        }
    }

    Ok(())
}

fn validate_os_configure_stage(stage: &Stage) -> Result<()> {
    for instruction in &stage.instructions {
        match instruction {
            Instruction::Run { command } => {
                if command.is_empty() {
                    return Err(anyhow!("Run instruction requires a command"));
                }
            }
            Instruction::Copy { from, to } => {
                if !from.exists() {
                    return Err(anyhow!("Copy source file does not exist: {}", from.display()));
                }
                if to.to_string_lossy().is_empty() {
                    return Err(anyhow!("Copy destination cannot be empty"));
                }
            }
            Instruction::Wait { duration, .. } => {
                if !is_valid_duration(duration) {
                    return Err(anyhow!("Invalid wait duration: {}", duration));
                }
            }
            Instruction::Press { key, .. } => {
                if key.is_empty() {
                    return Err(anyhow!("Press instruction requires a key"));
                }
            }
            Instruction::Type { text } => {
                if text.is_empty() {
                    return Err(anyhow!("Type instruction requires text"));
                }
            }
            _ => {
                return Err(anyhow!("Invalid instruction in os_configure stage: {:?}", instruction));
            }
        }
    }

    Ok(())
}

fn validate_pack_stage(stage: &Stage) -> Result<()> {
    let mut has_export = false;

    for instruction in &stage.instructions {
        match instruction {
            Instruction::Export { path } => {
                has_export = true;
                if path.to_string_lossy().is_empty() {
                    return Err(anyhow!("Export path cannot be empty"));
                }
            }
            Instruction::Format { format } => {
                if !["iso9660", "udf"].contains(&format.as_str()) {
                    return Err(anyhow!("Invalid format: {}. Supported: iso9660, udf", format));
                }
            }
            Instruction::Bootable { .. } => {} // Always valid
            Instruction::VolumeLabel { label } => {
                if label.is_empty() {
                    return Err(anyhow!("Volume label cannot be empty"));
                }
                if label.len() > 32 {
                    return Err(anyhow!("Volume label too long (max 32 characters)"));
                }
            }
            _ => {
                return Err(anyhow!("Invalid instruction in pack stage: {:?}", instruction));
            }
        }
    }

    if !has_export {
        return Err(anyhow!("Pack stage requires an EXPORT instruction"));
    }

    Ok(())
}

fn validate_stage_requirements(spec: &IsotopeSpec) -> Result<()> {
    let mut has_init = false;
    let mut has_pack = false;

    for stage in &spec.stages {
        match stage.name {
            StageType::Init => has_init = true,
            StageType::Pack => has_pack = true,
            _ => {}
        }
    }

    if !has_init {
        return Err(anyhow!("init stage is required"));
    }

    if !has_pack {
        return Err(anyhow!("pack stage is required"));
    }

    Ok(())
}

fn is_valid_memory_size(size: &str) -> bool {
    let size_lower = size.to_lowercase();
    if size_lower.ends_with("m") || size_lower.ends_with("mb") {
        if let Ok(num) = size_lower.trim_end_matches("mb").trim_end_matches("m").parse::<u64>() {
            return num >= 512 && num <= 65536; // 512MB to 64GB
        }
    }
    if size_lower.ends_with("g") || size_lower.ends_with("gb") {
        if let Ok(num) = size_lower.trim_end_matches("gb").trim_end_matches("g").parse::<u64>() {
            return num >= 1 && num <= 64; // 1GB to 64GB
        }
    }
    false
}

fn is_valid_disk_size(size: &str) -> bool {
    let size_lower = size.to_lowercase();
    if size_lower.ends_with("g") || size_lower.ends_with("gb") {
        if let Ok(num) = size_lower.trim_end_matches("gb").trim_end_matches("g").parse::<u64>() {
            return num >= 1 && num <= 1024; // 1GB to 1TB
        }
    }
    if size_lower.ends_with("t") || size_lower.ends_with("tb") {
        if let Ok(num) = size_lower.trim_end_matches("tb").trim_end_matches("t").parse::<u64>() {
            return num >= 1 && num <= 10; // 1TB to 10TB
        }
    }
    false
}

fn is_valid_duration(duration: &str) -> bool {
    let duration_lower = duration.to_lowercase();

    // Check longer suffixes first to avoid conflicts (ms before s)
    if duration_lower.ends_with("ms") {
        return duration_lower.trim_end_matches("ms").parse::<u64>().is_ok();
    }
    if duration_lower.ends_with("s") {
        return duration_lower.trim_end_matches("s").parse::<u64>().is_ok();
    }
    if duration_lower.ends_with("m") {
        return duration_lower.trim_end_matches("m").parse::<u64>().is_ok();
    }
    if duration_lower.ends_with("h") {
        return duration_lower.trim_end_matches("h").parse::<u64>().is_ok();
    }
    false
}