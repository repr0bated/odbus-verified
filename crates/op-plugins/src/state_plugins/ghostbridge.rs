//! Ghostbridge plugin — bridge identity, TLS management, and Ghostrunner surface.
//!
//! Ghostbridge is the inward-facing bridge that extends zeroclaw's routing with
//! TLS-backed gRPC-Web connectivity, wireguard-pubkey identity, and the
//! Ghostrunner browser UI surface. It follows the same
//! `PluginSchema`/`StatePlugin` pattern as every other state plugin — no
//! centralized registry, no JSON-RPC, no SQL — just a 1:1 schema declaration.

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::PluginSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use simd_json::OwnedValue as Value;

// =============================================================================
// PLUGIN ENTRY: identity and typed schema seed
// =============================================================================

const PLUGIN_NAME: &str = "ghostbridge";
const PLUGIN_VERSION: &str = "1.0.0";
const PLUGIN_CATEGORY: &str = "network";
const PLUGIN_DESCRIPTION: &str =
    "Ghostbridge TLS/identity bridge for Ghostrunner UI, gRPC-Web surface, and WireGuard pubkey transport";
const PLUGIN_DISPLAY_NAME: &str = "Ghostbridge";

/// TLS certificate identity carried by the bridge.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.ghostbridge.identity.schema@v1"))]
pub struct BridgeIdentity {
    /// WireGuard public key identifying this bridge.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.network.ghostbridge.identity.wireguard-pubkey@v1"))]
    pub wireguard_pubkey: String,
    /// TLS certificate subject (CN/SAN).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.service.ghostbridge.identity.tls-subject@v1"))]
    pub tls_subject: String,
    /// TLS certificate DNS SANs.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.service.ghostbridge.identity.tls-sans@v1"))]
    pub tls_sans: Vec<String>,
    /// TLS certificate expiration timestamp (ISO 8601).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.service.ghostbridge.identity.tls-expires@v1"))]
    pub tls_expires: String,
    /// Whether the TLS cert was self-signed or CA-issued.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.service.ghostbridge.identity.tls-self-signed@v1"))]
    pub tls_self_signed: bool,
}

/// Ghostrunner — the browser-based bridge administration UI surface.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.ghostbridge.ghostrunner.schema@v1"))]
pub struct GhostrunnerSurface {
    /// TCP port the Ghostrunner web UI listens on.
    #[serde(default)]
    #[schemars(extend(
        "x-oscal-subid" = "mut.service.ghostbridge.ghostrunner.port@v1"
    ))]
    pub port: u16,
    /// Bind address for the Ghostrunner web UI.
    #[serde(default)]
    #[schemars(extend(
        "x-oscal-subid" = "mut.service.ghostbridge.ghostrunner.bind-addr@v1"
    ))]
    pub bind_addr: String,
    /// Base URL path prefix (e.g. "/ghostrunner").
    #[serde(default)]
    #[schemars(extend(
        "x-oscal-subid" = "mut.service.ghostbridge.ghostrunner.path@v1"
    ))]
    pub path: String,
    /// True when the Ghostrunner UI is enabled.
    #[serde(default)]
    #[schemars(extend(
        "x-oscal-subid" = "obs.service.ghostbridge.ghostrunner.enabled@v1"
    ))]
    pub enabled: bool,
}

/// A connected bridge endpoint.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.network.ghostbridge.endpoint.schema@v1"))]
pub struct BridgeEndpoint {
    /// Unique endpoint identifier.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.network.ghostbridge.endpoint.id@v1"))]
    pub id: String,
    /// Remote address (IP:port).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.network.ghostbridge.endpoint.address@v1"))]
    pub address: String,
    /// Endpoint status: active, connecting, disconnected.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.network.ghostbridge.endpoint.status@v1"))]
    pub status: String,
    /// Peer WireGuard public key.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.network.ghostbridge.endpoint.peer-pubkey@v1"))]
    pub peer_pubkey: String,
    /// Connection uptime in seconds.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.network.ghostbridge.endpoint.uptime@v1"))]
    pub uptime_secs: u64,
}

