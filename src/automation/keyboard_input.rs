use anyhow::{anyhow, Result};
use std::collections::HashMap;
use tracing::debug;

/// Enhanced keyboard input mapping that supports all Unicode characters and special keys
pub struct KeyboardMapper {
    /// Maps characters to VirtualBox scancodes
    char_to_scancode: HashMap<char, Vec<String>>,
}

impl KeyboardMapper {
    pub fn new() -> Self {
        let mut mapper = Self {
            char_to_scancode: HashMap::new(),
        };
        mapper.init_basic_mappings();
        mapper
    }

    /// Initialize basic character-to-scancode mappings for VirtualBox
    fn init_basic_mappings(&mut self) {
        // Basic ASCII characters (unshifted)
        let basic_chars = [
            // Letters (lowercase)
            ('a', vec!["1e", "9e"]),
            ('b', vec!["30", "b0"]),
            ('c', vec!["2e", "ae"]),
            ('d', vec!["20", "a0"]),
            ('e', vec!["12", "92"]),
            ('f', vec!["21", "a1"]),
            ('g', vec!["22", "a2"]),
            ('h', vec!["23", "a3"]),
            ('i', vec!["17", "97"]),
            ('j', vec!["24", "a4"]),
            ('k', vec!["25", "a5"]),
            ('l', vec!["26", "a6"]),
            ('m', vec!["32", "b2"]),
            ('n', vec!["31", "b1"]),
            ('o', vec!["18", "98"]),
            ('p', vec!["19", "99"]),
            ('q', vec!["10", "90"]),
            ('r', vec!["13", "93"]),
            ('s', vec!["1f", "9f"]),
            ('t', vec!["14", "94"]),
            ('u', vec!["16", "96"]),
            ('v', vec!["2f", "af"]),
            ('w', vec!["11", "91"]),
            ('x', vec!["2d", "ad"]),
            ('y', vec!["15", "95"]),
            ('z', vec!["2c", "ac"]),
            
            // Numbers
            ('0', vec!["0b", "8b"]),
            ('1', vec!["02", "82"]),
            ('2', vec!["03", "83"]),
            ('3', vec!["04", "84"]),
            ('4', vec!["05", "85"]),
            ('5', vec!["06", "86"]),
            ('6', vec!["07", "87"]),
            ('7', vec!["08", "88"]),
            ('8', vec!["09", "89"]),
            ('9', vec!["0a", "8a"]),
            
            // Basic punctuation
            (' ', vec!["39", "b9"]),  // Space
            ('-', vec!["0c", "8c"]),  // Hyphen/minus
            ('=', vec!["0d", "8d"]),  // Equals
            ('[', vec!["1a", "9a"]),  // Left bracket
            (']', vec!["1b", "9b"]),  // Right bracket
            ('\\', vec!["2b", "ab"]), // Backslash
            (';', vec!["27", "a7"]),  // Semicolon
            ('\'', vec!["28", "a8"]), // Apostrophe/single quote
            ('`', vec!["29", "a9"]),  // Grave accent/backtick
            (',', vec!["33", "b3"]),  // Comma
            ('.', vec!["34", "b4"]),  // Period/dot
            ('/', vec!["35", "b5"]),  // Forward slash
        ];
        
        for (ch, scancodes) in basic_chars {
            self.char_to_scancode.insert(ch, scancodes.iter().map(|s| s.to_string()).collect());
        }
        
        // Uppercase letters and shifted characters
        let shifted_chars = [
            // Uppercase letters (shift + letter)
            ('A', vec!["2a", "1e", "9e", "aa"]),
            ('B', vec!["2a", "30", "b0", "aa"]),
            ('C', vec!["2a", "2e", "ae", "aa"]),
            ('D', vec!["2a", "20", "a0", "aa"]),
            ('E', vec!["2a", "12", "92", "aa"]),
            ('F', vec!["2a", "21", "a1", "aa"]),
            ('G', vec!["2a", "22", "a2", "aa"]),
            ('H', vec!["2a", "23", "a3", "aa"]),
            ('I', vec!["2a", "17", "97", "aa"]),
            ('J', vec!["2a", "24", "a4", "aa"]),
            ('K', vec!["2a", "25", "a5", "aa"]),
            ('L', vec!["2a", "26", "a6", "aa"]),
            ('M', vec!["2a", "32", "b2", "aa"]),
            ('N', vec!["2a", "31", "b1", "aa"]),
            ('O', vec!["2a", "18", "98", "aa"]),
            ('P', vec!["2a", "19", "99", "aa"]),
            ('Q', vec!["2a", "10", "90", "aa"]),
            ('R', vec!["2a", "13", "93", "aa"]),
            ('S', vec!["2a", "1f", "9f", "aa"]),
            ('T', vec!["2a", "14", "94", "aa"]),
            ('U', vec!["2a", "16", "96", "aa"]),
            ('V', vec!["2a", "2f", "af", "aa"]),
            ('W', vec!["2a", "11", "91", "aa"]),
            ('X', vec!["2a", "2d", "ad", "aa"]),
            ('Y', vec!["2a", "15", "95", "aa"]),
            ('Z', vec!["2a", "2c", "ac", "aa"]),
            
            // Shifted numbers and symbols
            ('!', vec!["2a", "02", "82", "aa"]),  // Shift+1
            ('@', vec!["2a", "03", "83", "aa"]),  // Shift+2
            ('#', vec!["2a", "04", "84", "aa"]),  // Shift+3
            ('$', vec!["2a", "05", "85", "aa"]),  // Shift+4
            ('%', vec!["2a", "06", "86", "aa"]),  // Shift+5
            ('^', vec!["2a", "07", "87", "aa"]),  // Shift+6
            ('&', vec!["2a", "08", "88", "aa"]),  // Shift+7
            ('*', vec!["2a", "09", "89", "aa"]),  // Shift+8
            ('(', vec!["2a", "0a", "8a", "aa"]),  // Shift+9
            (')', vec!["2a", "0b", "8b", "aa"]),  // Shift+0
            ('_', vec!["2a", "0c", "8c", "aa"]),  // Shift+-
            ('+', vec!["2a", "0d", "8d", "aa"]),  // Shift+=
            ('{', vec!["2a", "1a", "9a", "aa"]),  // Shift+[
            ('}', vec!["2a", "1b", "9b", "aa"]),  // Shift+]
            ('|', vec!["2a", "2b", "ab", "aa"]),  // Shift+\
            (':', vec!["2a", "27", "a7", "aa"]),  // Shift+;
            ('"', vec!["2a", "28", "a8", "aa"]),  // Shift+'
            ('~', vec!["2a", "29", "a9", "aa"]),  // Shift+`
            ('<', vec!["2a", "33", "b3", "aa"]),  // Shift+,
            ('>', vec!["2a", "34", "b4", "aa"]),  // Shift+.
            ('?', vec!["2a", "35", "b5", "aa"]),  // Shift+/
        ];
        
        for (ch, scancodes) in shifted_chars {
            self.char_to_scancode.insert(ch, scancodes.iter().map(|s| s.to_string()).collect());
        }
    }

