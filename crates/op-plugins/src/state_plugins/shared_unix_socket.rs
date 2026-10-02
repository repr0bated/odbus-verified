//! Schema carrier for durable shared Unix-socket registrations.
//!
//! The bridge owns and binds the listener. Runtime methods store metadata only.

use anyhow::{Result, bail};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

fn default_protocol() -> String {
    "grpc".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-oscal-subid" = "sch.software.shared-unix-socket.registration.schema@v2"))]
pub struct SharedRegistration {
    /// Stable across updates to the same registration name.
    #[schemars(extend("x-oscal-subid" = "obs.service.shared-unix-socket.registration.id@v1"))]
    pub id: String,
    #[schemars(extend("x-oscal-subid" = "mut.service.shared-unix-socket.registration.name@v1"))]
    pub name: String,
    /// Recorded metadata only; this plugin opens no ports.
    #[schemars(extend("x-oscal-subid" = "mut.service.shared-unix-socket.registration.ports@v1"))]
    pub ports: Vec<u16>,
    #[serde(default = "default_protocol")]
    #[schemars(extend("x-oscal-subid" = "obs.service.shared-unix-socket.registration.protocol@v1"))]
    pub protocol: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Public metadata shape. Values come from the durable method adapter, never a
/// sealed-schema example.
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.shared-unix-socket.schema@v2"))]
#[schemars(extend("x-oscal-category" = "service"))]
pub struct SharedUnixSocketState {
    #[schemars(extend("x-oscal-subid" = "obs.service.shared-unix-socket.registrations@v1"))]
    pub registrations: Vec<SharedRegistration>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateUnixSocketInput {
    pub name: String,
    pub ports: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CreateUnixSocketOutput {
    pub registration: SharedRegistration,
    pub created: bool,
    pub changed: bool,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListRegistrationsOutput {
    pub registrations: Vec<SharedRegistration>,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SharedUnixSocketConfig {
    /// Exact listener path supplied by bridge startup.
    pub shared_socket: String,
    pub listener_owner: String,
    pub registration_count: u64,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetConfigOutput {
    pub config: SharedUnixSocketConfig,
}

pub struct SharedUnixSocketPlugin;

impl SharedUnixSocketPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SharedUnixSocketPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StatePlugin for SharedUnixSocketPlugin {
    fn name(&self) -> &str {
        "shared_unix_socket"
    }

    fn version(&self) -> &str {
        "2.0.0"
    }

    fn schema(&self) -> Option<op_state_store::PluginSchema> {
        let mut schema = super::plugin_scaffold_helpers::shared_unix_socket_plugin_schema();
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema)
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
        bail!("shared Unix-socket metadata has no StatePlugin verification operation")
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        bail!("shared Unix-socket metadata has no StatePlugin checkpoint operation")
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        bail!("shared Unix-socket metadata has no StatePlugin rollback operation")
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: false,
            supports_verification: false,
            atomic_operations: false,
        }
    }
}

pub(crate) fn shared_unix_socket_schema() -> op_state_store::PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(SharedUnixSocketState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "shared_unix_socket",
        "2.0.0",
        "Durable service registrations for the bridge-owned shared Unix socket",
        &root,
    );
    use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
    use op_state_store::SideEffect;

    schema.methods.insert(
        "create_unix_socket".to_string(),
        method_decl_from_schemars_with_output::<CreateUnixSocketInput, CreateUnixSocketOutput>(
            "create_unix_socket",
            SideEffect::Mutation,
            true,
            "shared_unix_socket.invoke",
            "mut.service.shared-unix-socket.create@v2",
        ),
    );
    schema.methods.insert(
        "list_registrations".to_string(),
        method_decl_from_schemars_with_output::<(), ListRegistrationsOutput>(
            "list_registrations",
            SideEffect::Read,
            true,
            "shared_unix_socket.read",
            "obs.service.shared-unix-socket.registrations.list@v2",
        ),
    );
    schema.methods.insert(
        "get_config".to_string(),
        method_decl_from_schemars_with_output::<(), GetConfigOutput>(
            "get_config",
            SideEffect::Read,
            true,
            "shared_unix_socket.read",
            "obs.service.shared-unix-socket.config.get@v2",
        ),
    );
    schema.capabilities.insert(
        "shared_unix_socket.invoke".to_string(),
        op_state_store::CapabilityDecl {
            id: "shared_unix_socket.invoke".to_string(),
            description: "Grants only durable create_unix_socket metadata registration; it does not grant listener or routing control.".to_string(),
        },
    );
    schema.capabilities.insert(
        "shared_unix_socket.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "shared_unix_socket.read".to_string(),
            description: "Grants: list_registrations, get_config.".to_string(),
        },
    );
    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_methods_have_typed_outputs_and_matching_names() {
        let schema = shared_unix_socket_schema();
        assert_eq!(schema.version, "2.0.0");
        for name in ["create_unix_socket", "list_registrations", "get_config"] {
            let method = &schema.methods[name];
            assert_eq!(method.name, name);
            assert!(method.returns.is_some());
        }
    }
}

inventory::submit! {
    crate::default_registry::PluginReg::new("shared_unix_socket", |_ctx| std::sync::Arc::new(SharedUnixSocketPlugin::new()))
}
