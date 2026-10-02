//! The bridge-owned shared tonic listener. Registration metadata belongs to
//! shared_unix_socket; this plugin never creates an independent listener.

use anyhow::{Result, bail};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema, SideEffect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue;

pub const SHARED_CONTAINER_SOCKET: &str = "/run/ghostbridge/container.sock";
fn default_protocol() -> String {
    "grpc".into()
}
fn default_backlog() -> u32 {
    128
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BindInput {
    /// Must equal the bridge's configured shared container socket path.
    pub path: String,
    #[serde(default = "default_protocol")]
    pub protocol: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListenInput {
    pub path: String,
    /// Requested kernel backlog, 1..=65535; the kernel may cap it.
    #[serde(default = "default_backlog")]
    pub backlog: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptInput {
    pub path: String,
    /// Observe tonic's next accepted transport connection after this cursor.
    /// Omission starts after the current tail; no socket/FD is transferred.
    #[serde(default)]
    pub after_sequence: Option<u64>,
    /// Supply a previous observation's generation to detect bridge restart.
    #[serde(default)]
    pub generation: Option<String>,
    /// Bounded wait, 0..=60 seconds. Omission is a nonblocking observation.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CloseInput {
    pub path: String,
    /// Shut down actual active connections as well as the listener. A caller
    /// using this socket can lose its own response; D-Bus/TCP remain available.
    #[serde(default)]
    pub force: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.unix-socket.schema@v1"))]
#[schemars(extend("x-oscal-category" = "service"))]
pub struct UnixSocketState {
    #[schemars(extend("readOnly" = true))]
    pub path: String,
    #[schemars(extend("readOnly" = true))]
    pub protocol: String,
    /// closed, bound, or listening; reported by the actual listener owner.
    #[schemars(extend("readOnly" = true))]
    pub phase: String,
    #[schemars(extend("readOnly" = true))]
    pub generation: String,
    #[schemars(extend("readOnly" = true))]
    pub binding_generation: u64,
    #[schemars(extend("readOnly" = true))]
    pub requested_backlog: Option<u32>,
    #[schemars(extend("readOnly" = true))]
    pub active_connections: u64,
    #[schemars(extend("readOnly" = true))]
    pub accepted_connections: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AcceptedConnection {
    pub sequence: u64,
    /// Observation identifier only; never a usable descriptor or session ID.
    pub connection_id: String,
    pub binding_generation: u64,
    pub accepted_at_unix_ms: u64,
    /// Kernel peer credentials, not proof of authenticated SID1/OIA1 identity.
    pub peer_uid: Option<u32>,
    pub peer_gid: Option<u32>,
    pub peer_pid: Option<i32>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AcceptOutput {
    pub event: Option<AcceptedConnection>,
    pub observed_generation: String,
    pub cursor: u64,
    /// True only when a positive requested wait expires without an event.
    pub timed_out: bool,
}

pub fn unix_socket_schema_derived() -> PluginSchema {
    use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output as method;
    let root = serde_json::to_value(schemars::schema_for!(UnixSocketState)).expect("socket schema");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "unix_socket",
        "2.0.0",
        "Lifecycle and connection observations for the bridge-owned shared tonic listener",
        &root,
    );
    for (name, decl) in [
        (
            "bind",
            method::<BindInput, UnixSocketState>(
                "bind",
                SideEffect::Mutation,
                false,
                "network.write",
                "mut.service.unix-socket.bind@v1",
            ),
        ),
        (
            "listen",
            method::<ListenInput, UnixSocketState>(
                "listen",
                SideEffect::Mutation,
                false,
                "network.write",
                "mut.service.unix-socket.listen@v1",
            ),
        ),
        (
            "accept",
            method::<AcceptInput, AcceptOutput>(
                "accept",
                SideEffect::Read,
                true,
                "network.read",
                "obs.service.unix-socket.accept@v1",
            ),
        ),
        (
            "close",
            method::<CloseInput, UnixSocketState>(
                "close",
                SideEffect::Mutation,
                false,
                "network.write",
                "mut.service.unix-socket.close@v1",
            ),
        ),
    ] {
        schema.methods.insert(name.into(), decl);
    }
    for (id, description) in [
        ("network.write", "Grants: bind, listen, close."),
        (
            "network.read",
            "Grants: observe accepted transport connections.",
        ),
    ] {
        schema.capabilities.insert(
            id.into(),
            CapabilityDecl {
                id: id.into(),
                description: description.into(),
            },
        );
    }
    super::common::oscal::ensure_category_metadata_fields(&mut schema);
    schema
}

#[derive(Default)]
pub struct UnixSocketPlugin;
impl UnixSocketPlugin {
    pub fn new() -> Self {
        Self
    }
}
#[async_trait]
impl StatePlugin for UnixSocketPlugin {
    fn name(&self) -> &str {
        "unix_socket"
    }
    fn version(&self) -> &str {
        "2.0.0"
    }
    fn schema(&self) -> Option<PluginSchema> {
        Some(unix_socket_schema_derived())
    }
    async fn calculate_diff(&self, _: &OwnedValue, _: &OwnedValue) -> Result<StateDiff> {
        bail!("listener lifecycle is controlled by the bridge-owned plugin methods")
    }
    async fn apply_state(&self, _: &StateDiff) -> Result<ApplyResult> {
        bail!("listener state cannot be applied outside its bridge owner")
    }
    async fn verify_state(&self, _: &OwnedValue) -> Result<bool> {
        bail!("listener verification requires the bridge-owned controller")
    }
    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        bail!("live transport connections cannot be checkpointed")
    }
    async fn rollback(&self, _: &Checkpoint) -> Result<()> {
        bail!("live transport connections cannot be rolled back")
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
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schema_is_typed_and_keeps_only_listener_inputs() {
        let schema = unix_socket_schema_derived();
        assert_eq!(schema.version, "2.0.0");
        assert_eq!(schema.methods.len(), 4);
        assert!(
            schema
                .methods
                .values()
                .all(|method| method.returns.is_some())
        );
        assert!(
            serde_json::from_value::<BindInput>(
                serde_json::json!({"path":"/tmp/x","name":"ignored"})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<AcceptInput>(serde_json::json!({"path":"/tmp/x"})).is_ok()
        );
    }
}
inventory::submit! {
    crate::default_registry::PluginReg::new("unix_socket", |_ctx| std::sync::Arc::new(UnixSocketPlugin::new()))
}