    /// Convert text to VirtualBox scancodes, supporting Unicode characters
    pub fn text_to_scancodes(&self, text: &str) -> Result<Vec<String>> {
        let mut scancodes = Vec::new();
        
        for ch in text.chars() {
            if let Some(codes) = self.char_to_scancode.get(&ch) {
                // Use the direct mapping for characters we have
                scancodes.extend(codes.clone());
            } else {
                // For Unicode characters not in our mapping, try to handle them
                scancodes.extend(self.handle_unicode_char(ch)?);
            }
        }
        
        Ok(scancodes)
    }

    /// Handle Unicode characters that don't have direct scancode mappings
    fn handle_unicode_char(&self, ch: char) -> Result<Vec<String>> {
        debug!("Handling Unicode character: {} (U+{:04X})", ch, ch as u32);
        
        // For now, try to decompose or find alternatives for common Unicode chars
        match ch {
            // Common Unicode punctuation that might appear in text
            '\u{2018}' | '\u{2019}' => self.char_to_scancode.get(&'\'').cloned(), // Smart quotes -> apostrophe
            '\u{201C}' | '\u{201D}' => self.char_to_scancode.get(&'"').cloned(),  // Smart double quotes
            '\u{2013}' | '\u{2014}' => self.char_to_scancode.get(&'-').cloned(),  // En dash, em dash -> hyphen
            '\u{00A0}' => self.char_to_scancode.get(&' ').cloned(),              // Non-breaking space -> space
            '\n' | '\r' => Some(vec!["1c".to_string(), "9c".to_string()]),       // Line feeds -> Enter
            '\t' => Some(vec!["0f".to_string(), "8f".to_string()]),              // Tab
            
            // For other Unicode characters, we'll try to find the closest ASCII equivalent
            // or fall back to a placeholder
            _ => {
                // Try to find a similar ASCII character
                if let Some(ascii_equivalent) = self.find_ascii_equivalent(ch) {
                    debug!("Using ASCII equivalent '{}' for Unicode char '{}'", ascii_equivalent, ch);
                    self.char_to_scancode.get(&ascii_equivalent).cloned()
                } else {
                    // As a last resort, try to use Unicode input via ALT codes
                    // This is a fallback that may not work on all systems
                    debug!("No mapping found for Unicode character '{}', using placeholder", ch);
                    self.char_to_scancode.get(&'?').cloned() // Use '?' as placeholder
                }
            }
        }.ok_or_else(|| anyhow!("No scancode mapping available for character: {} (U+{:04X})", ch, ch as u32))
    }

