//! Built-in plugins

use anyhow::Result;
use async_trait::async_trait;
use simd_json::OwnedValue as Value;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::plugin::Plugin;
use crate::state::{DesiredState, StateChange, ValidationResult};

/// Echo plugin for testing
pub struct EchoPlugin {
    name: String,
    state: Arc<RwLock<Value>>,
    desired: Arc<RwLock<DesiredState>>,
}

impl EchoPlugin {
    pub fn new() -> Self {
        Self {
            name: "echo".to_string(),
            state: Arc::new(RwLock::new(simd_json::json!({}))),
            desired: Arc::new(RwLock::new(DesiredState::default())),
        }
    }
}

#[async_trait]
impl Plugin for EchoPlugin {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        "Echo plugin for testing"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }

    async fn get_state(&self) -> Result<Value> {
        Ok(self.state.read().await.clone())
    }

    async fn get_desired_state(&self) -> Result<DesiredState> {
        Ok(self.desired.read().await.clone())
    }

    async fn set_desired_state(&self, desired: DesiredState) -> Result<()> {
        *self.desired.write().await = desired;
        Ok(())
    }

    async fn apply_state(&self) -> Result<Vec<StateChange>> {
        let desired = self.desired.read().await;
        *self.state.write().await = desired.state.clone();
        Ok(vec![])
    }

    async fn diff(&self) -> Result<Vec<StateChange>> {
        Ok(vec![])
    }

    async fn validate(&self, _config: &Value) -> Result<ValidationResult> {
        Ok(ValidationResult::success())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Default for EchoPlugin {
    fn default() -> Self {
        Self::new()
    }
}

// Re-export DynamicLoadingPlugin from its module
pub use crate::dynamic_loading::DynamicLoadingPlugin;
