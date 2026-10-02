//! A.N.N.A. Scribe façade over the single decoy WireGuard kernel owner.

use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::collections::BTreeMap;

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
use super::wireguard::{WireGuardDeviceConfig, WireGuardInterface};

const PLUGIN_NAME: &str = "wg_opdbus";
const PLUGIN_VERSION: &str = "2.0.0";
const PLUGIN_CATEGORY: &str = "network";
const PLUGIN_DESCRIPTION: &str = "A.N.N.A. Scribe identity binding on the decoy WireGuard owner";
const PLUGIN_DISPLAY_NAME: &str = "A.N.N.A. Scribe WireGuard Identity";

pub const ANNA_SCRIBE_PRINCIPAL_ID: &str = "did:op:service:anna-scribe";
pub const ANNA_SCRIBE_PERSONA_ID: &str = "anna-scribe";
pub const ANNA_SCRIBE_EMAIL: &str = "anna.scribe@3tched.com";
pub const ANNA_SCRIBE_PLUGIN_REF: &str = PLUGIN_NAME;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct AnnaScribeServicePersona {
    principal_id: String,
    persona_id: String,
    email: String,
    plugin_ref: String,
}

impl Default for AnnaScribeServicePersona {
    fn default() -> Self {
        Self {
            principal_id: ANNA_SCRIBE_PRINCIPAL_ID.into(),
            persona_id: ANNA_SCRIBE_PERSONA_ID.into(),
            email: ANNA_SCRIBE_EMAIL.into(),
            plugin_ref: ANNA_SCRIBE_PLUGIN_REF.into(),
        }
    }
}

impl<'de> Deserialize<'de> for AnnaScribeServicePersona {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireIdentity {
            principal_id: String,
            persona_id: String,
            email: String,
            plugin_ref: String,
        }
        let supplied = WireIdentity::deserialize(deserializer)?;
        if supplied.principal_id != ANNA_SCRIBE_PRINCIPAL_ID
            || supplied.persona_id != ANNA_SCRIBE_PERSONA_ID
            || supplied.email != ANNA_SCRIBE_EMAIL
            || supplied.plugin_ref != ANNA_SCRIBE_PLUGIN_REF
        {
            return Err(serde::de::Error::custom(
                "A.N.N.A. Scribe service-persona identity is immutable",
            ));
        }
        Ok(Self::default())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-immutable-paths" = ["/service_persona"]))]
pub struct WgOpdbusState {
    #[serde(default)]
    #[schemars(extend("readOnly" = true))]
    pub service_persona: AnnaScribeServicePersona,
    pub interface: Option<WireGuardInterface>,
    #[serde(default)]
    #[schemars(extend("x-opdbus-exact-json-schema" = true))]
    pub addresses: Vec<String>,
    pub mtu: Option<u32>,
    pub admin_up: bool,
}

/// Complete native projection for the shared decoy WireGuard owner.
///
/// Every allowlisted interface has an entry. An absent kernel interface is
/// represented by a default status whose `interface` is `None`, so consumers
/// can distinguish known absence from an omitted sibling.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(
    extend("x-immutable-paths" = ["/service_persona"]),
    extend("x-opdbus-exact-json-schema" = true)
)]
pub struct WgOpdbusAggregateState {
    #[serde(default)]
    #[schemars(extend("readOnly" = true))]
    pub service_persona: AnnaScribeServicePersona,
    #[serde(default)]
    #[schemars(extend("x-opdbus-exact-json-schema" = true))]
    pub interfaces: BTreeMap<String, WgOpdbusState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateInterfaceInput {
    pub interface_name: String,
    #[serde(default)]
    #[schemars(extend("x-opdbus-exact-json-schema" = true))]
    pub addresses: Vec<String>,
    #[serde(default)]
    pub mtu: Option<u32>,
    #[serde(default)]
    #[schemars(extend("x-opdbus-exact-json-schema" = true))]
    pub device: WireGuardDeviceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetStatusInput {
    pub interface_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CreateInterfaceOutput {
    pub status: WgOpdbusState,
}

pub type WgOpdbusStatusOutput = WgOpdbusState;

#[derive(Debug, Clone, Default)]
pub struct WgOpdbusPlugin;
impl WgOpdbusPlugin {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StatePlugin for WgOpdbusPlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }
    fn version(&self) -> &str {
        PLUGIN_VERSION
    }
    fn schema(&self) -> Option<PluginSchema> {
        Some(wg_opdbus_schema())
    }
    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        anyhow::bail!("wg_opdbus is owned by the authenticated decoy D-Bus service")
    }
    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        anyhow::bail!("wg_opdbus is owned by the authenticated decoy D-Bus service")
    }
    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        anyhow::bail!("wg_opdbus verification requires decoy kernel readback")
    }
    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        anyhow::bail!("wg_opdbus checkpointing is not supported")
    }
    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        anyhow::bail!("wg_opdbus rollback is not supported")
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

