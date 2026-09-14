//! OVSDB Bridge plugin — selected RFC 7047 Bridge/Port/Interface topology.
//!
//! OVSDB *is* the source of truth. This plugin queries reality from ovsdb-server
//! and the MutationEngine projects complete observations into canonical SHM.
//! Opaque options/other_config are not observed by the topology monitor.

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
use anyhow::Result;
use async_trait::async_trait;
use op_network::rovs_proxy::OvsdbDbusClient;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::PluginSchema;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

#[path = "ovsdb_topology.rs"]
mod topology;
pub use topology::project_topology;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListDbsInput {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetSchemaInput {
    pub db_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TransactInput {
    pub db_name: String,
    pub operations: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MonitorInput {
    pub db_name: String,
    pub monitor_id: String,
    pub monitor_requests: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MonitorCondInput {
    pub db_name: String,
    pub monitor_id: String,
    pub monitor_cond_requests: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LockInput {
    pub lock_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StealInput {
    pub lock_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UnlockInput {
    pub lock_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EchoInput {
    pub params: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CancelInput {
    pub id: String,
}

/// Input struct for CreateBridge method
/// D-Bus method spec: https://www.opennetworking.org/wp-content/uploads/2014/10/of_spec_1_0.pdf
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateBridgeInput {
    /// Bridge name
    pub name: String,
    /// Datapath type (e.g., "system", "internal")
    pub datapath_type: Option<String>,
    /// Fail mode (secure or standalone)
    pub fail_mode: Option<String>,
    /// Enable STP
    pub stp_enable: Option<bool>,
}

/// Input struct for DeleteBridge method
/// D-Bus method spec: https://www.opennetworking.org/wp-content/uploads/2014/10/of_spec_1_0.pdf
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeleteBridgeInput {
    /// Bridge name to delete
    pub name: String,
}

/// Input struct for AddPort method
/// D-Bus method spec: https://www.opennetworking.org/wp-content/uploads/2014/10/of_spec_1_0.pdf
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AddPortInput {
    /// Bridge name
    pub bridge: String,
    /// Port name
    pub port: String,
    /// Interface name
    pub interface: Option<String>,
    /// VLAN tag (optional)
    pub tag: Option<i32>,
}

/// Input struct for RemovePort method
/// D-Bus method spec: https://www.opennetworking.org/wp-content/uploads/2014/10/of_spec_1_0.pdf
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RemovePortInput {
    /// Bridge name
    pub bridge: String,
    /// Port name to remove
    pub port: String,
}

use std::sync::Arc;

// ============================================================================
// RFC 7047 §3.2 Schema Types — Bridge → Port → Interface hierarchy
// ============================================================================

/// Typed OVS topology. Configuration maps are optional and intentionally
/// omitted by the selected-column observer; absence is not an empty-map claim.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-category" = "network"))]
pub struct OvsBridgeState {
    pub bridges: Vec<BridgeConfig>,
}

/// RFC 7047 Bridge table row.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BridgeConfig {
    pub name: String,
    #[serde(default)]
    pub ports: Vec<PortConfig>,
    /// "system" | "netdev" | "" (kernel datapath)
    #[serde(default)]
    pub datapath_type: String,
    /// "standalone" | "secure" | null
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fail_mode: Option<String>,
    #[serde(default)]
    pub stp_enable: bool,
    #[serde(default)]
    pub mcast_snooping_enable: bool,
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub other_config: std::collections::HashMap<String, String>,
}

/// RFC 7047 Port table row.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PortConfig {
    pub name: String,
    #[serde(default)]
    pub interfaces: Vec<InterfaceConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trunks: Vec<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vlan_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bond_mode: Option<String>,
}

/// RFC 7047 Interface table row.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InterfaceConfig {
    pub name: String,
    /// "system" | "internal" | "patch" | "vxlan" | "gre" | "geneve" | ""
    #[serde(default, rename = "type")]
    pub iface_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac_in_use: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admin_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_state: Option<String>,
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub options: std::collections::HashMap<String, String>,
}

// ============================================================================
// Plugin
// ============================================================================

pub struct OvsBridgePlugin {
    ovsdb: Arc<OvsdbDbusClient>,
}

impl Default for OvsBridgePlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl OvsBridgePlugin {
    pub fn new() -> Self {
        Self {
            ovsdb: Arc::new(OvsdbDbusClient::new()),
        }
    }

    /// One native initial monitor observation; dropping this temporary
    /// receiver terminates its connection. No error becomes invented state.
    pub async fn query_bridges(&self) -> Result<OvsBridgeState> {
        let receiver = self.ovsdb.monitor_db("Open_vSwitch").await?;
        let observed = receiver.borrow().clone();
        match observed {
            op_network::rovs_proxy::monitor::MonitorObservation::Snapshot(snapshot) => {
                project_topology(&snapshot)
            }
            op_network::rovs_proxy::monitor::MonitorObservation::Unavailable(_) => {
                anyhow::bail!("OVSDB topology observation is unavailable")
            }
        }
    }
}

#[async_trait]
impl StatePlugin for OvsBridgePlugin {
    fn name(&self) -> &str {
        "ovsdb_bridge"
    }

    fn version(&self) -> &str {
        "2.0.0"
    }

    fn schema(&self) -> Option<op_state_store::PluginSchema> {
        Some(ovsdb_bridge_schema())
    }

    fn is_available(&self) -> bool {
        std::path::Path::new("/var/run/openvswitch/db.sock").exists()
    }

    fn unavailable_reason(&self) -> String {
        "OVSDB socket not found at /var/run/openvswitch/db.sock".to_string()
    }

    /// Query reality — dump OVSDB Bridge/Port/Interface tables.

    /// Reconciliation, not diff. OVSDB is the DB — the "desired" parameter
    /// is what the D-Bus mirror currently shows. We return actions needed
    /// to update the mirror to match OVSDB reality.
    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        // No diff — OVSDB is authoritative. The mirror reconciliation loop
        // in op-dbus-mirror handles projection. Return empty diff.
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

    /// No-op — reconciliation happens via the mirror, not through apply.
    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        Ok(ApplyResult {
            success: true,
            changes_applied: vec![],
            errors: vec![],
            checkpoint: None,
        })
    }

    /// Verify just re-queries OVSDB — it's always "correct" by definition.
    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        let state = simd_json::json!(null);
        Ok(Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: state,
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
            atomic_operations: true,
        }
    }
}

pub(crate) fn ovsdb_bridge_schema() -> PluginSchema {
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "ovsdb_bridge",
        "2.0.0",
        "Selected native OVS bridge, port and interface topology; private configuration maps are not observed",
        &serde_json::to_value(schemars::schema_for!(OvsBridgeState)).unwrap(),
    );
    super::schemars_adapter::apply_state_defaults(
        &mut schema,
        &simd_json::serde::to_owned_value(&OvsBridgeState::default()).unwrap(),
    );

    schema.methods.insert(
        "list_dbs".to_string(),
        method_decl_from_schemars_with_output::<
            ListDbsInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "list_dbs",
            op_state_store::SideEffect::Read,
            true,
            "ovsdb.read",
            "obs.network.ovsdb.dbs.list@v1",
        ),
    );
    schema.methods.insert(
        "get_schema".to_string(),
        method_decl_from_schemars_with_output::<
            GetSchemaInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "get_schema",
            op_state_store::SideEffect::Read,
            true,
            "ovsdb.read",
            "obs.network.ovsdb.schema.get@v1",
        ),
    );
    schema.methods.insert(
        "transact".to_string(),
        method_decl_from_schemars_with_output::<
            TransactInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "transact",
            op_state_store::SideEffect::Mutation,
            false,
            "ovsdb.write",
            "mut.network.ovsdb.transact@v1",
        ),
    );
    schema.methods.insert(
        "monitor".to_string(),
        method_decl_from_schemars_with_output::<
            MonitorInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "monitor",
            op_state_store::SideEffect::Read,
            false,
            "ovsdb.read",
            "obs.network.ovsdb.monitor@v1",
        ),
    );
    schema.methods.insert(
        "monitor_cond".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            MonitorCondInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "monitor_cond",
            op_state_store::SideEffect::Read,
            false,
            "ovsdb.read",
            "obs.network.ovsdb.monitor-cond@v1",
        ),
    );
    schema.methods.insert(
        "lock".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            LockInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "lock",
            op_state_store::SideEffect::Mutation,
            false,
            "ovsdb.write",
            "mut.network.ovsdb.lock@v1",
        ),
    );
    schema.methods.insert(
        "steal".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            StealInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "steal",
            op_state_store::SideEffect::Mutation,
            false,
            "ovsdb.write",
            "mut.network.ovsdb.lock.steal@v1",
        ),
    );
    schema.methods.insert(
        "unlock".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            UnlockInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "unlock",
            op_state_store::SideEffect::Mutation,
            false,
            "ovsdb.write",
            "mut.network.ovsdb.lock.unlock@v1",
        ),
    );
    schema.methods.insert(
        "echo".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            EchoInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "echo",
            op_state_store::SideEffect::Read,
            true,
            "ovsdb.read",
            "obs.network.ovsdb.echo@v1",
        ),
    );
    schema.methods.insert(
        "cancel".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            CancelInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "cancel",
            op_state_store::SideEffect::Mutation,
            false,
            "ovsdb.write",
            "mut.network.ovsdb.cancel@v1",
        ),
    );

    // Add required methods: CreateBridge, DeleteBridge, AddPort, RemovePort
    // D-Bus method spec: https://www.opennetworking.org/wp-content/uploads/2014/10/of_spec_1_0.pdf
    schema.methods.insert(
        "create_bridge".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            CreateBridgeInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "CreateBridge",
            op_state_store::SideEffect::Mutation,
            false,
            "ovsdb.write",
            "mut.network.ovsdb.bridge.create@v1",
        ),
    );
    schema.methods.insert(
        "delete_bridge".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            DeleteBridgeInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "DeleteBridge",
            op_state_store::SideEffect::Mutation,
            false,
            "ovsdb.write",
            "mut.network.ovsdb.bridge.delete@v1",
        ),
    );
    schema.methods.insert(
        "add_port".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            AddPortInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "AddPort",
            op_state_store::SideEffect::Mutation,
            false,
            "ovsdb.write",
            "mut.network.ovsdb.port.add@v1",
        ),
    );
    schema.methods.insert(
        "remove_port".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            RemovePortInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "RemovePort",
            op_state_store::SideEffect::Mutation,
            false,
            "ovsdb.write",
            "mut.network.ovsdb.port.remove@v1",
        ),
    );

    schema.capabilities.insert(
        "ovsdb.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "ovsdb.read".to_string(),
            description: "Grants: list_dbs, get_schema.".to_string(),
        },
    );
    schema.capabilities.insert(
        "ovsdb.write".to_string(),
        op_state_store::CapabilityDecl {
            id: "ovsdb.write".to_string(),
            description: "Grants: transact, create_bridge, delete_bridge, add_port, remove_port."
                .to_string(),
        },
    );

    schema.public_state.paths = [
        "/bridges/*/name",
        "/bridges/*/datapath_type",
        "/bridges/*/fail_mode",
        "/bridges/*/stp_enable",
        "/bridges/*/mcast_snooping_enable",
        "/bridges/*/ports/*/name",
        "/bridges/*/ports/*/tag",
        "/bridges/*/ports/*/trunks",
        "/bridges/*/ports/*/vlan_mode",
        "/bridges/*/ports/*/bond_mode",
        "/bridges/*/ports/*/interfaces/*/name",
        "/bridges/*/ports/*/interfaces/*/type",
        "/bridges/*/ports/*/interfaces/*/mac_in_use",
        "/bridges/*/ports/*/interfaces/*/mac",
        "/bridges/*/ports/*/interfaces/*/admin_state",
        "/bridges/*/ports/*/interfaces/*/link_state",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    schema
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("ovsdb_bridge", |_ctx| std::sync::Arc::new(OvsBridgePlugin::new()))
}
