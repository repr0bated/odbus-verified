//! GB.Keypair plugin — schemars-seeded, D-Bus-backed keypair declaration state.
//!
//! This is the reference implementation of the GB (Golden Bridge) plugin
//! pattern: the plugin file owns the schemars seed (state struct), the
//! `PluginSchema` contract is derived from that seed, and method declarations
//! use typed input/output structs via `method_decl_from_schemars_with_output`.

use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use simd_json::json;
use simd_json::prelude::*;

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
mod agent;
pub use agent::dispatch_keypair_method;

// =============================================================================
// PLUGIN ENTRY: identity and typed schema seed
// =============================================================================

const PLUGIN_NAME: &str = "keypair";
const PLUGIN_VERSION: &str = "2.0.0";
const PLUGIN_CATEGORY: &str = "service";
const PLUGIN_DESCRIPTION: &str = "Public identities advertised by the configured SSH agent";
const PLUGIN_DISPLAY_NAME: &str = "GB.Keypair";

/// Keypair declaration state.
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.keypair.schema@v1"))]
#[schemars(extend("x-oscal-category" = "service"))]
pub struct KeypairState {
    /// Managed keypairs.
    #[serde(default)]
    #[schemars(
        description = "Managed keypairs",
        extend("x-oscal-subid" = "exp.service.keypair.keypairs.render@v1")
    )]
    pub keypairs: Vec<Keypair>,
}

/// A single SSH keypair (state-side declaration).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Keypair {
    /// Keypair name.
    #[schemars(
        description = "Keypair name",
        example = &"id_ed25519",
        extend("x-oscal-subid" = "exp.service.keypair.name.declare@v1")
    )]
    pub name: String,

    /// Public-key algorithm.
    #[schemars(
        description = "Public-key algorithm",
        example = &"ssh-ed25519",
        extend("x-oscal-subid" = "exp.service.keypair.algorithm.declare@v1")
    )]
    pub algorithm: String,

    /// Public key material.
    #[serde(default)]
    #[schemars(
        description = "Public key material",
        extend("x-oscal-subid" = "exp.service.keypair.public-key.declare@v1")
    )]
    pub public_key: Option<String>,

    /// Whether the keypair is present on the system.
    #[serde(default)]
    #[schemars(
        description = "Whether the keypair is present on the system",
        extend("x-oscal-subid" = "obs.service.keypair.present.detect@v1")
    )]
    pub present: bool,
}

/// Typed input for the `list_keypairs` method (empty — no parameters).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "obs.service.keypair.list.input@v1"))]
#[serde(deny_unknown_fields)]
pub struct ListKeypairsInput {}

/// Typed output for the `list_keypairs` method.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "obs.service.keypair.list.output@v1"))]
pub struct ListKeypairsOutput {
    /// Provider scope; this is an agent inventory, not a scan of key files.
    pub source: String,
    /// Discovered keypairs.
    #[schemars(
        description = "Discovered keypairs",
        extend("x-oscal-subid" = "exp.service.keypair.list.keypairs.render@v1")
    )]
    pub keypairs: Vec<KeypairInfo>,
}

/// A single keypair as returned by `list_keypairs` (output-side view).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "obs.service.keypair.keypair-info.list@v1"))]
pub struct KeypairInfo {
    /// SHA-256 fingerprint of the SSH public-key wire encoding.
    pub fingerprint: String,
    /// Keypair name.
    #[schemars(
        description = "Keypair name",
        example = &"id_ed25519",
        extend("x-oscal-subid" = "exp.service.keypair.info.name.render@v1")
    )]
    pub name: String,

    /// Public-key algorithm.
    #[schemars(
        description = "Public-key algorithm",
        example = &"ssh-ed25519",
        extend("x-oscal-subid" = "exp.service.keypair.info.algorithm.render@v1")
    )]
    pub algorithm: String,

    /// Public key material.
    #[serde(default)]
    #[schemars(
        description = "Public key material",
        extend("x-oscal-subid" = "exp.service.keypair.info.public-key.render@v1")
    )]
    pub public_key: Option<String>,

    /// Whether the agent advertised this identity. This does not assert that
    /// signing is currently permitted or that private material is exportable.
    #[serde(default)]
    #[schemars(
        description = "Identity advertised by the SSH agent; signing availability is not tested",
        extend("x-oscal-subid" = "obs.service.keypair.info.present.render@v1")
    )]
    pub present: bool,
}

// =============================================================================
// PLUGIN BODY: D-Bus-backed behavior only
// =============================================================================

pub struct KeypairPlugin;

impl Default for KeypairPlugin {
    fn default() -> Self {
        Self
    }
}

impl KeypairPlugin {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StatePlugin for KeypairPlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn schema(&self) -> Option<PluginSchema> {
        Some(keypair_schema())
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: "unknown".to_string(),
                desired_hash: "unknown".to_string(),
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
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: Value::null(),
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

// =============================================================================
// PLUGIN EXIT: publish the single PluginSchema contract
// =============================================================================

/// Derived `keypair` schema from the `KeypairState` struct.
pub fn keypair_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(KeypairState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        PLUGIN_NAME,
        PLUGIN_VERSION,
        PLUGIN_DESCRIPTION,
        &root,
    );
    schema.category = PLUGIN_CATEGORY.to_string();
    schema.display_name = Some(PLUGIN_DISPLAY_NAME.to_string());
    schema.methods.insert(
        "list_keypairs".to_string(),
        method_decl_from_schemars_with_output::<ListKeypairsInput, ListKeypairsOutput>(
            "list_keypairs",
            SideEffect::Read,
            true,
            "cap.service.keypair.list@v1",
            "obs.service.keypair.list@v1",
        ),
    );
    schema.capabilities.insert(
        "cap.service.keypair.list@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.service.keypair.list@v1".to_string(),
            description: "Grants: list_keypairs.".to_string(),
        },
    );

    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_schemars_seeded_and_typed() {
        let schema = keypair_schema();
        assert_eq!(schema.name, PLUGIN_NAME);
        assert_eq!(schema.version, PLUGIN_VERSION);
        assert_eq!(schema.display_name, Some(PLUGIN_DISPLAY_NAME.to_string()));
        assert!(schema.fields.contains_key("keypairs"));
        assert!(schema.methods.contains_key("list_keypairs"));
    }

    /// Every `x-oscal-subid` annotation in the derived schema must be a valid
    /// OSCAL subid according to the canonical taxonomy.
    #[test]
    fn all_subids_are_valid() {
        let raw = serde_json::to_value(schemars::schema_for!(KeypairState)).unwrap();
        let mut subids = Vec::new();
        collect_subids(&raw, &mut subids);
        assert!(!subids.is_empty(), "expected at least one subid");
        for subid in subids {
            crate::state_plugins::common::oscal::validate_subid(&subid)
                .unwrap_or_else(|e| panic!("invalid subid {subid}: {e}"));
        }
    }

    fn collect_subids(value: &serde_json::Value, out: &mut Vec<String>) {
        if let serde_json::Value::Object(map) = value {
            if let Some(subid) = map.get("x-oscal-subid").and_then(|v| v.as_str()) {
                out.push(subid.to_string());
            }
            for v in map.values() {
                collect_subids(v, out);
            }
        } else if let serde_json::Value::Array(arr) = value {
            for v in arr {
                collect_subids(v, out);
            }
        }
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new(PLUGIN_NAME, |_ctx| std::sync::Arc::new(KeypairPlugin::new()))
}
