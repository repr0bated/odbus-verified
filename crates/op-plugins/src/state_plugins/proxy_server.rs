use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{PluginSchema, SideEffect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.network.proxy-server.schema@v1"))]
#[schemars(extend("x-oscal-category" = "network"))]
pub struct ProxyServerState {
    pub revision: u64,
    #[schemars(extend("x-opdbus-exact-json-schema" = true))]
    pub proxies: std::collections::BTreeMap<String, ProxyStatus>,
}

/// A named, fixed-destination TCP relay. This is not a forward HTTP proxy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProxyConfig {
    pub enabled: bool,
    /// Literal local IP address; host names are not accepted for binding.
    pub bind_address: String,
    #[schemars(range(min = 1, max = 65535))]
    pub port: u16,
    /// Fixed literal IP:port destination (IPv6 uses brackets).
    pub upstream: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProxyStatus {
    pub config: ProxyConfig,
    pub running: bool,
    pub bound_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProxyMethodOutput {
    pub name: String,
    pub revision: u64,
    pub status: ProxyStatus,
}

/// StartProxy method input
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StartProxyInput {
    /// Proxy server identifier
    pub name: String,
    /// Port to bind to
    pub port: Option<u16>,
    pub expected_revision: u64,
}

/// StopProxy method input
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StopProxyInput {
    /// Proxy server identifier
    pub name: String,
    pub expected_revision: u64,
}

/// ConfigureProxy method input
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigureProxyInput {
    /// Proxy server identifier
    pub name: String,
    /// Enable/disable the proxy
    pub enabled: Option<bool>,
    pub bind_address: Option<String>,
    /// Port number
    pub port: Option<u16>,
    /// Fixed literal upstream IP:port; no URL credentials or request-time destination.
    pub upstream: Option<String>,
    pub expected_revision: u64,
}

pub struct ProxyServerPlugin;

impl Default for ProxyServerPlugin {
    fn default() -> Self {
        Self
    }
}

impl ProxyServerPlugin {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StatePlugin for ProxyServerPlugin {
    fn name(&self) -> &str {
        "proxy_server"
    }

    fn version(&self) -> &str {
        "2.0.0"
    }

    fn schema(&self) -> Option<op_state_store::PluginSchema> {
        Some(proxy_server_schema())
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        anyhow::bail!("proxy_server reconciliation has no authoritative backend")
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        anyhow::bail!("proxy_server apply has no authoritative backend")
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        anyhow::bail!("proxy_server verification requires authoritative readback")
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        anyhow::bail!("proxy_server has no authoritative checkpoint owner")
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        anyhow::bail!("proxy_server has no authoritative rollback owner")
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

pub(crate) fn proxy_server_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(ProxyServerState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "proxy_server",
        "2.0.0",
        "Named fixed-destination TCP relays; configure durable intent, explicitly start or stop listeners",
        &root,
    );

    // The owning relay implements these contracts; no external proxy spec is implied.
    schema.methods.insert(
        "start_proxy".to_string(),
        method_decl_from_schemars_with_output::<StartProxyInput, ProxyMethodOutput>(
            "start_proxy",
            SideEffect::Mutation,
            false,
            "cap.network.proxy.start@v1",
            "mut.network.proxy.start@v1",
        ),
    );
    schema.methods.insert(
        "stop_proxy".to_string(),
        method_decl_from_schemars_with_output::<StopProxyInput, ProxyMethodOutput>(
            "stop_proxy",
            SideEffect::Mutation,
            false,
            "cap.network.proxy.stop@v1",
            "mut.network.proxy.stop@v1",
        ),
    );
    schema.methods.insert(
        "configure".to_string(),
        method_decl_from_schemars_with_output::<ConfigureProxyInput, ProxyMethodOutput>(
            "configure",
            SideEffect::Mutation,
            false,
            "cap.network.proxy.configure@v1",
            "mut.network.proxy.configure@v1",
        ),
    );

    schema.capabilities.insert(
        "cap.network.proxy.start@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.network.proxy.start@v1".to_string(),
            description: "Grants: start_proxy.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.network.proxy.stop@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.network.proxy.stop@v1".to_string(),
            description: "Grants: stop_proxy.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.network.proxy.configure@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.network.proxy.configure@v1".to_string(),
            description: "Grants: configure.".to_string(),
        },
    );

    schema
}

#[cfg(test)]
mod fail_closed_tests {
    use super::*;

    #[tokio::test]
    async fn generic_lifecycle_never_returns_a_false_receipt() {
        let plugin = ProxyServerPlugin::new();
        let state = simd_json::json!({});
        assert!(plugin.calculate_diff(&state, &state).await.is_err());
        let diff = StateDiff {
            plugin: plugin.name().to_owned(),
            actions: vec![],
            metadata: op_state::DiffMetadata {
                timestamp: 0,
                current_hash: String::new(),
                desired_hash: String::new(),
            },
        };
        assert!(plugin.apply_state(&diff).await.is_err());
        assert!(plugin.verify_state(&state).await.is_err());
        assert!(plugin.create_checkpoint().await.is_err());
        let checkpoint = Checkpoint {
            id: "fixture".into(),
            plugin: plugin.name().into(),
            timestamp: 0,
            state_snapshot: state,
            backend_checkpoint: None,
        };
        assert!(plugin.rollback(&checkpoint).await.is_err());
        let capabilities = plugin.capabilities();
        assert!(!capabilities.supports_verification);
        assert!(!capabilities.supports_checkpoints);
        assert!(!capabilities.supports_rollback);
        assert!(!capabilities.atomic_operations);
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("proxy_server", |_ctx| std::sync::Arc::new(ProxyServerPlugin::new()))
}
