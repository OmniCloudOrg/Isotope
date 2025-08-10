use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::PathBuf;

use super::{ChecksumInfo, Instruction, IsotopeSpec, Stage, StageType};

pub fn parse_isotope_spec(content: &str) -> Result<IsotopeSpec> {
    let mut lines = content.lines().enumerate().peekable();
    let mut from = String::new();
    let mut checksum = None;
    let mut labels = HashMap::new();
    let mut stages = Vec::new();
    let mut current_stage: Option<Stage> = None;

    while let Some((line_num, line)) = lines.next() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        let instruction = parts[0];
        let args = if parts.len() > 1 { parts[1] } else { "" };

        match instruction {
            "FROM" => {
                from = args.to_string();
            }
            "CHECKSUM" => {
                let checksum_parts: Vec<&str> = args.splitn(2, ':').collect();
                if checksum_parts.len() != 2 {
                    return Err(anyhow!(
                        "Line {}: Invalid CHECKSUM format. Expected 'algorithm:value'",
                        line_num + 1
                    ));
                }
                checksum = Some(ChecksumInfo {
                    algorithm: checksum_parts[0].to_string(),
                    value: checksum_parts[1].to_string(),
                });
            }
            "LABEL" => {
                let label_parts: Vec<&str> = args.splitn(2, '=').collect();
                if label_parts.len() != 2 {
                    return Err(anyhow!(
                        "Line {}: Invalid LABEL format. Expected 'key=value'",
                        line_num + 1
                    ));
                }
                labels.insert(
                    label_parts[0].to_string(),
                    label_parts[1].trim_matches('"').to_string(),
                );
            }
            "STAGE" => {
                // Save previous stage if exists
                if let Some(stage) = current_stage.take() {
                    stages.push(stage);
                }

                let stage_type = match args {
                    "init" => StageType::Init,
                    "os_install" => StageType::OsInstall,
                    "os_configure" => StageType::OsConfigure,
                    "pack" => StageType::Pack,
                    _ => {
                        return Err(anyhow!(
                            "Line {}: Unknown stage type '{}'",
                            line_num + 1,
                            args
                        ))
                    }
                };

                current_stage = Some(Stage {
                    name: stage_type,
                    instructions: Vec::new(),
                });
            }
            _ => {
                // Parse stage-specific instructions
                if let Some(ref mut stage) = current_stage {
                    let instruction = parse_stage_instruction(instruction, args, line_num + 1)?;
                    stage.instructions.push(instruction);
                } else {
                    return Err(anyhow!(
                        "Line {}: Instruction '{}' found outside of stage",
                        line_num + 1,
                        instruction
                    ));
                }
            }
        }
    }

    // Save the last stage
    if let Some(stage) = current_stage {
        stages.push(stage);
    }

    if from.is_empty() {
        return Err(anyhow!("Missing FROM instruction"));
    }

    Ok(IsotopeSpec {
        from,
        checksum,
        labels,
        stages,
    })
}

