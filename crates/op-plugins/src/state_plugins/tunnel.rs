//! The door tunnel: the host door (10.0.0.2:8090) spliced over a shared socket
//! into the door-owning container's own tonic door, where TLS ends.
//!
//! The host bridge owns the mux (crate `op-tunnel-mux`). This plugin is its
//! sealed schema: state and controls are methods visible through 3tchedFS on
//! the host and in the chatbot. The bytes never pass through this plugin or
//! state; they stay on the socket.

use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

/// Observed door state, committed by the bridge after each method.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.tunnel.schema@v1"))]
#[schemars(extend("x-oscal-category" = "network"))]
pub struct TunnelState {
    /// Host door address the mux listens on.
    pub door: String,
    /// Shared door socket bound into the door-owning container.
    pub socket: String,
    /// Whether new door connections are delivered.
    pub open: bool,
    /// Host uids of containers holding the door (one is the owner).
    pub waiting_uids: Vec<u32>,
    /// Accepts held open by the owner.
    pub waiting: u32,
    /// Door connections currently spliced.
    pub active: u32,
    /// Unix seconds of the oldest active connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_active_since: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetOpenInput {
    /// true delivers new door connections; false refuses them and drops every
    /// active connection.
    pub open: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CloseConnectionsOutput {
    /// Active connections dropped.
    pub closed: u32,
    pub state: TunnelState,
}

pub struct TunnelPlugin;

impl Default for TunnelPlugin {
    fn default() -> Self {
        Self
    }
}

#[async_trait]
impl StatePlugin for TunnelPlugin {
    fn name(&self) -> &str {
        "tunnel"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn schema(&self) -> Option<PluginSchema> {
        let mut schema = tunnel_schema();
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema)
    }
    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: "observed".into(),
                desired_hash: "observed".into(),
            },
        })
    }
    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        anyhow::ensure!(
            diff.actions.is_empty(),
            "tunnel state is observed; change it through set_open or close_connections"
        );
        Ok(ApplyResult { success: true, changes_applied: vec![], errors: vec![], checkpoint: None })
    }
    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }
    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::serde::to_owned_value(TunnelState::default())?,
            backend_checkpoint: None,
        })
    }
    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        anyhow::bail!("tunnel has no rollback surface; the door state is observed")
    }
    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: false,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

pub(crate) fn tunnel_schema() -> PluginSchema {
    use super::plugin_scaffold_helpers::{EmptyInput, method_decl_from_schemars_with_output};
    let root = serde_json::to_value(schemars::schema_for!(TunnelState)).expect("schema serializes");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "tunnel",
        "1.0.0",
        "Door tunnel: the host door spliced into the door-owning container's tonic door; status and open/close only, bytes stay on the socket",
        &root,
    );
    let example = simd_json::serde::to_owned_value(TunnelState {
        door: "10.0.0.2:8090".into(),
        socket: "/run/opdbus/tunnel/tunnel.sock".into(),
        open: true,
        ..TunnelState::default()
    })
    .expect("state serializes");
    super::schemars_adapter::apply_state_defaults(&mut schema, &example);
    schema.example = Some(example);
    schema.methods.insert(
        "status".into(),
        method_decl_from_schemars_with_output::<EmptyInput, TunnelState>(
            "status",
            SideEffect::Read,
            true,
            "tunnel.read",
            "obs.network.tunnel.status@v1",
        ),
    );
    schema.methods.insert(
        "set_open".into(),
        method_decl_from_schemars_with_output::<SetOpenInput, TunnelState>(
            "set_open",
            SideEffect::Mutation,
            true,
            "tunnel.admin",
            "mut.network.tunnel.set-open@v1",
        ),
    );
    schema.methods.insert(
        "close_connections".into(),
        method_decl_from_schemars_with_output::<EmptyInput, CloseConnectionsOutput>(
            "close_connections",
            SideEffect::Mutation,
            false,
            "tunnel.admin",
            "mut.network.tunnel.close-connections@v1",
        ),
    );
    for (id, description) in [
        ("tunnel.read", "Read the door tunnel's state."),
        ("tunnel.admin", "Open or close the door and drop its active connections."),
    ] {
        schema.capabilities.insert(
            id.into(),
            CapabilityDecl { id: id.into(), description: description.into() },
        );
    }
    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_status_and_controls_only() {
        let schema = tunnel_schema();
        let mut methods: Vec<&String> = schema.methods.keys().collect();
        methods.sort();
        assert_eq!(methods, ["close_connections", "set_open", "status"]);
        assert_eq!(schema.methods["status"].side_effect, SideEffect::Read);
    }
}

inventory::submit! {
    crate::default_registry::PluginReg::new("tunnel", |_ctx| std::sync::Arc::new(TunnelPlugin))
}
