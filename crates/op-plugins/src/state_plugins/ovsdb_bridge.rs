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
use std::collections::BTreeMap;

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
    pub monitor_requests: BTreeMap<String, Vec<OvsdbMonitorRequest>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MonitorCondInput {
    pub db_name: String,
    pub monitor_id: String,
    pub monitor_cond_requests: BTreeMap<String, Vec<OvsdbMonitorRequest>>,
}

/// One RFC 7047 monitor request. `where` is accepted only by monitor_cond.
/// The native owner additionally restricts tables and columns to the selected
/// non-secret topology surface declared by this plugin.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OvsdbMonitorRequest {
    pub columns: Vec<String>,
    #[serde(default, rename = "where", skip_serializing_if = "Option::is_none")]
    pub where_: Option<Vec<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub select: Option<OvsdbMonitorSelect>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OvsdbMonitorSelect {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modify: Option<bool>,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListDbsOutput {
    pub databases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetSchemaOutput {
    pub schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TransactOutput {
    pub results: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MonitorOutput {
    pub monitor_id: String,
    pub method: String,
    pub sequence: u64,
    pub tables: Vec<OvsdbMonitorTable>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LockOutput {
    pub lock_id: String,
    pub locked: bool,
    pub queued: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UnlockOutput {
    pub lock_id: String,
    pub unlocked: bool,
    pub acquired_while_queued: bool,
    pub stolen_before_unlock: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EchoOutput {
    pub params: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CancelOutput {
    pub monitor_id: String,
    pub canceled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BridgeMutationOutput {
    pub bridge: String,
    pub changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PortMutationOutput {
    pub bridge: String,
    pub port: String,
    pub changed: bool,
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
    /// Active caller-scoped monitor materializations. This protected field is
    /// omitted from public_state and is updated as a child so topology refresh
    /// never removes another monitor or unrelated bridge state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(default)]
    pub monitors: Vec<OvsdbMonitorState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OvsdbMonitorState {
    /// Opaque caller-and-monitor key used only to update this protected row.
    pub projection_key: String,
    pub monitor_id: String,
    pub method: String,
    pub sequence: u64,
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub tables: Vec<OvsdbMonitorTable>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OvsdbMonitorTable {
    pub name: String,
    pub rows: Vec<OvsdbMonitorRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OvsdbMonitorRow {
    pub uuid: String,
    pub columns: Vec<OvsdbMonitorColumn>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OvsdbMonitorColumn {
    pub name: String,
    pub value: serde_json::Value,
}

/// Preserve dynamic native table, UUID and column keys as bounded data in a
/// typed array tree. PluginSchema's field model cannot faithfully represent
/// constraints nested below arbitrary map keys.
pub fn materialize_monitor_tables(
    tables: &op_network::rovs_proxy::monitor::ProtocolTables,
) -> Vec<OvsdbMonitorTable> {
    tables
        .iter()
        .map(|(name, rows)| OvsdbMonitorTable {
            name: name.clone(),
            rows: rows
                .iter()
                .map(|(uuid, columns)| OvsdbMonitorRow {
                    uuid: uuid.clone(),
                    columns: columns
                        .iter()
                        .map(|(name, value)| OvsdbMonitorColumn {
                            name: name.clone(),
                            value: value.clone(),
                        })
                        .collect(),
                })
                .collect(),
        })
        .collect()
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
        "3.0.0"
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
        "3.0.0",
        "Selected native OVS bridge, port and interface topology; private configuration maps are not observed",
        &serde_json::to_value(schemars::schema_for!(OvsBridgeState)).unwrap(),
    );
    super::schemars_adapter::apply_state_defaults(
        &mut schema,
        &simd_json::serde::to_owned_value(&OvsBridgeState::default()).unwrap(),
    );

    schema.methods.insert(
        "list_dbs".to_string(),
        method_decl_from_schemars_with_output::<ListDbsInput, ListDbsOutput>(
            "list_dbs",
            op_state_store::SideEffect::Read,
            true,
            "ovsdb.read",
            "obs.network.ovsdb.dbs.list@v1",
        ),
    );
    schema.methods.insert(
        "get_schema".to_string(),
        method_decl_from_schemars_with_output::<GetSchemaInput, GetSchemaOutput>(
            "get_schema",
            op_state_store::SideEffect::Read,
            true,
            "ovsdb.read",
            "obs.network.ovsdb.schema.get@v1",
        ),
    );
    schema.methods.insert(
        "transact".to_string(),
        method_decl_from_schemars_with_output::<TransactInput, TransactOutput>(
            "transact",
            op_state_store::SideEffect::Mutation,
            false,
            "ovsdb.write",
            "mut.network.ovsdb.transact@v1",
        ),
    );
    schema.methods.insert(
        "monitor".to_string(),
        method_decl_from_schemars_with_output::<MonitorInput, MonitorOutput>(
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
            MonitorOutput,
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
            LockOutput,
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
            LockOutput,
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
            UnlockOutput,
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
            EchoOutput,
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
            CancelOutput,
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
            BridgeMutationOutput,
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
            BridgeMutationOutput,
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
            PortMutationOutput,
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
            PortMutationOutput,
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