pub(crate) fn wg_opdbus_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(WgOpdbusAggregateState))
        .expect("schema serializes");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        PLUGIN_NAME,
        PLUGIN_VERSION,
        PLUGIN_DESCRIPTION,
        &root,
    );
    schema.category = PLUGIN_CATEGORY.into();
    schema.display_name = Some(PLUGIN_DISPLAY_NAME.into());
    schema.dependencies.push("wireguard".into());
    schema.methods.insert(
        "create_interface".into(),
        method_decl_from_schemars_with_output::<CreateInterfaceInput, CreateInterfaceOutput>(
            "create_interface",
            SideEffect::Mutation,
            false,
            "wg_opdbus.invoke",
            "mut.network.wg-opdbus.interface.create@v2",
        ),
    );
    schema.methods.insert(
        "get_status".into(),
        method_decl_from_schemars_with_output::<GetStatusInput, WgOpdbusStatusOutput>(
            "get_status",
            SideEffect::Read,
            true,
            "wg_opdbus.read",
            "obs.network.wg-opdbus.status.get@v2",
        ),
    );
    schema.example = Some({
        let mut bytes =
            serde_json::to_vec(&WgOpdbusAggregateState::default()).expect("default serializes");
        simd_json::to_owned_value(&mut bytes).expect("default is JSON")
    });
    schema.capabilities.insert(
        "wg_opdbus.invoke".into(),
        CapabilityDecl {
            id: "wg_opdbus.invoke".into(),
            description: "Grants: create_interface.".into(),
        },
    );
    schema.capabilities.insert(
        "wg_opdbus.read".into(),
        CapabilityDecl {
            id: "wg_opdbus.read".into(),
            description: "Grants: get_status.".into(),
        },
    );
    schema
}

inventory::submit! { crate::default_registry::PluginReg::new(PLUGIN_NAME, |_ctx| std::sync::Arc::new(WgOpdbusPlugin::new())) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_service_persona_is_immutable() {
        let identity = serde_json::to_value(AnnaScribeServicePersona::default()).unwrap();
        assert_eq!(identity["principal_id"], ANNA_SCRIBE_PRINCIPAL_ID);
        let mut changed = identity;
        changed["email"] = serde_json::json!("caller@example.invalid");
        assert!(serde_json::from_value::<AnnaScribeServicePersona>(changed).is_err());
    }
    #[test]
    fn retired_netmaker_and_config_path_fields_are_absent() {
        let schema = wg_opdbus_schema();
        let input = serde_json::to_value(&schema.methods["create_interface"].args).unwrap();
        let text = input.to_string();
        assert!(!text.contains("netmaker"));
        assert!(!text.contains("config_path"));
        assert_eq!(schema.immutable_paths, vec!["/service_persona"]);
    }

    #[test]
    fn state_schema_is_an_exact_aggregate_while_method_outputs_remain_per_interface() {
        let schema = wg_opdbus_schema();
        let interfaces = serde_json::to_value(&schema.fields["interfaces"]).unwrap();
        assert!(
            interfaces
                .to_string()
                .contains("x-opdbus-exact-json-schema")
        );

        let output = serde_json::to_value(&schema.methods["get_status"].returns).unwrap();
        assert!(output.to_string().contains("interface"));
        assert!(!output.to_string().contains("interfaces"));
    }
}
