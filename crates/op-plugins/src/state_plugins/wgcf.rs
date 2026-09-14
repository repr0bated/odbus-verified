//! Decoy-side Cloudflare WARP account and WireGuard profile contract.
use super::plugin_scaffold_helpers::{method_decl_from_schemars_with_output, EmptyInput};
use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

pub use super::decoy_network::wgcf_owner as owner;
use super::wireguard::WireGuardState;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WgcfConfig {
    #[serde(default = "default_interface")]
    pub interface: String,
}
fn default_interface() -> String {
    "wgcf".into()
}
impl Default for WgcfConfig {
    fn default() -> Self {
        Self {
            interface: default_interface(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WgcfAccountView {
    pub id: String,
    pub account_type: String,
    pub role: String,
    pub warp_plus: bool,
    pub quota: f64,
    pub premium_data: f64,
    pub referral_count: f64,
    pub referral_renewal_countdown: f64,
    pub created: String,
    pub updated: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WgcfDeviceView {
    pub id: String,
    pub enabled: bool,
    pub name: Option<String>,
    pub model: Option<String>,
    #[serde(rename = "type")]
    pub device_type: String,
    pub created: String,
    pub updated: String,
    pub warp_enabled: bool,
    pub public_key: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WgcfProfileView {
    pub profile_id: String,
    pub interface: String,
    pub addresses: Vec<String>,
    /// Resolver values retained in the portable profile. `apply_config`
    /// applies only WireGuard device/link state and does not alter DNS.
    pub dns: Vec<String>,
    pub peer_public_key: String,
    pub endpoint: String,
    pub mtu: Option<u32>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WgcfStatusOutput {
    pub revision: u64,
    pub account: WgcfAccountView,
    pub device: WgcfDeviceView,
    pub active_profile: Option<WgcfProfileView>,
    /// Complete sanitized shared WireGuard-owner state when observed.
    #[schemars(extend("x-opdbus-exact-json-schema" = true))]
    pub kernel: Option<WireGuardState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegisterInput {
    pub accept_tos: bool,
    pub device_model: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateInput {
    #[schemars(extend("x-secret" = true))]
    pub license: Option<String>,
    pub device_name: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetEndpointInput {
    pub endpoint: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateConfigInput {
    #[serde(default = "default_interface")]
    pub interface: String,
    #[serde(default)]
    pub dns: Vec<String>,
    pub mtu: Option<u32>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateConfigOutput {
    pub revision: u64,
    pub profile: WgcfProfileView,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyConfigInput {
    pub profile_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyConfigOutput {
    pub revision: u64,
    /// The retained profile reference; DNS is descriptive and is not part of
    /// the kernel application receipt.
    pub profile: WgcfProfileView,
    /// Sanitized WireGuard kernel readback after device/link application.
    #[schemars(extend("x-opdbus-exact-json-schema" = true))]
    pub kernel: WireGuardState,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RefreshKind {
    Account,
    Device,
    All,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RefreshInput {
    pub kind: RefreshKind,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-oscal-subid"="sch.software.plugin.wgcf.schema@v2"))]
pub struct WgcfState {
    #[serde(default)]
    pub config: WgcfConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<WgcfStatusOutput>,
    #[serde(default)]
    pub profiles: Vec<WgcfProfileView>,
}

pub struct WgcfPlugin {
    _config: WgcfConfig,
}
impl WgcfPlugin {
    pub fn new(config: WgcfConfig) -> Self {
        Self { _config: config }
    }
    pub(crate) fn current_state() -> WgcfState {
        WgcfState::default()
    }
}
#[async_trait]
impl StatePlugin for WgcfPlugin {
    fn name(&self) -> &'static str {
        "wgcf"
    }
    fn version(&self) -> &str {
        "2.0.0"
    }
    fn schema(&self) -> Option<PluginSchema> {
        let mut s = wgcf_schema();
        super::common::oscal::ensure_category_metadata_fields(&mut s);
        Some(s)
    }
    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: false,
            supports_verification: false,
            atomic_operations: false,
        }
    }
    async fn calculate_diff(&self, _: &Value, _: &Value) -> Result<StateDiff> {
        anyhow::bail!("wgcf uses admitted native methods")
    }
    async fn apply_state(&self, _: &StateDiff) -> Result<ApplyResult> {
        anyhow::bail!("wgcf uses admitted native methods")
    }
    async fn verify_state(&self, _: &Value) -> Result<bool> {
        anyhow::bail!("wgcf uses admitted native methods")
    }
    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        anyhow::bail!("wgcf does not support checkpoints")
    }
    async fn rollback(&self, _: &Checkpoint) -> Result<()> {
        anyhow::bail!("wgcf does not support rollback")
    }
}

pub(crate) fn wgcf_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(WgcfState)).unwrap();
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "wgcf",
        "2.0.0",
        "Decoy-owned Cloudflare WARP account and kernel-applied WireGuard profiles; generated DNS values are retained but not installed",
        &root,
    );
    super::schemars_adapter::apply_state_defaults(
        &mut schema,
        &simd_json::serde::to_owned_value(WgcfState::default()).unwrap(),
    );
    macro_rules! m {
        ($id:literal,$i:ty,$o:ty,$e:ident,$cap:literal,$sub:literal) => {
            schema.methods.insert(
                $id.into(),
                method_decl_from_schemars_with_output::<$i, $o>(
                    $id,
                    SideEffect::$e,
                    matches!(SideEffect::$e, SideEffect::Read),
                    $cap,
                    $sub,
                ),
            );
        };
    }
    m!(
        "register",
        RegisterInput,
        WgcfStatusOutput,
        Mutation,
        "wgcf.register",
        "mut.software.plugin.wgcf.register@v2"
    );
    m!(
        "update",
        UpdateInput,
        WgcfStatusOutput,
        Mutation,
        "wgcf.write",
        "mut.software.plugin.wgcf.update@v2"
    );
    m!(
        "status",
        EmptyInput,
        WgcfStatusOutput,
        Read,
        "wgcf.read",
        "obs.software.plugin.wgcf.status@v2"
    );
    m!(
        "rotate_keys",
        EmptyInput,
        WgcfStatusOutput,
        Mutation,
        "wgcf.write",
        "mut.software.plugin.wgcf.keys.rotate@v2"
    );
    m!(
        "set_endpoint",
        SetEndpointInput,
        GenerateConfigOutput,
        Mutation,
        "wgcf.write",
        "mut.software.plugin.wgcf.endpoint.set@v2"
    );
    m!(
        "generate_config",
        GenerateConfigInput,
        GenerateConfigOutput,
        Mutation,
        "wgcf.write",
        "mut.software.plugin.wgcf.config.generate@v2"
    );
    m!(
        "apply_config",
        ApplyConfigInput,
        ApplyConfigOutput,
        Mutation,
        "wgcf.write",
        "mut.software.plugin.wgcf.config.apply@v2"
    );
    m!(
        "refresh",
        RefreshInput,
        WgcfStatusOutput,
        Mutation,
        "wgcf.write",
        "mut.software.plugin.wgcf.refresh@v2"
    );
    for (id, methods) in [
        ("wgcf.read", "status"),
        ("wgcf.register", "register"),
        (
            "wgcf.write",
            "update, rotate_keys, set_endpoint, generate_config, apply_config, refresh",
        ),
    ] {
        schema.capabilities.insert(
            id.into(),
            op_state_store::CapabilityDecl {
                id: id.into(),
                description: format!("Grants: {methods}."),
            },
        );
    }
    schema
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_state::StatePlugin;
    #[test]
    fn eight_typed_methods_never_project_secrets() {
        let s = WgcfPlugin::new(WgcfConfig::default()).schema().unwrap();
        assert_eq!(s.version, "2.0.0");
        assert_eq!(s.methods.len(), 8);
        let j = serde_json::to_string(&s).unwrap();
        assert!(!j.contains("private_key"));
        assert!(!j.contains("access_token"));
        assert!(j.contains("accept_tos"));
    }
}
inventory::submit! {crate::default_registry::PluginReg::new("wgcf",|_ctx|std::sync::Arc::new(WgcfPlugin::new(WgcfConfig::default())))}