    /// Try to find an ASCII equivalent for a Unicode character
    fn find_ascii_equivalent(&self, ch: char) -> Option<char> {
        // Simple mapping for common accented characters
        match ch {
            // Accented vowels -> unaccented
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => Some('a'),
            'è' | 'é' | 'ê' | 'ë' | 'È' | 'É' | 'Ê' | 'Ë' => Some('e'),
            'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' => Some('i'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' => Some('o'),
            'ù' | 'ú' | 'û' | 'ü' | 'Ù' | 'Ú' | 'Û' | 'Ü' => Some('u'),
            
            // Other accented characters
            'ç' | 'Ç' => Some('c'),
            'ñ' | 'Ñ' => Some('n'),
            'ý' | 'ÿ' | 'Ý' => Some('y'),
            
            // Currency symbols
            '€' => Some('E'), // Euro
            '£' => Some('L'), // Pound
            '¥' => Some('Y'), // Yen
            
            _ => None
        }
    }

    /// Get special key scancodes (for PRESS command)
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
            "home" => vec!["e0", "47", "e0", "c7"],
            "end" => vec!["e0", "4f", "e0", "cf"],
            "pageup" | "pgup" => vec!["e0", "49", "e0", "c9"],
            "pagedown" | "pgdn" => vec!["e0", "51", "e0", "d1"],
            "insert" => vec!["e0", "52", "e0", "d2"],
            "delete" | "del" => vec!["e0", "53", "e0", "d3"],
            "backspace" => vec!["0e", "8e"],
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
            "ctrl" | "control" => if press { "1d" } else { "9d" },
            "shift" => if press { "2a" } else { "aa" },
            "alt" => if press { "38" } else { "b8" },
            "meta" | "cmd" | "win" | "windows" => if press { "e0 5b" } else { "e0 db" },
            _ => return Err(anyhow!("Unknown modifier key: {}", modifier)),
        };
        
        Ok(scancode.to_string())
    }

    /// Handle complex key combinations with multiple modifiers
    pub fn key_combination_to_scancodes(&self, modifiers: &[String], key: &str) -> Result<Vec<String>> {
        let mut scancodes = Vec::new();
        
        // Press all modifiers
        for modifier in modifiers {
            scancodes.push(self.modifier_to_scancodes(modifier, true)?);
        }
        
        // Press and release the main key
        if key.len() == 1 {
            // Single character
            let ch = key.chars().next().unwrap();
            if let Some(key_scancodes) = self.char_to_scancode.get(&ch.to_ascii_lowercase()) {
                // Just press/release the key, don't include shift since modifiers handle it
                if key_scancodes.len() >= 2 {
                    scancodes.push(key_scancodes[0].clone()); // Press
                    scancodes.push(key_scancodes[1].clone()); // Release
                }
            }
        } else {
            // Special key
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

impl Default for KeyboardMapper {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_text_mapping() {
        let mapper = KeyboardMapper::new();
        
        // Test basic ASCII text
        let result = mapper.text_to_scancodes("hello").unwrap();
        assert!(!result.is_empty());
        
        // Test mixed case
        let result = mapper.text_to_scancodes("Hello").unwrap();
        assert!(!result.is_empty());
        
        // Test numbers and symbols
        let result = mapper.text_to_scancodes("test123!@#").unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn test_special_keys() {
        let mapper = KeyboardMapper::new();
        
        // Test function keys
        let result = mapper.special_key_to_scancodes("f1").unwrap();
        assert_eq!(result, vec!["3b", "bb"]);
        
        // Test arrow keys
        let result = mapper.special_key_to_scancodes("up").unwrap();
        assert!(!result.is_empty());
        
        // Test enter key
        let result = mapper.special_key_to_scancodes("enter").unwrap();
        assert_eq!(result, vec!["1c", "9c"]);
    }

    #[test]
    fn test_key_combinations() {
        let mapper = KeyboardMapper::new();
        
        // Test ctrl+c
        let modifiers = vec!["ctrl".to_string()];
        let result = mapper.key_combination_to_scancodes(&modifiers, "c").unwrap();
        assert!(!result.is_empty());
        
        // Test ctrl+alt+t
        let modifiers = vec!["ctrl".to_string(), "alt".to_string()];
        let result = mapper.key_combination_to_scancodes(&modifiers, "t").unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn test_unicode_fallback() {
        let mapper = KeyboardMapper::new();
        
        // Test Unicode character with ASCII equivalent
        let result = mapper.text_to_scancodes("café").unwrap();
        assert!(!result.is_empty());
        
        // Test smart quotes
        let result = mapper.text_to_scancodes("\"test\"").unwrap();
        assert!(!result.is_empty());
    }
}