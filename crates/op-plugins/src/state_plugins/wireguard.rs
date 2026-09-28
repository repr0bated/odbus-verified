//! Canonical schema for decoy-owned Linux kernel WireGuard administration.

use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;

const PLUGIN_NAME: &str = "wireguard";
const PLUGIN_VERSION: &str = "2.0.0";
const PLUGIN_CATEGORY: &str = "network";
const PLUGIN_DESCRIPTION: &str = "Decoy-owned Linux kernel WireGuard interface state";
const PLUGIN_DISPLAY_NAME: &str = "GB.Wireguard";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.network.plugin.wireguard.schema@v2"))]
#[schemars(extend("x-oscal-category" = "network"))]
pub struct WireGuardState {
    /// Complete sanitized kernel readback for allowlisted interfaces.
    #[serde(default)]
    #[schemars(extend("x-opdbus-exact-json-schema" = true))]
    pub interfaces: Vec<WireGuardInterface>,
}

/// Sanitized kernel interface readback. Secret keys are structurally absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WireGuardInterface {
    pub name: String,
    pub public_key: Option<String>,
    pub listen_port: Option<u16>,
    pub fwmark: Option<u32>,
    #[serde(default)]
    #[schemars(extend("x-opdbus-exact-json-schema" = true))]
    pub peers: Vec<WireGuardPeer>,
}

/// Sanitized kernel peer readback. The preshared key is never represented.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WireGuardPeer {
    pub public_key: String,
    #[serde(default)]
    #[schemars(extend("x-opdbus-exact-json-schema" = true))]
    pub allowed_ips: Vec<String>,
    pub endpoint: Option<String>,
    pub persistent_keepalive: Option<u16>,
    pub latest_handshake_unix: Option<i64>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WireGuardPeerConfig {
    pub public_key: String,
    #[serde(default)]
    #[schemars(extend("x-opdbus-exact-json-schema" = true))]
    pub allowed_ips: Vec<String>,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub persistent_keepalive: Option<u16>,
    #[serde(default)]
    #[schemars(extend("x-secret" = true))]
    pub preshared_key: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WireGuardDeviceConfig {
    #[serde(default)]
    #[schemars(extend("x-secret" = true))]
    pub private_key: Option<String>,
    #[serde(default)]
    pub listen_port: Option<u16>,
    #[serde(default)]
    pub fwmark: Option<u32>,
    #[serde(default)]
    #[schemars(extend("x-opdbus-exact-json-schema" = true))]
    pub peers: Vec<WireGuardPeerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetDeviceInput {
    pub interface: String,
    #[serde(default)]
    #[schemars(extend("x-secret" = true))]
    pub private_key: Option<String>,
    #[serde(default)]
    pub listen_port: Option<u16>,
    #[serde(default)]
    pub fwmark: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetDeviceInput {
    pub interface: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetDeviceOutput {
    pub interface: Option<WireGuardInterface>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetDeviceOutput {
    pub interface: WireGuardInterface,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AddPeerInput {
    pub interface: String,
    #[serde(flatten)]
    pub peer: WireGuardPeerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemovePeerInput {
    pub interface: String,
    pub public_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListPeersInput {
    pub interface: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListPeersOutput {
    #[schemars(extend("x-opdbus-exact-json-schema" = true))]
    pub peers: Vec<WireGuardPeer>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PeerMutationOutput {
    pub interface: WireGuardInterface,
    pub peer: Option<WireGuardPeer>,
    pub changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetConfigInput {
    pub interface: String,
    #[schemars(extend("x-opdbus-exact-json-schema" = true))]
    pub config: WireGuardDeviceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetConfigOutput {
    pub interface: WireGuardInterface,
}

pub struct WireGuardPlugin;
impl Default for WireGuardPlugin {
    fn default() -> Self {
        Self
    }
}
impl WireGuardPlugin {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StatePlugin for WireGuardPlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }
    fn version(&self) -> &str {
        PLUGIN_VERSION
    }
    fn schema(&self) -> Option<PluginSchema> {
        Some(wireguard_schema())
    }
    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: PLUGIN_NAME.into(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: "unknown".into(),
                desired_hash: "unknown".into(),
            },
        })
    }
    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        anyhow::bail!("WireGuard effects are accepted only by the authenticated decoy D-Bus owner")
    }
    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        anyhow::bail!("WireGuard verification requires decoy kernel readback")
    }
    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        anyhow::bail!("WireGuard checkpointing is not supported")
    }
    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        anyhow::bail!("WireGuard rollback is not supported")
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

pub fn wireguard_schema() -> PluginSchema {
    let root =
        serde_json::to_value(schemars::schema_for!(WireGuardState)).expect("schema serializes");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        PLUGIN_NAME,
        PLUGIN_VERSION,
        PLUGIN_DESCRIPTION,
        &root,
    );
    schema.category = PLUGIN_CATEGORY.into();
    schema.display_name = Some(PLUGIN_DISPLAY_NAME.into());
    macro_rules! method {
        ($id:literal, $input:ty, $output:ty, $effect:expr_2021, $idem:expr_2021, $cap:literal, $subid:literal) => {
            schema.methods.insert(
                $id.into(),
                method_decl_from_schemars_with_output::<$input, $output>(
                    $id, $effect, $idem, $cap, $subid,
                ),
            );
        };
    }
    method!(
        "set_device",
        SetDeviceInput,
        SetDeviceOutput,
        SideEffect::Mutation,
        false,
        "wireguard.write",
        "mut.network.wireguard.device.set@v2"
    );
    method!(
        "get_device",
        GetDeviceInput,
        GetDeviceOutput,
        SideEffect::Read,
        true,
        "wireguard.read",
        "obs.network.wireguard.device.get@v2"
    );
    method!(
        "add_peer",
        AddPeerInput,
        PeerMutationOutput,
        SideEffect::Mutation,
        false,
        "wireguard.write",
        "mut.network.wireguard.peer.add@v2"
    );
    method!(
        "remove_peer",
        RemovePeerInput,
        PeerMutationOutput,
        SideEffect::Mutation,
        false,
        "wireguard.write",
        "mut.network.wireguard.peer.remove@v2"
    );
    method!(
        "list_peers",
        ListPeersInput,
        ListPeersOutput,
        SideEffect::Read,
        true,
        "wireguard.read",
        "obs.network.wireguard.peers.list@v2"
    );
    method!(
        "set_config",
        SetConfigInput,
        SetConfigOutput,
        SideEffect::Mutation,
        false,
        "wireguard.write",
        "mut.network.wireguard.config.set@v2"
    );
    schema.capabilities.insert(
        "wireguard.write".into(),
        CapabilityDecl {
            id: "wireguard.write".into(),
            description: "Grants: set_device, add_peer, remove_peer, set_config.".into(),
        },
    );
    schema.capabilities.insert(
        "wireguard.read".into(),
        CapabilityDecl {
            id: "wireguard.read".into(),
            description: "Grants: get_device, list_peers.".into(),
        },
    );
    schema
}

inventory::submit! { crate::default_registry::PluginReg::new(PLUGIN_NAME, |_ctx| std::sync::Arc::new(WireGuardPlugin::new())) }
