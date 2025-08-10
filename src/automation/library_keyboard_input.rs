// Prototype keyboard mapper using external libraries instead of hardcoded mappings
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use tracing::debug;

/// Enhanced keyboard input mapping using external libraries where possible
pub struct LibraryBasedKeyboardMapper {
    /// Cache for previously computed scancode mappings
    scancode_cache: HashMap<char, Vec<String>>,
    /// Fallback mappings for characters that libraries can't handle
    fallback_mappings: HashMap<char, Vec<String>>,
}

impl LibraryBasedKeyboardMapper {
    pub fn new() -> Self {
        let mut mapper = Self {
            scancode_cache: HashMap::new(),
            fallback_mappings: HashMap::new(),
        };

        // Initialize critical fallback mappings for characters that may not work with libraries
        mapper.init_fallback_mappings();
        mapper
    }

    /// Initialize minimal fallback mappings for critical characters
    fn init_fallback_mappings(&mut self) {
        // Only include mappings that are critical and likely to fail with external libraries
        let critical_mappings = [
            (' ', vec!["39", "b9"]),  // Space
            ('\n', vec!["1c", "9c"]), // Enter
            ('\t', vec!["0f", "8f"]), // Tab
        ];

        for (ch, scancodes) in critical_mappings {
            self.fallback_mappings
                .insert(ch, scancodes.iter().map(|s| s.to_string()).collect());
        }
    }

    /// Convert text to VirtualBox scancodes using external libraries where possible
    pub fn text_to_scancodes(&mut self, text: &str) -> Result<Vec<String>> {
        let mut scancodes = Vec::new();

        for ch in text.chars() {
            // Check cache first
            if let Some(cached_codes) = self.scancode_cache.get(&ch) {
                scancodes.extend(cached_codes.clone());
                continue;
            }

            // Try to generate scancodes for this character
            let char_scancodes = self.generate_char_scancodes(ch)?;

            // Cache the result
            self.scancode_cache.insert(ch, char_scancodes.clone());
            scancodes.extend(char_scancodes);
        }

        Ok(scancodes)
    }

    /// Generate scancodes for a single character using the best available method
    fn generate_char_scancodes(&self, ch: char) -> Result<Vec<String>> {
        // 1. Try fallback mappings first for critical characters
        if let Some(codes) = self.fallback_mappings.get(&ch) {
            debug!("Using fallback mapping for character: {}", ch);
            return Ok(codes.clone());
        }

        // 2. Try to use external library (scancode crate would go here)
        if let Ok(codes) = self.try_library_mapping(ch) {
            debug!("Generated library-based mapping for character: {}", ch);
            return Ok(codes);
        }

        // 3. Try Unicode handling
        if let Ok(codes) = self.handle_unicode_char(ch) {
            debug!("Using Unicode fallback for character: {}", ch);
            return Ok(codes);
        }

        // 4. Final fallback - use '?' as placeholder
        debug!("No mapping found for character '{}', using placeholder", ch);
        Ok(vec![
            "2a".to_string(),
            "35".to_string(),
            "b5".to_string(),
            "aa".to_string(),
        ]) // Shift+/ = ?
    }

