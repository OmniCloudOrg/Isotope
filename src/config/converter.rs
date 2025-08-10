use anyhow::{Context, Result};
use serde_json::Value;
use std::fs;
use std::path::Path;

pub fn convert_json_to_isotope(input_path: &Path, output_path: &Path) -> Result<()> {
    let json_content = fs::read_to_string(input_path)
        .with_context(|| format!("Failed to read JSON file: {}", input_path.display()))?;

    let json_value: Value =
        serde_json::from_str(&json_content).with_context(|| "Failed to parse JSON content")?;

    let isotope_content = convert_json_value_to_isotope(&json_value)?;

    fs::write(output_path, isotope_content)
        .with_context(|| format!("Failed to write Isotope file: {}", output_path.display()))?;

    Ok(())
}

fn convert_json_value_to_isotope(json: &Value) -> Result<String> {
    let mut isotope_lines = Vec::new();

    // Extract FROM instruction
    if let Some(source) = json.get("source") {
        if let Some(path) = source.get("path").and_then(|v| v.as_str()) {
            isotope_lines.push(format!("FROM {path}"));
        }

        if let Some(checksum) = source.get("checksum") {
            if let (Some(typ), Some(value)) = (
                checksum.get("type").and_then(|v| v.as_str()),
                checksum.get("value").and_then(|v| v.as_str()),
            ) {
                isotope_lines.push(format!("CHECKSUM {typ}:{value}"));
            }
        }
    }

    // Extract project metadata as labels
    if let Some(project) = json.get("project") {
        if let Some(name) = project.get("name").and_then(|v| v.as_str()) {
            isotope_lines.push(format!("LABEL name=\"{name}\""));
        }
        if let Some(version) = project.get("version").and_then(|v| v.as_str()) {
            isotope_lines.push(format!("LABEL version=\"{version}\""));
        }
        if let Some(description) = project.get("description").and_then(|v| v.as_str()) {
            isotope_lines.push(format!("LABEL description=\"{description}\""));
        }
    }

    isotope_lines.push("".to_string()); // Empty line

    // Convert test VM configuration to init stage
    if let Some(test) = json.get("test") {
        isotope_lines.push("STAGE init".to_string());

        if let Some(vm) = test.get("vm") {
            if let Some(provider) = vm.get("provider").and_then(|v| v.as_str()) {
                isotope_lines.push(format!("VM provider={provider}"));
            }
            if let Some(memory) = vm.get("memory").and_then(|v| v.as_str()) {
                isotope_lines.push(format!("VM memory={memory}"));
            }
            if let Some(cpus) = vm.get("cpus").and_then(|v| v.as_u64()) {
                isotope_lines.push(format!("VM cpus={cpus}"));
            }
        }

        if let Some(boot_wait) = test.get("boot_wait").and_then(|v| v.as_str()) {
            isotope_lines.push(format!("VM boot-wait={boot_wait}"));
        }

        isotope_lines.push("".to_string());
    }

    // Convert GUI installation to os_install stage
    if let Some(gui) = json.get("gui_installation") {
        if let Some(interactive) = gui
            .get("interactive_installation")
            .and_then(|v| v.as_array())
        {
            isotope_lines.push("STAGE os_install".to_string());

            for step in interactive {
                if let Some(description) = step.get("description").and_then(|v| v.as_str()) {
                    isotope_lines.push(format!("# {description}"));
                }

                if let Some(detection) = step.get("detection") {
                    if let Some(timeout) =
                        detection.get("wait_for_timeout").and_then(|v| v.as_str())
                    {
                        if let Some(pattern) =
                            detection.get("success_pattern").and_then(|v| v.as_str())
                        {
                            isotope_lines.push(format!("WAIT {timeout} FOR \"{pattern}\""));
                        } else {
                            isotope_lines.push(format!("WAIT {timeout}"));
                        }
                    }
                }

                if let Some(keypresses) = step.get("keypress_sequence").and_then(|v| v.as_array()) {
                    for keypress in keypresses {
                        if let Some(wait) = keypress.get("wait").and_then(|v| v.as_str()) {
                            isotope_lines.push(format!("WAIT {wait}"));
                        } else if let Some(key) = keypress.get("key").and_then(|v| v.as_str()) {
                            if let Some(repeat) = keypress.get("repeat").and_then(|v| v.as_u64()) {
                                isotope_lines.push(format!("PRESS {key} repeat {repeat}"));
                            } else {
                                isotope_lines.push(format!("PRESS {key}"));
                            }
                        } else if let Some(text) = keypress.get("key_text").and_then(|v| v.as_str())
                        {
                            isotope_lines.push(format!("TYPE \"{text}\""));
                        }
                    }
                }

                isotope_lines.push("".to_string());
            }
        }
    }

    // Convert provisioning to os_configure stage
    if let Some(test) = json.get("test") {
        if let Some(provision) = test.get("provision").and_then(|v| v.as_array()) {
            isotope_lines.push("STAGE os_configure".to_string());

            for step in provision {
                if let Some(script) = step.get("script").and_then(|v| v.as_str()) {
                    isotope_lines.push(format!("RUN bash {script}"));
                } else if let Some(inline) = step.get("inline").and_then(|v| v.as_array()) {
                    for command in inline {
                        if let Some(cmd) = command.as_str() {
                            isotope_lines.push(format!("RUN {cmd}"));
                        }
                    }
                }
            }

            isotope_lines.push("".to_string());
        }
    }

    // Convert modifications to COPY instructions in os_configure stage (if not already present)
    if let Some(modifications) = json.get("modifications").and_then(|v| v.as_array()) {
        let has_configure_stage = isotope_lines
            .iter()
            .any(|line| line == "STAGE os_configure");

        if !has_configure_stage {
            isotope_lines.push("STAGE os_configure".to_string());
        }

        for modification in modifications {
            if let Some(mod_type) = modification.get("type").and_then(|v| v.as_str()) {
                match mod_type {
                    "file_add" | "directory_add" => {
                        if let (Some(source), Some(destination)) = (
                            modification.get("source").and_then(|v| v.as_str()),
                            modification.get("destination").and_then(|v| v.as_str()),
                        ) {
                            isotope_lines.push(format!("COPY {source} {destination}"));
                        }
                    }
                    _ => {} // Skip other modification types for now
                }
            }
        }

        isotope_lines.push("".to_string());
    }

    // Convert output to pack stage
    if let Some(output) = json.get("output") {
        isotope_lines.push("STAGE pack".to_string());

        if let Some(path) = output.get("path").and_then(|v| v.as_str()) {
            isotope_lines.push(format!("EXPORT {path}"));
        }

        if let Some(format) = output.get("format").and_then(|v| v.as_str()) {
            isotope_lines.push(format!("FORMAT {format}"));
        }

        if let Some(options) = output.get("options") {
            if let Some(bootable) = options.get("bootable").and_then(|v| v.as_bool()) {
                isotope_lines.push(format!("BOOTABLE {bootable}"));
            }
        }
    }

    Ok(isotope_lines.join("\n"))
}
