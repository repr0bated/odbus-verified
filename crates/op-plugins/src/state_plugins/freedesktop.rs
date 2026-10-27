//! FreeDesktop Plugin
//!
//! Implements FreeDesktop D-Bus standards support including:
//! - org.freedesktop.DBus.ObjectManager
//! - org.freedesktop.DBus.Properties
//! - org.freedesktop.DBus.Introspectable
//! - org.freedesktop.DBus.Peer
//!
//! This plugin follows the canonical path convention:
//! - D-Bus Path: /org/opdbus/v1/plugins/freedesktop
//! - Interface: org.opdbus.v1.PluginV1
//! - Schema: this plugin's Rust declaration.
//!
//! ## Architecture
//!
//! The bridge executes these standard operations against canonical sealed
//! OP-DBUS plugin objects. The descriptor tables below describe FreeDesktop
//! metadata; they do not grant arbitrary external D-Bus access.
//!
//! ## Canonical Path Compliance
//!
//! This plugin uses the canonical paths as defined in `crate::canonical`:
//! - Object path prefix: `/org/opdbus/v1/plugins/`
//! - Interface: `org.opdbus.v1.PluginV1`
//!
//! Legacy paths like `/opdbus/v1/plugins/` or `/org/opdbus/v1/plugin/plugins/`
//! are normalized to the canonical prefix by `crate::canonical::normalize_plugin_path`.