/// Root state for the Ghostbridge plugin.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.ghostbridge.state.schema@v1"))]
#[schemars(extend("x-oscal-category" = "network"))]
pub struct GhostbridgeState {
    /// Operational status: active, connecting, error.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.service.ghostbridge.status@v1"))]
    pub status: String,
    /// Observed bridge identity (aggregates TLS + WireGuard state).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.service.ghostbridge.identity@v1"))]
    pub bridge_identity: BridgeIdentity,
    /// Ghostrunner web UI surface config.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.ghostbridge.ghostrunner@v1"))]
    pub ghostrunner: GhostrunnerSurface,
    /// Connected bridge endpoints.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.network.ghostbridge.endpoints@v1"))]
    pub endpoints: Vec<BridgeEndpoint>,
}

// =============================================================================
// Input / Output types for D-Bus / gRPC methods
// =============================================================================

/// Empty input for read-only Ghostbridge methods.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EmptyGhostbridgeInput {}

/// Full Ghostbridge state output.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "exp.service.ghostbridge.state.result@v1"))]
pub struct GetStateOutput {
    pub state: GhostbridgeState,
}

/// Bridge identity output.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "obs.service.ghostbridge.identity.result@v1"))]
pub struct GetIdentityOutput {
    pub identity: BridgeIdentity,
}

/// Endpoints list output.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "obs.network.ghostbridge.endpoints.result@v1"))]
pub struct ListEndpointsOutput {
    pub endpoints: Vec<BridgeEndpoint>,
}

/// Ghostrunner surface output.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "obs.service.ghostbridge.ghostrunner.result@v1"))]
pub struct GetGhostrunnerOutput {
    pub ghostrunner: GhostrunnerSurface,
}

// =============================================================================
// PLUGIN BODY
// =============================================================================

pub struct GhostbridgePlugin;

impl Default for GhostbridgePlugin {
    fn default() -> Self {
        Self
    }
}

impl GhostbridgePlugin {
    const DEFAULT_GHOSTRUNNER_PORT: u16 = 8091;
    const DEFAULT_GHOSTRUNNER_BIND: &'static str = "127.0.0.1";
    const DEFAULT_GHOSTRUNNER_PATH: &'static str = "/ghostrunner";

    pub fn new() -> Self {
        Self
    }

    fn env_or(key: &str, fallback: &str) -> String {
        std::env::var(key).unwrap_or_else(|_| fallback.to_string())
    }

