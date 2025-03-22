use serde::{Deserialize, Serialize};
use std::fmt;

/// Keypress sequence for VM automation
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KeypressSequence {
    /// Optional wait time (e.g., "5s", "500ms", "2m")
    pub wait: Option<String>,
    /// Key to press (e.g., "enter", "tab", "esc")
    pub key: Option<String>,
    /// Text to type
    pub key_text: Option<String>,
    /// Command to execute (e.g., full commands or configurations)
    pub key_command: Option<String>,
    /// Number of times to repeat the key press
    pub repeat: Option<u32>,
    /// Description of what this keypress sequence does
    pub description: Option<String>,
}

impl fmt::Display for KeypressSequence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(desc) = &self.description {
            return write!(f, "{}", desc);
        }
        
        let mut parts = Vec::new();
        
        if let Some(wait) = &self.wait {
            parts.push(format!("wait {}ms", wait));
        }
        
        if let Some(key) = &self.key {
            let repeat = self.repeat.unwrap_or(1);
            if repeat > 1 {
                parts.push(format!("press {} x{}", key, repeat));
            } else {
                parts.push(format!("press {}", key));
            }
        }
        
        if let Some(text) = &self.key_text {
            parts.push(format!("type \"{}\"", text));
        }
        
        if let Some(cmd) = &self.key_command {
            parts.push(format!("execute \"{}\"", cmd));
        }
        
        if parts.is_empty() {
            write!(f, "empty sequence")
        } else {
            write!(f, "{}", parts.join(", "))
        }
    }
}

/// Map special key names to their values (for different automation systems)
pub fn map_key_name(key_name: &str, target_system: &str) -> String {
    match target_system {
        "qemu" => map_key_name_qemu(key_name),
        "virtualbox" => map_key_name_virtualbox(key_name),
        "vmware" => map_key_name_vmware(key_name),
        _ => key_name.to_string(),
    }
}

/// Map special key names to QEMU HMP/QMP commands
fn map_key_name_qemu(key_name: &str) -> String {
    match key_name.to_lowercase().as_str() {
        "enter" => "ret".to_string(),
        "return" => "ret".to_string(),
        "tab" => "tab".to_string(),
        "esc" => "esc".to_string(),
        "escape" => "esc".to_string(),
        "space" => "spc".to_string(),
        "backspace" => "backspace".to_string(),
        "delete" => "delete".to_string(),
        "up" => "up".to_string(),
        "down" => "down".to_string(),
        "left" => "left".to_string(),
        "right" => "right".to_string(),
        "f1" => "f1".to_string(),
        "f2" => "f2".to_string(),
        "f3" => "f3".to_string(),
        "f4" => "f4".to_string(),
        "f5" => "f5".to_string(),
        "f6" => "f6".to_string(),
        "f7" => "f7".to_string(),
        "f8" => "f8".to_string(),
        "f9" => "f9".to_string(),
        "f10" => "f10".to_string(),
        "f11" => "f11".to_string(),
        "f12" => "f12".to_string(),
        "home" => "home".to_string(),
        "end" => "end".to_string(),
        "pageup" => "pgup".to_string(),
        "pagedown" => "pgdn".to_string(),
        "win" => "meta_l".to_string(),
        "alt" => "alt".to_string(),
        "ctrl" => "ctrl".to_string(),
        "shift" => "shift".to_string(),
        "capslock" => "caps_lock".to_string(),
        "numlock" => "num_lock".to_string(),
        _ => key_name.to_string(),
    }
}

/// Map special key names to VirtualBox scancodes
fn map_key_name_virtualbox(key_name: &str) -> String {
    match key_name.to_lowercase().as_str() {
        "enter" => "1c 9c".to_string(),
        "return" => "1c 9c".to_string(),
        "tab" => "0f 8f".to_string(),
        "esc" => "01 81".to_string(),
        "escape" => "01 81".to_string(),
        "space" => "39 b9".to_string(),
        "backspace" => "0e 8e".to_string(),
        "delete" => "53 d3".to_string(),
        "up" => "48 c8".to_string(),
        "down" => "50 d0".to_string(),
        "left" => "4b cb".to_string(),
        "right" => "4d cd".to_string(),
        "f1" => "3b bb".to_string(),
        "f2" => "3c bc".to_string(),
        "f3" => "3d bd".to_string(),
        "f4" => "3e be".to_string(),
        "f5" => "3f bf".to_string(),
        "f6" => "40 c0".to_string(),
        "f7" => "41 c1".to_string(),
        "f8" => "42 c2".to_string(),
        "f9" => "43 c3".to_string(),
        "f10" => "44 c4".to_string(),
        "f11" => "57 d7".to_string(),
        "f12" => "58 d8".to_string(),
        "home" => "47 c7".to_string(),
        "end" => "4f cf".to_string(),
        "pageup" => "49 c9".to_string(),
        "pagedown" => "51 d1".to_string(),
        "win" => "5b db".to_string(),
        "alt" => "38 b8".to_string(),
        "ctrl" => "1d 9d".to_string(),
        "shift" => "2a aa".to_string(),
        "capslock" => "3a ba".to_string(),
        "numlock" => "45 c5".to_string(),
        _ => {
            // For regular keys, map to ASCII
            if key_name.len() == 1 {
                let c = key_name.chars().next().unwrap();
                match c {
                    'a'..='z' => {
                        let scancode = 0x1e + (c as u8 - b'a');
                        format!("{:02x} {:02x}", scancode, scancode + 0x80)
                    },
                    'A'..='Z' => {
                        let scancode = 0x1e + (c.to_lowercase().next().unwrap() as u8 - b'a');
                        format!("2a {:02x} {:02x} aa", scancode, scancode + 0x80)
                    },
                    '0'..='9' => {
                        let scancode = if c == '0' { 0x0b } else { 0x02 + (c as u8 - b'1') };
                        format!("{:02x} {:02x}", scancode, scancode + 0x80)
                    },
                    _ => key_name.to_string(),
                }
            } else {
                key_name.to_string()
            }
        }
    }
}