fn parse_stage_instruction(instruction: &str, args: &str, line_num: usize) -> Result<Instruction> {
    match instruction {
        // VM Configuration
        "VM" => {
            let vm_parts: Vec<&str> = args.splitn(2, '=').collect();
            if vm_parts.len() != 2 {
                return Err(anyhow!(
                    "Line {}: Invalid VM format. Expected 'key=value'",
                    line_num
                ));
            }
            Ok(Instruction::Vm {
                key: vm_parts[0].to_string(),
                value: vm_parts[1].to_string(),
            })
        }

        // OS Installation
        "WAIT" => {
            if args.contains(" FOR ") {
                let wait_parts: Vec<&str> = args.splitn(2, " FOR ").collect();
                let mut condition_text = wait_parts[1].trim();

                // Strip comments first
                if let Some(comment_pos) = condition_text.find('#') {
                    condition_text = condition_text[..comment_pos].trim();
                }

                // Then strip quotes from the cleaned text
                condition_text = condition_text.trim_matches('"');

                Ok(Instruction::Wait {
                    duration: wait_parts[0].to_string(),
                    condition: Some(condition_text.to_string()),
                })
            } else {
                Ok(Instruction::Wait {
                    duration: args.to_string(),
                    condition: None,
                })
            }
        }
        "PRESS" => {
            let mut parts = args.split_whitespace();
            let key_or_combo = parts.next().unwrap_or("").to_string();
            let mut repeat = None;
            let mut modifiers: Option<Vec<String>> = None;

            // Check if this is a key combination (e.g., "ctrl+alt+t")
            if key_or_combo.contains('+') {
                let combo_parts: Vec<&str> = key_or_combo.split('+').collect();
                if combo_parts.len() >= 2 {
                    // Last part is the key, others are modifiers
                    let key = combo_parts.last().unwrap();
                    let modifier_parts = &combo_parts[..combo_parts.len() - 1];

                    // Validate that all modifier parts are known modifiers
                    let mut valid_modifiers = Vec::new();
                    for modifier in modifier_parts {
                        let modifier_lower = modifier.to_lowercase();
                        if matches!(
                            modifier_lower.as_str(),
                            "ctrl"
                                | "control"
                                | "alt"
                                | "shift"
                                | "meta"
                                | "cmd"
                                | "win"
                                | "windows"
                        ) {
                            valid_modifiers.push(modifier_lower);
                        } else {
                            // Invalid modifier, treat as regular key
                            break;
                        }
                    }

                    // If all parts are valid modifiers, treat as key combination
                    if valid_modifiers.len() == modifier_parts.len() {
                        // Check for repeat count
                        if let Some(next) = parts.next() {
                            if next == "repeat" || next == "x" {
                                if let Some(count_str) = parts.next() {
                                    repeat = count_str.parse().ok();
                                }
                            }
                        }

                        return Ok(Instruction::Press {
                            key: key.to_string(),
                            repeat,
                            modifiers: Some(valid_modifiers),
                        });
                    }
                }
            }

            // Regular key press - check for repeat count
            if let Some(next) = parts.next() {
                if next == "repeat" || next == "x" {
                    if let Some(count_str) = parts.next() {
                        repeat = count_str.parse().ok();
                    }
                }
            }

            Ok(Instruction::Press {
                key: key_or_combo,
                repeat,
                modifiers: None,
            })
        }
        "TYPE" => Ok(Instruction::Type {
            text: args.trim_matches('"').to_string(),
        }),

        // OS Configuration
        "RUN" => Ok(Instruction::Run {
            command: args.to_string(),
        }),
        "COPY" => {
            let copy_parts: Vec<&str> = args.splitn(2, ' ').collect();
            if copy_parts.len() != 2 {
                return Err(anyhow!(
                    "Line {}: Invalid COPY format. Expected 'source destination'",
                    line_num
                ));
            }
            Ok(Instruction::Copy {
                from: PathBuf::from(copy_parts[0]),
                to: PathBuf::from(copy_parts[1]),
            })
        }
        // SSH Login
        "LOGIN" => {
            // Example: LOGIN root password=mypassword
            let mut username = String::new();
            let mut password = None;
            let mut private_key = None;
            let mut parts = args.split_whitespace();
            if let Some(user) = parts.next() {
                username = user.to_string();
            }
            for part in parts {
                if let Some((k, v)) = part.split_once('=') {
                    match k {
                        "password" => password = Some(v.to_string()),
                        "private_key" => private_key = Some(PathBuf::from(v)),
                        _ => {}
                    }
                }
            }
            Ok(Instruction::Login {
                username,
                password,
                private_key,
            })
        }
        // Packaging
        "EXPORT" => Ok(Instruction::Export {
            path: PathBuf::from(args),
        }),
        "FORMAT" => Ok(Instruction::Format {
            format: args.to_string(),
        }),
        "BOOTABLE" => {
            let enabled = match args.to_lowercase().as_str() {
                "true" | "yes" | "1" => true,
                "false" | "no" | "0" => false,
                _ => {
                    return Err(anyhow!(
                        "Line {}: Invalid BOOTABLE value. Expected true/false",
                        line_num
                    ))
                }
            };
            Ok(Instruction::Bootable { enabled })
        }
        "VOLUME_LABEL" => Ok(Instruction::VolumeLabel {
            label: args.trim_matches('"').to_string(),
        }),
        _ => Err(anyhow!(
            "Line {}: Unknown instruction '{}'",
            line_num,
            instruction
        )),
    }
}