    /// Try to generate scancodes using external libraries
    fn try_library_mapping(&self, ch: char) -> Result<Vec<String>> {
        // This is where we would integrate with the scancode crate
        // For now, implement a basic mapping for common ASCII characters

    match ch {
            // Letters (lowercase)
            'a'..='z' => {
                let base_code = (ch as u8 - b'a') as u8;
                let scancode_table = [
                    0x1e, 0x30, 0x2e, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32,
                    0x31, 0x18, 0x19, 0x10, 0x13, 0x1f, 0x14, 0x16, 0x2f, 0x11, 0x2d, 0x15, 0x2c,
                ];

                if let Some(&code) = scancode_table.get(base_code as usize) {
                    Ok(vec![
                        format!("{:02x}", code),
                        format!("{:02x}", code | 0x80),
                    ])
                } else {
                    Err(anyhow!("Invalid letter"))
                }
            }

            // Uppercase letters (use shift + lowercase)
            'A'..='Z' => {
                let lowercase = ch.to_ascii_lowercase();
                if let Ok(mut codes) = self.try_library_mapping(lowercase) {
                    // Insert shift press at beginning and shift release at end
                    codes.insert(0, "2a".to_string()); // Shift press
                    codes.push("aa".to_string()); // Shift release
                    Ok(codes)
                } else {
                    Err(anyhow!("Failed to generate uppercase letter"))
                }
            }

            // Numbers
            '0'..='9' => {
                let digit = (ch as u8 - b'0') as u8;
                let number_codes = [0x0b, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a];

                if let Some(&code) = number_codes.get(digit as usize) {
                    Ok(vec![
                        format!("{:02x}", code),
                        format!("{:02x}", code | 0x80),
                    ])
                } else {
                    Err(anyhow!("Invalid digit"))
                }
            }

            // Special characters (US keyboard layout)
            '`' => Ok(vec!["29".to_string(), "a9".to_string()]), // Backtick
            '~' => Ok(vec!["2a".to_string(), "29".to_string(), "a9".to_string(), "aa".to_string()]), // Shift + backtick
            '!' => Ok(vec!["2a".to_string(), "02".to_string(), "82".to_string(), "aa".to_string()]), // Shift + 1
            '@' => Ok(vec!["2a".to_string(), "03".to_string(), "83".to_string(), "aa".to_string()]), // Shift + 2
            '#' => Ok(vec!["2a".to_string(), "04".to_string(), "84".to_string(), "aa".to_string()]), // Shift + 3
            '$' => Ok(vec!["2a".to_string(), "05".to_string(), "85".to_string(), "aa".to_string()]), // Shift + 4
            '%' => Ok(vec!["2a".to_string(), "06".to_string(), "86".to_string(), "aa".to_string()]), // Shift + 5
            '^' => Ok(vec!["2a".to_string(), "07".to_string(), "87".to_string(), "aa".to_string()]), // Shift + 6
            '&' => Ok(vec!["2a".to_string(), "08".to_string(), "88".to_string(), "aa".to_string()]), // Shift + 7
            '*' => Ok(vec!["2a".to_string(), "09".to_string(), "89".to_string(), "aa".to_string()]), // Shift + 8
            '(' => Ok(vec!["2a".to_string(), "0a".to_string(), "8a".to_string(), "aa".to_string()]), // Shift + 9
            ')' => Ok(vec!["2a".to_string(), "0b".to_string(), "8b".to_string(), "aa".to_string()]), // Shift + 0
            '-' => Ok(vec!["0c".to_string(), "8c".to_string()]), // Hyphen/minus
            '_' => Ok(vec!["2a".to_string(), "0c".to_string(), "8c".to_string(), "aa".to_string()]), // Shift + hyphen
            // Equals sign '=' (US keyboard: 0x0d)
            '=' => Ok(vec!["0d".to_string(), "8d".to_string()]),
            '+' => Ok(vec!["2a".to_string(), "0d".to_string(), "8d".to_string(), "aa".to_string()]), // Shift + equals
            '[' => Ok(vec!["1a".to_string(), "9a".to_string()]), // Left bracket
            '{' => Ok(vec!["2a".to_string(), "1a".to_string(), "9a".to_string(), "aa".to_string()]), // Shift + left bracket
            ']' => Ok(vec!["1b".to_string(), "9b".to_string()]), // Right bracket
            '}' => Ok(vec!["2a".to_string(), "1b".to_string(), "9b".to_string(), "aa".to_string()]), // Shift + right bracket
            '\\' => Ok(vec!["2b".to_string(), "ab".to_string()]), // Backslash
            '|' => Ok(vec!["2a".to_string(), "2b".to_string(), "ab".to_string(), "aa".to_string()]), // Shift + backslash
            ';' => Ok(vec!["27".to_string(), "a7".to_string()]), // Semicolon
            ':' => Ok(vec!["2a".to_string(), "27".to_string(), "a7".to_string(), "aa".to_string()]), // Shift + semicolon
            '\'' => Ok(vec!["28".to_string(), "a8".to_string()]), // Single quote
            '"' => Ok(vec!["2a".to_string(), "28".to_string(), "a8".to_string(), "aa".to_string()]), // Shift + single quote
            ',' => Ok(vec!["33".to_string(), "b3".to_string()]), // Comma
            '<' => Ok(vec!["2a".to_string(), "33".to_string(), "b3".to_string(), "aa".to_string()]), // Shift + comma
            '.' => Ok(vec!["34".to_string(), "b4".to_string()]), // Period
            '>' => Ok(vec!["2a".to_string(), "34".to_string(), "b4".to_string(), "aa".to_string()]), // Shift + period
            '/' => Ok(vec!["35".to_string(), "b5".to_string()]), // Forward slash
            '?' => Ok(vec!["2a".to_string(), "35".to_string(), "b5".to_string(), "aa".to_string()]), // Shift + forward slash

            _ => Err(anyhow!("Character not supported by library mapping")),
        }
    }

    /// Handle Unicode characters with fallbacks
    fn handle_unicode_char(&self, ch: char) -> Result<Vec<String>> {
        match ch {
            // Smart quotes to regular quotes
            '\u{2018}' | '\u{2019}' => self.try_library_mapping('\''),
            '\u{201C}' | '\u{201D}' => self.try_library_mapping('"'),

            // Dashes to hyphens
            '\u{2013}' | '\u{2014}' => self.try_library_mapping('-'),

            // Non-breaking space to space
            '\u{00A0}' => Ok(self.fallback_mappings.get(&' ').unwrap().clone()),

            // Try to find ASCII equivalent
            _ => {
                if let Some(ascii_equiv) = self.find_ascii_equivalent(ch) {
                    self.try_library_mapping(ascii_equiv)
                } else {
                    Err(anyhow!("No Unicode fallback available"))
                }
            }
        }
    }

