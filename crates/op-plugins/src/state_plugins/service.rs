//! Runit lifecycle operations for service definitions supplied by the snapshot.

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
mod methods;
use crate::service_def::ServiceName;
use anyhow::Result;
use async_trait::async_trait;
pub use methods::{ServiceInitOutput, ServiceOperationOutput, dispatch_service_method};
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema, SideEffect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simd_json::{OwnedValue as Value, json};

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct ServiceLifecycle {
    pub last_active: Option<u64>,
    pub days_since_active: Option<u64>,
    pub is_orphaned: bool,
    pub orphan_reason: Option<String>,
}

// D-Bus method input types for service lifecycle operations.

/// Enable a runit definition already supplied by the filesystem snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InitInput {
    /// Service name
    pub name: String,
    /// Optional expected native definition type; mismatches prevent enablement.
    #[serde(default)]
    pub service_type: String,
}

/// Run method input - start/running a service
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunInput {
    /// Service name
    pub name: String,
    /// Runit executes the installed definition. Runtime argument overrides are
    /// not supported; this compatibility field must be empty.
    #[serde(default)]
    #[schemars(length(max = 0))]
    pub args: Vec<String>,
}

/// Shutdown method input - stop a service
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShutdownInput {
    /// Service name
    pub name: String,
    /// Force stop (SIGKILL)
    #[serde(default)]
    pub force: bool,
}

pub struct ServicePlugin;

impl Default for ServicePlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ServicePlugin {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StatePlugin for ServicePlugin {
    fn name(&self) -> &str {
        "service"
    }
    fn version(&self) -> &str {
        "2.0.0"
    }

    fn schema(&self) -> Option<op_state_store::PluginSchema> {
        Some(service_schema())
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: String::new(),
                desired_hash: String::new(),
            },
        })
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        Ok(ApplyResult {
            success: true,
            changes_applied: vec![],
            errors: vec![],
            checkpoint: None,
        })
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: format!("service-{}", chrono::Utc::now().timestamp()),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: json!({}),
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: true,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
#[schemars(extend("x-oscal-category" = "service"))]
pub struct ServiceSchemaState {
    pub services: serde_json::Value,
}

pub(crate) fn service_schema() -> PluginSchema {
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "service",
        "2.0.0",
        "Service definition declarations",
        &serde_json::to_value(schemars::schema_for!(ServiceSchemaState)).unwrap(),
    );
    super::schemars_adapter::apply_state_defaults(
        &mut schema,
        &simd_json::serde::to_owned_value(&ServiceSchemaState::default()).unwrap(),
    );

    // Add D-Bus methods for runit service lifecycle management.
    schema.methods.insert(
        "init".to_string(),
        method_decl_from_schemars_with_output::<InitInput, ServiceInitOutput>(
            "init",
            SideEffect::Mutation,
            false,
            "cap.service.lifecycle.init@v1",
            "mut.service.lifecycle.init@v1",
        ),
    );
    schema.methods.insert(
        "run".to_string(),
        method_decl_from_schemars_with_output::<RunInput, ServiceOperationOutput>(
            "run",
            SideEffect::Mutation,
            false,
            "cap.service.lifecycle.run@v1",
            "mut.service.lifecycle.run@v1",
        ),
    );
    schema.methods.insert(
        "shutdown".to_string(),
        method_decl_from_schemars_with_output::<ShutdownInput, ServiceOperationOutput>(
            "shutdown",
            SideEffect::Mutation,
            false,
            "cap.service.lifecycle.shutdown@v1",
            "mut.service.lifecycle.shutdown@v1",
        ),
    );

    schema.capabilities.insert(
        "cap.service.lifecycle.init@v1".to_string(),
        CapabilityDecl {
            id: "cap.service.lifecycle.init@v1".to_string(),
            description: "Grants: init.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.service.lifecycle.run@v1".to_string(),
        CapabilityDecl {
            id: "cap.service.lifecycle.run@v1".to_string(),
            description: "Grants: run.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.service.lifecycle.shutdown@v1".to_string(),
        CapabilityDecl {
            id: "cap.service.lifecycle.shutdown@v1".to_string(),
            description: "Grants: shutdown.".to_string(),
        },
    );

    schema
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("service", |_ctx| std::sync::Arc::new(ServicePlugin::new()))
}
