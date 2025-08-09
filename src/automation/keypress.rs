use anyhow::Result;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, info};

use crate::automation::vm::{VmInstance, VmManager};

#[derive(Debug, Clone)]
pub enum KeypressAction {
    Key(String),
    KeyCombo(String, String),
    TypeText(String),
    Wait(Duration),
}

pub struct KeypressExecutor {
    // Uses VM manager to send keys through the provider abstraction
}

impl KeypressExecutor {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn execute_action(&mut self, vm: &VmInstance, action: &KeypressAction, vm_manager: &VmManager) -> Result<()> {
        match action {
            KeypressAction::Key(key) => {
                self.send_key(vm, key, vm_manager).await?;
            }
            KeypressAction::KeyCombo(modifier, key) => {
                self.send_key_combination(vm, modifier, key, vm_manager).await?;
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

    async fn send_key(&self, vm: &VmInstance, key: &str, vm_manager: &VmManager) -> Result<()> {
        debug!("Sending key '{}' to VM {}", key, vm.name);
        
        // Convert key to provider-agnostic format
        let keys = vec![key.to_string()];
        vm_manager.send_keys_to_vm(vm, &keys).await
    }

    async fn send_key_combination(&self, vm: &VmInstance, modifier: &str, key: &str, vm_manager: &VmManager) -> Result<()> {
        debug!("Sending key combination '{}+{}' to VM {}", modifier, key, vm.name);
        
        // For key combinations, we'll send them as separate key events
        // The provider will handle the specific implementation details
        let keys = vec![
            format!("{}+{}", modifier, key)
        ];
        vm_manager.send_keys_to_vm(vm, &keys).await
    }

    async fn type_text(&self, vm: &VmInstance, text: &str, vm_manager: &VmManager) -> Result<()> {
        info!("Typing text to VM {}: '{}'", vm.name, text);
        
        // Convert text to individual character keys
        let keys: Vec<String> = text.chars().map(|c| c.to_string()).collect();
        vm_manager.send_keys_to_vm(vm, &keys).await
    }
}