    /// Find ASCII equivalent for Unicode characters
    fn find_ascii_equivalent(&self, ch: char) -> Option<char> {
        match ch {
            // Accented vowels
            'à'..='å' | 'À'..='Å' => Some('a'),
            'è'..='ë' | 'È'..='Ë' => Some('e'),
            'ì'..='ï' | 'Ì'..='Ï' => Some('i'),
            'ò'..='ö' | 'Ò'..='Ö' => Some('o'),
            'ù'..='ü' | 'Ù'..='Ü' => Some('u'),
            'ç' | 'Ç' => Some('c'),
            'ñ' | 'Ñ' => Some('n'),

            // Currency symbols
            '€' => Some('E'),
            '£' => Some('L'),
            '¥' => Some('Y'),

            _ => None,
        }
    }

    /// Get special key scancodes
    pub fn special_key_to_scancodes(&self, key: &str) -> Result<Vec<String>> {
        let scancodes = match key.to_lowercase().as_str() {
            "enter" | "return" => vec!["1c", "9c"],
            "tab" => vec!["0f", "8f"],
            "space" => vec!["39", "b9"],
            "esc" | "escape" => vec!["01", "81"],
            "up" => vec!["e0", "48", "e0", "c8"],
            "down" => vec!["e0", "50", "e0", "d0"],
            "left" => vec!["e0", "4b", "e0", "cb"],
            "right" => vec!["e0", "4d", "e0", "cd"],
            "f1" => vec!["3b", "bb"],
            "f2" => vec!["3c", "bc"],
            "f3" => vec!["3d", "bd"],
            "f4" => vec!["3e", "be"],
            "f5" => vec!["3f", "bf"],
            "f6" => vec!["40", "c0"],
            "f7" => vec!["41", "c1"],
            "f8" => vec!["42", "c2"],
            "f9" => vec!["43", "c3"],
            "f10" => vec!["44", "c4"],
            "f11" => vec!["57", "d7"],
            "f12" => vec!["58", "d8"],
            _ => return Err(anyhow!("Unknown special key: {}", key)),
        };

        Ok(scancodes.into_iter().map(|s| s.to_string()).collect())
    }

    /// Get modifier key scancodes
    pub fn modifier_to_scancodes(&self, modifier: &str, press: bool) -> Result<String> {
        let scancode = match modifier.to_lowercase().as_str() {
            "ctrl" | "control" => {
                if press {
                    "1d"
                } else {
                    "9d"
                }
            }
            "shift" => {
                if press {
                    "2a"
                } else {
                    "aa"
                }
            }
            "alt" => {
                if press {
                    "38"
                } else {
                    "b8"
                }
            }
            _ => return Err(anyhow!("Unknown modifier key: {}", modifier)),
        };

        Ok(scancode.to_string())
    }

    /// Handle complex key combinations
    pub fn key_combination_to_scancodes(
        &mut self,
        modifiers: &[String],
        key: &str,
    ) -> Result<Vec<String>> {
        let mut scancodes = Vec::new();

        // Press all modifiers
        for modifier in modifiers {
            scancodes.push(self.modifier_to_scancodes(modifier, true)?);
        }

        // Press and release the main key
        if key.len() == 1 {
            let ch = key.chars().next().unwrap();
            let key_scancodes = self.generate_char_scancodes(ch.to_ascii_lowercase())?;
            // For combinations, only use the key press/release, not shift modifiers
            if key_scancodes.len() >= 2 {
                scancodes.push(key_scancodes[0].clone());
                scancodes.push(key_scancodes[1].clone());
            }
        } else {
            let key_scancodes = self.special_key_to_scancodes(key)?;
            scancodes.extend(key_scancodes);
        }

        // Release all modifiers (in reverse order)
        for modifier in modifiers.iter().rev() {
            scancodes.push(self.modifier_to_scancodes(modifier, false)?);
        }

        Ok(scancodes)
    }
}

impl Default for LibraryBasedKeyboardMapper {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_text_mapping() {
        let mut mapper = LibraryBasedKeyboardMapper::new();

        // Test basic ASCII text
        let result = mapper.text_to_scancodes("hello").unwrap();
        assert!(!result.is_empty());

        // Test mixed case
        let result = mapper.text_to_scancodes("Hello").unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn test_fallback_mappings() {
        let mapper = LibraryBasedKeyboardMapper::new();

        // Test space (should use fallback)
        let result = mapper.special_key_to_scancodes("space").unwrap();
        assert_eq!(result, vec!["39", "b9"]);
    }

    #[test]
    fn test_unicode_fallback() {
        let mut mapper = LibraryBasedKeyboardMapper::new();

        // Test Unicode character with ASCII equivalent
        let result = mapper.text_to_scancodes("café").unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn test_equals_sign_mapping() {
        let mut mapper = LibraryBasedKeyboardMapper::new();
        let result = mapper.text_to_scancodes("=").unwrap();
        // 0x0d = make, 0x8d = break for '=' on US keyboard
        assert_eq!(result, vec!["0d", "8d"]);
    }
}