    pub fn current_state() -> GhostbridgeState {
        let wg_pubkey = Self::env_or("GHOSTBRIDGE_WG_PUBKEY", "");
        let tls_subject = Self::env_or("GHOSTBRIDGE_TLS_SUBJECT", "ghostbridge.tech");
        let tls_sans = Self::env_or(
            "GHOSTBRIDGE_TLS_SANS",
            "localhost,ghostbridge.tech,3tched.com",
        );
        let tls_expires = Self::env_or("GHOSTBRIDGE_TLS_EXPIRES", "");
        let tls_self_signed = std::env::var("GHOSTBRIDGE_TLS_SELF_SIGNED")
            .map(|v| v == "1" || v == "true")
            .unwrap_or(true);
        let ghostrunner_port: u16 = std::env::var("GHOSTBRIDGE_UI_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(Self::DEFAULT_GHOSTRUNNER_PORT);
        let ghostrunner_bind = Self::env_or("GHOSTBRIDGE_UI_BIND", Self::DEFAULT_GHOSTRUNNER_BIND);
        let ghostrunner_path = Self::env_or("GHOSTBRIDGE_UI_PATH", Self::DEFAULT_GHOSTRUNNER_PATH);
        let ghostrunner_enabled = std::env::var("GHOSTBRIDGE_UI_ENABLED")
            .map(|v| v == "1" || v == "true")
            .unwrap_or(true);

        GhostbridgeState {
            status: "declared".to_string(),
            bridge_identity: BridgeIdentity {
                wireguard_pubkey: wg_pubkey,
                tls_subject,
                tls_sans: tls_sans.split(',').map(|s| s.trim().to_string()).collect(),
                tls_expires,
                tls_self_signed,
            },
            ghostrunner: GhostrunnerSurface {
                port: ghostrunner_port,
                bind_addr: ghostrunner_bind,
                path: ghostrunner_path,
                enabled: ghostrunner_enabled,
            },
            endpoints: vec![],
        }
    }
}

#[async_trait]
impl StatePlugin for GhostbridgePlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn schema(&self) -> Option<PluginSchema> {
        let schema = ghostbridge_schema();
        Some(schema)
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: PLUGIN_NAME.to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: "schema-declared".to_string(),
                desired_hash: "schema-declared".to_string(),
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
            id: uuid::Uuid::new_v4().to_string(),
            plugin: PLUGIN_NAME.to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::serde::to_owned_value(Self::current_state())?,
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

// =============================================================================
// PLUGIN EXIT: publish the single PluginSchema contract
// =============================================================================

/// Canonical `ghostbridge` schema derived from [`GhostbridgeState`] via schemars.
pub(crate) fn ghostbridge_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(GhostbridgeState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        PLUGIN_NAME,
        PLUGIN_VERSION,
        PLUGIN_DESCRIPTION,
        &root,
    );
    schema.category = PLUGIN_CATEGORY.to_string();
    schema.display_name = Some(PLUGIN_DISPLAY_NAME.to_string());

    if let Ok(state) = simd_json::serde::to_owned_value(GhostbridgePlugin::current_state()) {
        super::schemars_adapter::apply_state_defaults(&mut schema, &state);
        schema.example = Some(state);
    }

    schema.methods.insert(
        "GetState".to_string(),
        method_decl_from_schemars_with_output::<EmptyGhostbridgeInput, GetStateOutput>(
            "GetState",
            op_state_store::SideEffect::Read,
            true,
            "cap.service.ghostbridge.state.read@v1",
            "obs.service.ghostbridge.state.get@v1",
        ),
    );
    schema.methods.insert(
        "GetIdentity".to_string(),
        method_decl_from_schemars_with_output::<EmptyGhostbridgeInput, GetIdentityOutput>(
            "GetIdentity",
            op_state_store::SideEffect::Read,
            true,
            "cap.service.ghostbridge.identity.read@v1",
            "obs.service.ghostbridge.identity.get@v1",
        ),
    );
    schema.methods.insert(
        "ListEndpoints".to_string(),
        method_decl_from_schemars_with_output::<EmptyGhostbridgeInput, ListEndpointsOutput>(
            "ListEndpoints",
            op_state_store::SideEffect::Read,
            true,
            "cap.service.ghostbridge.endpoints.read@v1",
            "obs.network.ghostbridge.endpoints.list@v1",
        ),
    );
    schema.methods.insert(
        "GetGhostrunner".to_string(),
        method_decl_from_schemars_with_output::<EmptyGhostbridgeInput, GetGhostrunnerOutput>(
            "GetGhostrunner",
            op_state_store::SideEffect::Read,
            true,
            "cap.service.ghostbridge.ghostrunner.read@v1",
            "obs.service.ghostbridge.ghostrunner.get@v1",
        ),
    );

    schema.capabilities.insert(
        "cap.service.ghostbridge.state.read@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.service.ghostbridge.state.read@v1".to_string(),
            description: "Grants: GetState.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.service.ghostbridge.identity.read@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.service.ghostbridge.identity.read@v1".to_string(),
            description: "Grants: GetIdentity.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.service.ghostbridge.endpoints.read@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.service.ghostbridge.endpoints.read@v1".to_string(),
            description: "Grants: ListEndpoints.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.service.ghostbridge.ghostrunner.read@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.service.ghostbridge.ghostrunner.read@v1".to_string(),
            description: "Grants: GetGhostrunner.".to_string(),
        },
    );