/// Map special key names to VMware keycodes
fn map_key_name_vmware(key_name: &str) -> String {
    match key_name.to_lowercase().as_str() {
        "enter" => "0x0d".to_string(),
        "return" => "0x0d".to_string(),
        "tab" => "0x09".to_string(),
        "esc" => "0x1b".to_string(),
        "escape" => "0x1b".to_string(),
        "space" => "0x20".to_string(),
        "backspace" => "0x08".to_string(),
        "delete" => "0x7f".to_string(),
        "up" => "0x26".to_string(),
        "down" => "0x28".to_string(),
        "left" => "0x25".to_string(),
        "right" => "0x27".to_string(),
        "f1" => "0x70".to_string(),
        "f2" => "0x71".to_string(),
        "f3" => "0x72".to_string(),
        "f4" => "0x73".to_string(),
        "f5" => "0x74".to_string(),
        "f6" => "0x75".to_string(),
        "f7" => "0x76".to_string(),
        "f8" => "0x77".to_string(),
        "f9" => "0x78".to_string(),
        "f10" => "0x79".to_string(),
        "f11" => "0x7a".to_string(),
        "f12" => "0x7b".to_string(),
        "home" => "0x24".to_string(),
        "end" => "0x23".to_string(),
        "pageup" => "0x21".to_string(),
        "pagedown" => "0x22".to_string(),
        "win" => "0x5b".to_string(),
        "alt" => "0x12".to_string(),
        "ctrl" => "0x11".to_string(),
        "shift" => "0x10".to_string(),
        "capslock" => "0x14".to_string(),
        "numlock" => "0x90".to_string(),
        _ => key_name.to_string(),
    }
}

/// Process a key sequence with combinations (e.g., "ctrl+c", "shift+alt+tab")
pub fn process_key_combination(combination: &str, target_system: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let parts: Vec<&str> = combination.split('+').collect();
    
    match target_system {
        "qemu" => {
            // For QEMU, send each key in the combination
            for part in parts {
                keys.push(map_key_name_qemu(part));
            }
        },
        "virtualbox" => {
            // For VirtualBox, handle special combinations
            // This is a simplified implementation for common combinations
            if combination.to_lowercase() == "ctrl+c" {
                keys.push("1d 2e ae 9d".to_string()); // ctrl down, c down, c up, ctrl up
            } else if combination.to_lowercase() == "ctrl+v" {
                keys.push("1d 2f af 9d".to_string()); // ctrl down, v down, v up, ctrl up
            } else {
                // Generic handling for any combination
                let mut down_codes = Vec::new();
                let mut up_codes = Vec::new();
                
                for part in parts {
                    let key_code = map_key_name_virtualbox(part);
                    let codes: Vec<&str> = key_code.split_whitespace().collect();
                    
                    // Add down codes
                    for &code in &codes[0..codes.len() / 2] {
                        down_codes.push(code.to_string());
                    }
                    
                    // Add up codes in reverse order
                    for &code in codes[codes.len() / 2..].iter().rev() {
                        up_codes.push(code.to_string());
                    }
                }
                
                // Combine all codes
                let all_codes: Vec<String> = down_codes.into_iter().chain(up_codes.into_iter()).collect();
                keys.push(all_codes.join(" "));
            }
        },
        "vmware" => {
            // For VMware, convert key combination to VMware format
            let mut vmware_keys = Vec::new();
            
            for part in &parts {
                let key = map_key_name_vmware(part);
                vmware_keys.push(key);
            }
            
            keys.push(vmware_keys.join("+"));
        },
        _ => {
            // Default implementation
            keys.push(combination.to_string());
        }
    }
    
    keys
}

/// Generate a bootable ISO with a custom keypress sequence for automated installation
pub fn generate_boot_keypress_iso<P1: AsRef<std::path::Path>, P2: AsRef<std::path::Path>>(
    source_iso: P1,
    output_iso: P2,
    keypress_sequence: &[KeypressSequence],
    vm_type: &str
) -> anyhow::Result<()> {
    // This is a placeholder for actual ISO modification with keypress sequence
    // In a real implementation, we would:
    // 1. Extract the ISO
    // 2. Modify the boot configuration to include the keypress sequence
    // 3. Repackage the ISO
    
    use log::info;
    
    info!("Generating bootable ISO with keypress sequence");
    info!("Source ISO: {}", source_iso.as_ref().display());
    info!("Output ISO: {}", output_iso.as_ref().display());
    info!("VM type: {}", vm_type);
    info!("Keypress sequence: {:?}", keypress_sequence);
    
    // For now, we'll just return Ok
    Ok(())
}