use crate::canonical;
use anyhow::Result;
use async_trait::async_trait;
use op_state::{
    ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, PluginSchema, StateDiff, StatePlugin,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use simd_json::prelude::{ValueAsScalar, ValueObjectAccess};
use std::collections::HashMap;

/// Enumeration is restricted to the current sealed OP-DBUS plugin catalog.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetManagedObjectsInput {}

fn canonical_interface() -> String {
    canonical::PLUGIN_INTERFACE.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetPropertyInput {
    pub plugin_id: String,
    #[serde(default = "canonical_interface")]
    pub interface_name: String,
    pub property_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetAllPropertiesInput {
    pub plugin_id: String,
    #[serde(default = "canonical_interface")]
    pub interface_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetPropertyInput {
    pub plugin_id: String,
    #[serde(default = "canonical_interface")]
    pub interface_name: String,
    pub property_name: String,
    /// A native JSON value checked against the target plugin's sealed field.
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IntrospectInput {
    pub plugin_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PingInput {
    pub plugin_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetMachineIdInput {
    pub plugin_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ManagedPluginObject {
    pub plugin_id: String,
    pub object_path: String,
    pub interface_name: String,
    pub schema_hash: String,
    /// Only fields declared by this sealed plugin and present in public SHM.
    pub properties: std::collections::BTreeMap<String, serde_json::Value>,
    pub projection_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetManagedObjectsOutput {
    pub objects: Vec<ManagedPluginObject>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetPropertyOutput {
    pub plugin_id: String,
    pub property_name: String,
    pub value: serde_json::Value,
    pub read_only: bool,
    pub schema_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetAllPropertiesOutput {
    pub object: ManagedPluginObject,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SetPropertyOutput {
    pub plugin_id: String,
    pub property_name: String,
    pub method: String,
    /// The mapped mutation's actual result; no requested-value echo.
    pub result: serde_json::Value,
    pub event_id: u64,
    pub event_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IntrospectOutput {
    pub plugin_id: String,
    pub object_path: String,
    pub schema_hash: String,
    /// Canonical PluginV1 ABI with sealed method/field metadata annotations.
    /// A declared plugin method is invoked through Call, not a new D-Bus member.
    pub xml: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PingOutput {
    pub plugin_id: String,
    pub object_path: String,
    pub schema_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetMachineIdOutput {
    pub plugin_id: String,
    pub machine_id: String,
}

/// FreeDesktop plugin implementation
pub struct FreeDesktopPlugin {
    schema: Option<PluginSchema>,
    dbus_interfaces: HashMap<String, DbusInterface>,
}

/// D-Bus interface definition
#[derive(Debug, Clone)]
pub struct DbusInterface {
    pub name: String,
    pub description: String,
    pub methods: Vec<DbusMethod>,
    pub signals: Vec<DbusSignal>,
    pub properties: Vec<DbusProperty>,
}

/// D-Bus method definition
#[derive(Debug, Clone)]
pub struct DbusMethod {
    pub name: String,
    pub signature: String,
    pub description: String,
}

/// D-Bus signal definition
#[derive(Debug, Clone)]
pub struct DbusSignal {
    pub name: String,
    pub signature: String,
    pub description: String,
}

/// D-Bus property definition
#[derive(Debug, Clone)]
pub struct DbusProperty {
    pub name: String,
    pub property_type: String,
    pub access: PropertyAccess,
    pub description: String,
}

#[derive(Debug, Clone)]
pub enum PropertyAccess {
    Read,
    Write,
    ReadWrite,
}

impl FreeDesktopPlugin {
    /// Create a new FreeDesktop plugin with default configuration
    pub fn new() -> Self {
        let mut plugin = Self {
            schema: None,
            dbus_interfaces: HashMap::new(),
        };

        // Initialize schema from embedded definition
        plugin.initialize_schema();

        // Register standard FreeDesktop interfaces
        plugin.register_standard_interfaces();

        plugin
    }

    /// Initialize the plugin schema
    fn initialize_schema(&mut self) {
        // Static fallback; the plugin owns its contract.
        // For now, we define the schema programmatically following the canonical structure
        self.schema = Some(
            PluginSchema::builder("freedesktop")
                .category("service")
                .version("2.0.0")
                .category("system")
                .description("FreeDesktop operations scoped to sealed OP-DBUS plugin objects")
                .method(
                    super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
GetManagedObjectsInput,
                        GetManagedObjectsOutput,
                    >(
                        "GetManagedObjects",
                        op_state_store::SideEffect::Read,
                        true,
                        "freedesktop.read",
                        "obs.software.freedesktop.managed-objects.get@v1",
                    ),
                )
                .method(
                    super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
GetPropertyInput,
                        GetPropertyOutput,
                    >(
                        "Get",
                        op_state_store::SideEffect::Read,
                        true,
                        "freedesktop.read",
                        "obs.software.freedesktop.property.get@v1",
                    ),
                )
                .method(
                    super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
GetAllPropertiesInput,
                        GetAllPropertiesOutput,
                    >(
                        "GetAll",
                        op_state_store::SideEffect::Read,
                        true,
                        "freedesktop.read",
                        "obs.software.freedesktop.properties.getall@v1",
                    ),
                )
                .method(
                    super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
SetPropertyInput,
                        SetPropertyOutput,
                    >(
                        "Set",
                        op_state_store::SideEffect::Mutation,
                        true,
                        "freedesktop.write",
                        "mut.software.freedesktop.property.set@v1",
                    ),
                )
                .method(
                    super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
IntrospectInput,
                        IntrospectOutput,
                    >(
                        "Introspect",
                        op_state_store::SideEffect::Read,
                        true,
                        "freedesktop.read",
                        "obs.software.freedesktop.introspect@v1",
                    ),
                )
                .method(
                    super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
PingInput,
                        PingOutput,
                    >(
                        "Ping",
                        op_state_store::SideEffect::Read,
                        true,
                        "freedesktop.read",
                        "obs.software.freedesktop.ping@v1",
                    ),
                )
                .method(
                    super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
GetMachineIdInput,
                        GetMachineIdOutput,
                    >(
                        "GetMachineId",
                        op_state_store::SideEffect::Read,
                        true,
                        "freedesktop.read",
                        "obs.software.freedesktop.machine-id.get@v1",
                    ),
                )
                .capability(op_state_store::CapabilityDecl {
                    id: "freedesktop.read".to_string(),
                    description:
                        "Grants: GetManagedObjects, Get, GetAll, Introspect, Ping, GetMachineId."
                            .to_string(),
                })
                .capability(op_state_store::CapabilityDecl {
                    id: "freedesktop.write".to_string(),
                    description: "Admits Set; the target plugin's mapped mutation capability is independently required.".to_string(),
                })
                .build(),
        );
    }

    /// Register standard FreeDesktop D-Bus interfaces
    fn register_standard_interfaces(&mut self) {
        // ObjectManager interface
        self.dbus_interfaces.insert(
            canonical::OBJECT_MANAGER_INTERFACE.to_string(),
            DbusInterface {
                name: canonical::OBJECT_MANAGER_INTERFACE.to_string(),
                description: "Interface for enumerating managed objects".to_string(),
                methods: vec![DbusMethod {
                    name: "GetManagedObjects".to_string(),
                    signature: "a{oa{sa{sv}}}".to_string(),
                    description: "Get all managed objects with their interfaces".to_string(),
                }],
                signals: vec![
                    DbusSignal {
                        name: "InterfacesAdded".to_string(),
                        signature: "oa{sa{sv}}".to_string(),
                        description: "Emitted when interfaces are added".to_string(),
                    },
                    DbusSignal {
                        name: "InterfacesRemoved".to_string(),
                        signature: "oas".to_string(),
                        description: "Emitted when interfaces are removed".to_string(),
                    },
                ],
                properties: vec![],
            },
        );

        // Properties interface
        self.dbus_interfaces.insert(
            canonical::PROPERTIES_INTERFACE.to_string(),
            DbusInterface {
                name: canonical::PROPERTIES_INTERFACE.to_string(),
                description: "Standard D-Bus properties interface".to_string(),
                methods: vec![
                    DbusMethod {
                        name: "Get".to_string(),
                        signature: "v".to_string(),
                        description: "Get a property value".to_string(),
                    },
                    DbusMethod {
                        name: "GetAll".to_string(),
                        signature: "a{sv}".to_string(),
                        description: "Get all properties of an interface".to_string(),
                    },
                    DbusMethod {
                        name: "Set".to_string(),
                        signature: "".to_string(),
                        description: "Set a property value".to_string(),
                    },
                ],
                signals: vec![DbusSignal {
                    name: "PropertiesChanged".to_string(),
                    signature: "sa{sv}as".to_string(),
                    description: "Emitted when properties change".to_string(),
                }],
                properties: vec![],
            },
        );

        // Introspectable interface
        self.dbus_interfaces.insert(
            canonical::INTROSPECTABLE_INTERFACE.to_string(),
            DbusInterface {
                name: canonical::INTROSPECTABLE_INTERFACE.to_string(),
                description: "Standard D-Bus introspection interface".to_string(),
                methods: vec![DbusMethod {
                    name: "Introspect".to_string(),
                    signature: "s".to_string(),
                    description: "Return XML introspection data".to_string(),
                }],
                signals: vec![],
                properties: vec![],
            },
        );

        // Peer interface
        self.dbus_interfaces.insert(
            canonical::PEER_INTERFACE.to_string(),
            DbusInterface {
                name: canonical::PEER_INTERFACE.to_string(),
                description: "Standard D-Bus peer interface".to_string(),
                methods: vec![
                    DbusMethod {
                        name: "Ping".to_string(),
                        signature: "".to_string(),
                        description: "Ping the peer".to_string(),
                    },
                    DbusMethod {
                        name: "GetMachineId".to_string(),
                        signature: "s".to_string(),
                        description: "Get the machine ID".to_string(),
                    },
                ],
                signals: vec![],
                properties: vec![],
            },
        );
    }

    /// Get the canonical D-Bus object path for this plugin
    pub fn dbus_path(&self) -> String {
        canonical::plugin_path("freedesktop")
    }

    /// Get the canonical D-Bus interface name for this plugin
    pub fn dbus_interface(&self) -> String {
        canonical::plugin_interface("freedesktop")
    }

    /// Get a registered D-Bus interface by name
    pub fn get_interface(&self, name: &str) -> Option<&DbusInterface> {
        self.dbus_interfaces.get(name)
    }

    /// Get all registered interfaces
    pub fn all_interfaces(&self) -> &HashMap<String, DbusInterface> {
        &self.dbus_interfaces
    }

    /// Validate that a D-Bus path follows the canonical convention
    pub fn validate_canonical_path(&self, path: &str) -> Result<()> {
        if canonical::is_canonical_plugin_path(path) {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Path '{}' is not in canonical form. Expected prefix: {}",
                path,
                canonical::PLUGIN_BASE_PATH
            ))
        }
    }

    /// Normalize a legacy path to canonical form
    pub fn normalize_path(&self, path: &str) -> Option<String> {
        canonical::normalize_plugin_path(path)
    }
}

impl Default for FreeDesktopPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StatePlugin for FreeDesktopPlugin {
    fn name(&self) -> &str {
        "freedesktop"
    }

    fn version(&self) -> &str {
        "2.0.0"
    }

    fn is_available(&self) -> bool {
        true
    }

    fn unavailable_reason(&self) -> String {
        String::new()
    }

    fn schema(&self) -> Option<PluginSchema> {
        self.schema.clone()
    }

    async fn calculate_diff(&self, _current: &Value, desired: &Value) -> anyhow::Result<StateDiff> {
        // FreeDesktop plugin is informational - we only check if desired state is valid
        let actions = vec![];

        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions,
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: String::new(),
                desired_hash: format!("{:x}", md5::compute(simd_json::to_string(desired)?)),
            },
        })
    }

    async fn apply_state(&self, _diff: &StateDiff) -> anyhow::Result<ApplyResult> {
        // FreeDesktop plugin is primarily informational/structural
        // No actual system changes are made
        Ok(ApplyResult {
            success: true,
            changes_applied: vec!["FreeDesktop configuration is informational only".to_string()],
            errors: vec![],
            checkpoint: None,
        })
    }

    async fn verify_state(&self, desired: &Value) -> anyhow::Result<bool> {
        // Verify the desired state has the correct structure
        let current_name = desired.get("name").and_then(|v| v.as_str());
        Ok(current_name == Some("freedesktop"))
    }

    async fn create_checkpoint(&self) -> anyhow::Result<Checkpoint> {
        Ok(Checkpoint {
            id: format!("freedesktop-{}", chrono::Utc::now().timestamp()),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::json!(null),
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> anyhow::Result<()> {
        // FreeDesktop plugin is informational, nothing to rollback
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

/// FreeDesktopPlugin configuration
#[derive(Debug, Clone)]
pub struct FreeDesktopConfig {
    pub enable_object_manager: bool,
    pub enable_properties: bool,
    pub enable_introspection: bool,
    pub enable_peer: bool,
}

impl Default for FreeDesktopConfig {
    fn default() -> Self {
        Self {
            enable_object_manager: true,
            enable_properties: true,
            enable_introspection: true,
            enable_peer: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_contract_requires_canonical_target_and_native_json_value() {
        let input: SetPropertyInput = serde_json::from_value(serde_json::json!({
            "plugin_id": "chat_router",
            "property_name": "selected_model",
            "value": "model-id"
        }))
        .unwrap();
        assert_eq!(input.interface_name, canonical::PLUGIN_INTERFACE);
        assert_eq!(input.value, serde_json::json!("model-id"));
        assert!(
            serde_json::from_value::<SetPropertyInput>(serde_json::json!({
                "property_name": "selected_model", "value": "model-id"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<SetPropertyInput>(serde_json::json!({
                "plugin_id": "chat_router", "property_name": "selected_model",
                "value": "model-id", "destination": "org.external.Service"
            }))
            .is_err()
        );
    }

    #[test]
    fn test_freedesktop_plugin_creation() {
        let plugin = FreeDesktopPlugin::new();
        assert_eq!(plugin.name(), "freedesktop");
        assert_eq!(plugin.version(), "2.0.0");
        assert!(plugin.is_available());
    }

    #[test]
    fn test_canonical_paths() {
        let plugin = FreeDesktopPlugin::new();

        // Verify canonical paths
        assert_eq!(plugin.dbus_path(), "/org/opdbus/v1/plugins/freedesktop");
        assert_eq!(plugin.dbus_interface(), "org.opdbus.v1.PluginV1");
    }

    #[test]
    fn test_registered_interfaces() {
        let plugin = FreeDesktopPlugin::new();

        // Check all standard interfaces are registered
        assert!(
            plugin
                .get_interface(canonical::OBJECT_MANAGER_INTERFACE)
                .is_some()
        );
        assert!(
            plugin
                .get_interface(canonical::PROPERTIES_INTERFACE)
                .is_some()
        );
        assert!(
            plugin
                .get_interface(canonical::INTROSPECTABLE_INTERFACE)
                .is_some()
        );
        assert!(plugin.get_interface(canonical::PEER_INTERFACE).is_some());
    }

    #[test]
    fn test_path_validation() {
        let plugin = FreeDesktopPlugin::new();

        // Valid canonical path
        assert!(
            plugin
                .validate_canonical_path("/org/opdbus/v1/plugins/test")
                .is_ok()
        );

        // Invalid legacy path (missing /org prefix)
        assert!(
            plugin
                .validate_canonical_path("/opdbus/v1/plugins/test")
                .is_err()
        );
    }

    #[test]
    fn test_path_normalization() {
        let plugin = FreeDesktopPlugin::new();

        // Legacy path should be normalized to canonical
        let normalized = plugin.normalize_path("/opdbus/v1/plugins/test");
        assert_eq!(normalized, Some("/org/opdbus/v1/plugins/test".to_string()));

        // Legacy alias also normalizes to canonical
        let canonical = plugin.normalize_path("/org/opdbus/v1/plugin/plugins/test");
        assert_eq!(canonical, Some("/org/opdbus/v1/plugins/test".to_string()));
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("freedesktop", |_ctx| std::sync::Arc::new(FreeDesktopPlugin::new()))
}