    schema
}

/// Public accessor for crates that embed the Ghostbridge plugin contract.
pub fn ghostbridge_plugin_schema() -> PluginSchema {
    ghostbridge_schema()
}

/// Plugin-owned method dispatch for the Ghostbridge D-Bus/gRPC surface.
///
/// Keeping these handlers beside the schema makes the contract executable:
/// every method declared by [`ghostbridge_schema`] has a domain result instead
/// of falling through to the bridge's generic argument echo.
pub fn dispatch_ghostbridge_method(method: &str, state: &GhostbridgeState) -> Result<JsonValue> {
    match method {
        "GetState" => Ok(serde_json::json!({ "state": state })),
        "GetIdentity" => Ok(serde_json::json!({
            "identity": &state.bridge_identity
        })),
        "ListEndpoints" => Ok(serde_json::json!({
            "endpoints": &state.endpoints
        })),
        "GetGhostrunner" => Ok(serde_json::json!({
            "ghostrunner": &state.ghostrunner
        })),
        other => Err(anyhow::anyhow!(
            "ghostbridge method is not declared: {other}"
        )),
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new(PLUGIN_NAME, |_ctx| std::sync::Arc::new(GhostbridgePlugin::new()))
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_plugins::common::oscal::validate_subid;

    fn collect_subids(node: &serde_json::Value, out: &mut Vec<String>) {
        if let Some(subid) = node
            .get("x-oscal-subid")
            .and_then(serde_json::Value::as_str)
        {
            out.push(subid.to_string());
        }
        if let Some(obj) = node.as_object() {
            for v in obj.values() {
                collect_subids(v, out);
            }
        }
        if let Some(arr) = node.as_array() {
            for v in arr {
                collect_subids(v, out);
            }
        }
    }

    #[test]
    fn all_subids_are_valid() {
        let raw = serde_json::to_value(schemars::schema_for!(GhostbridgeState)).unwrap();
        let mut subids = Vec::new();
        collect_subids(&raw, &mut subids);
        assert!(!subids.is_empty(), "expected at least one x-oscal-subid");
        for subid in subids {
            validate_subid(&subid).unwrap_or_else(|_| panic!("invalid subid: {subid}"));
        }
    }

    #[test]
    fn public_schema_accessor_returns_ghostbridge_schema() {
        let schema = ghostbridge_plugin_schema();
        assert_eq!(schema.name, PLUGIN_NAME);
        assert_eq!(schema.version, PLUGIN_VERSION);
        assert_eq!(schema.display_name, Some(PLUGIN_DISPLAY_NAME.to_string()));
        assert_eq!(schema.methods.len(), 4);
    }

    #[test]
    fn current_state_has_defaults() {
        let state = GhostbridgePlugin::current_state();
        assert_eq!(state.status, "declared");
        assert_eq!(state.ghostrunner.port, 8091);
        assert_eq!(state.ghostrunner.path, "/ghostrunner");
    }

    #[test]
    fn every_declared_method_has_a_domain_dispatcher() {
        let schema = ghostbridge_plugin_schema();
        let state = GhostbridgePlugin::current_state();

        for method in schema.methods.keys() {
            let result = dispatch_ghostbridge_method(method, &state)
                .unwrap_or_else(|error| panic!("{method} is not executable: {error}"));
            assert!(result.is_object(), "{method} must return an object");
        }
    }

    #[test]
    fn undeclared_method_is_rejected() {
        let error = dispatch_ghostbridge_method("NotDeclared", &GhostbridgePlugin::current_state())
            .unwrap_err();
        assert!(error.to_string().contains("not declared"));
    }
}
