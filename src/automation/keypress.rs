#![allow(dead_code)]

use anyhow::Result;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, info};

use crate::automation::library_keyboard_input::LibraryBasedKeyboardMapper;
use crate::automation::vm::{VmInstance, VmManager};

#[derive(Debug, Clone)]
pub enum KeypressAction {
    Key(String),
    KeyCombo(Vec<String>, String), // modifiers, key
    TypeText(String),
    Wait(Duration),
}

pub struct KeypressExecutor {
    // Uses VM manager to send keys through the provider abstraction
    keyboard_mapper: LibraryBasedKeyboardMapper,
}

impl KeypressExecutor {
    pub fn new() -> Self {
        Self {
            keyboard_mapper: LibraryBasedKeyboardMapper::new(),
        }
    }

    pub async fn execute_action(
        &mut self,
        vm: &VmInstance,
        action: &KeypressAction,
        vm_manager: &VmManager,
    ) -> Result<()> {
        match action {
            KeypressAction::Key(key) => {
                self.send_key(vm, key, vm_manager).await?;
            }
            KeypressAction::KeyCombo(modifiers, key) => {
                self.send_key_combination(vm, modifiers, key, vm_manager)
                    .await?;
            }
            KeypressAction::TypeText(text) => {
                self.type_text(vm, text, vm_manager).await?;
            }
            KeypressAction::Wait(duration) => {
                debug!("Waiting for {:?}", duration);
                sleep(*duration).await;
            }
        }

        // Small delay between actions to ensure they're processed
        sleep(Duration::from_millis(50)).await;
        Ok(())
    }

    async fn send_key(&mut self, vm: &VmInstance, key: &str, vm_manager: &VmManager) -> Result<()> {
        debug!("Sending key '{}' to VM {}", key, vm.name);

        // Use the enhanced keyboard mapper for special keys
        let scancodes = if key.len() == 1 {
            // Single character
            self.keyboard_mapper.text_to_scancodes(key)?
        } else {
            // Special key (e.g., "enter", "f1", etc.)
            self.keyboard_mapper.special_key_to_scancodes(key)?
        };

        vm_manager.send_keys_to_vm(vm, &scancodes).await
    }

    async fn send_key_combination(
        &mut self,
        vm: &VmInstance,
        modifiers: &[String],
        key: &str,
        vm_manager: &VmManager,
    ) -> Result<()> {
        debug!(
            "Sending key combination '{:?}+{}' to VM {}",
            modifiers, key, vm.name
        );

        // Use the enhanced keyboard mapper for key combinations
        let scancodes = self
            .keyboard_mapper
            .key_combination_to_scancodes(modifiers, key)?;
        vm_manager.send_keys_to_vm(vm, &scancodes).await
    }

    async fn type_text(
        &mut self,
        vm: &VmInstance,
        text: &str,
        vm_manager: &VmManager,
    ) -> Result<()> {
        info!("Typing text to VM {}: '{}'", vm.name, text);

        // Use the enhanced keyboard mapper for comprehensive text input
        let scancodes = self.keyboard_mapper.text_to_scancodes(text)?;
        vm_manager.send_keys_to_vm(vm, &scancodes).await
    }
}
