use op_state_store::PluginSchema;
// Core trait for pluggable state management
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::collections::HashMap;

use chrono::{DateTime, Utc};

/// Desired state configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesiredState {
    pub state: Value,
    pub timestamp: DateTime<Utc>,
    pub hash: String,
    pub description: Option<String>,
    pub source: StateSource,
}

impl DesiredState {
    pub fn new(state: Value) -> Self {
        let hash = format!(
            "{:x}",
            md5::compute(simd_json::to_string(&state).unwrap_or_default())
        );
        Self {
            state,
            timestamp: Utc::now(),
            hash,
            description: None,
            source: StateSource::User,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StateSource {
    User,
    AutoDiscovered,
    Import(String),
    Plugin(String),
    Default,
}

/// Represents a change to be applied
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateChange {
    pub operation: ChangeOperation,
    pub path: String,
    pub old_value: Option<Value>,
    pub new_value: Option<Value>,
    pub description: String,
    pub hash: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeOperation {
    Create,
    Update,
    Delete,
    NoOp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<String>,
    pub suggestions: Vec<String>,
}

impl ValidationResult {
    pub fn success() -> Self {
        Self {
            valid: true,
            errors: vec![],
            warnings: vec![],
            suggestions: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub path: String,
    pub message: String,
    pub code: String,
}

/// Plugin metadata
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginMetadata {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: Option<String>,
    pub license: Option<String>,
    pub dependencies: Vec<String>,
    pub dbus_services: Vec<String>,
    pub feature_schemas: Vec<Value>,
    pub object_schemas: HashMap<String, Value>,
}

/// Core trait that all state management plugins must implement
#[async_trait]
pub trait StatePlugin: Send + Sync {
    /// Get the plugin metadata including description and schemas
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: self.name().to_string(),
            version: self.version().to_string(),
            description: format!("{} plugin", self.name()),
            author: None,
            license: None,
            dependencies: vec![],
            dbus_services: vec![],
            feature_schemas: vec![],
            object_schemas: HashMap::new(),
        }
    }

    /// Get the plugin structured schema if available
    fn schema(&self) -> Option<PluginSchema> {
        None
    }

    /// Get the plugin structured schema with any live-probed present-state
    /// folded in (e.g. a real reachability check against a remote backend),
    /// for callers that seal/publish present-state and want it to reflect
    /// reality rather than static declarations.
    ///
    /// Default: `None`, meaning "no live variant — use [`schema`](Self::schema)".
    /// Plugins with something worth probing live (a remote API, a daemon
    /// socket) override this; everyone else is unaffected.
    async fn schema_live(&self) -> Option<PluginSchema> {
        None
    }

    /// Plugin identifier (e.g., "network", "filesystem", "user")
    fn name(&self) -> &str;

    /// Plugin version for compatibility checking
    #[allow(dead_code)]
    fn version(&self) -> &str;

    /// Check if this plugin's dependencies are available on the system
    fn is_available(&self) -> bool {
        true
    }

    /// Get a message explaining why the plugin is unavailable
    fn unavailable_reason(&self) -> String {
        format!("Plugin '{}' is not available", self.name())
    }

    /// Calculate difference between current and desired state
    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff>;

    /// Apply the state changes
    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult>;

    /// Verify that current state matches desired state
    #[allow(dead_code)]
    async fn verify_state(&self, desired: &Value) -> Result<bool>;

    /// Create a checkpoint for rollback capability
    async fn create_checkpoint(&self) -> Result<Checkpoint>;

    /// Rollback to a previous checkpoint
    #[allow(dead_code)]
    async fn rollback(&self, checkpoint: &Checkpoint) -> Result<()>;

    /// Get plugin capabilities and limitations
    #[allow(dead_code)]
    fn capabilities(&self) -> PluginCapabilities;
}

/// Represents the difference between current and desired state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDiff {
    pub plugin: String,
    pub actions: Vec<StateAction>,
    pub metadata: DiffMetadata,
}

/// Metadata about the diff calculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffMetadata {
    pub timestamp: i64,
    pub current_hash: String,
    pub desired_hash: String,
}

/// Actions to be performed on resources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StateAction {
    Create { resource: String, config: Value },
    Modify { resource: String, changes: Value },
    Delete { resource: String },
    NoOp { resource: String },
}

/// Result of applying state changes
#[derive(Debug, Serialize, Deserialize)]
pub struct ApplyResult {
    pub success: bool,
    pub changes_applied: Vec<String>,
    pub errors: Vec<String>,
    pub checkpoint: Option<Checkpoint>,
}

/// Checkpoint for rollback
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub plugin: String,
    pub timestamp: i64,
    pub state_snapshot: Value,
    pub backend_checkpoint: Option<Value>,
}

// PluginCapabilities is now canonically defined in op_state_store::plugin_schema
// and re-exported from the op_state_store crate root.  This file re-exports it
// so existing `use op_state::PluginCapabilities` call sites continue to work.
pub use op_state_store::PluginCapabilities;

#[cfg(test)]
mod tests {
    use super::PluginCapabilities as ReExportedCaps;
    use op_state_store::PluginCapabilities as CanonicalCaps;

    /// VAL-DEDUP-002: op-state must not define its own PluginCapabilities
    /// type. The re-exported type must be the exact same type as the canonical
    /// one in op_state_store (type identity, not a duplicate definition).
    #[test]
    fn plugin_capabilities_is_reexport_from_op_state_store() {
        use std::any::TypeId;
        assert_eq!(
            TypeId::of::<ReExportedCaps>(),
            TypeId::of::<CanonicalCaps>(),
            "op_state::PluginCapabilities must be the same type as \
             op_state_store::PluginCapabilities, not a separate struct"
        );
    }

    /// VAL-DEDUP-004: callers that `use op_state::PluginCapabilities` are
    /// transitively importing from op_state_store.  The 4-field guarantee
    /// block must be the one they get.
    #[test]
    fn reexported_plugin_capabilities_has_four_bool_fields() {
        let caps = ReExportedCaps::default();
        assert!(!caps.supports_rollback);
        assert!(!caps.supports_checkpoints);
        assert!(!caps.supports_verification);
        assert!(!caps.atomic_operations);
    }

    /// The serialized form must contain exactly the four canonical keys.
    #[test]
    fn reexported_plugin_capabilities_serializes_four_keys() {
        let caps = ReExportedCaps {
            supports_rollback: true,
            supports_checkpoints: false,
            supports_verification: true,
            atomic_operations: false,
        };
        let json = serde_json::to_value(&caps).unwrap();
        let obj = json.as_object().unwrap();
        assert_eq!(obj.len(), 4);
        assert_eq!(obj["supports_rollback"], serde_json::json!(true));
        assert_eq!(obj["supports_checkpoints"], serde_json::json!(false));
        assert_eq!(obj["supports_verification"], serde_json::json!(true));
        assert_eq!(obj["atomic_operations"], serde_json::json!(false));
    }

    /// The old 8-field access-control fields (can_read, can_write, can_delete,
    /// requires_root, supported_platforms) must not be present in the
    /// serialized form.  They are superseded by MethodDecl.required_capability.
    #[test]
    fn reexported_plugin_capabilities_has_no_access_control_fields() {
        let caps = ReExportedCaps::default();
        let json = serde_json::to_string(&caps).unwrap();
        assert!(
            !json.contains("can_read"),
            "can_read must not exist in PluginCapabilities"
        );
        assert!(
            !json.contains("can_write"),
            "can_write must not exist in PluginCapabilities"
        );
        assert!(
            !json.contains("can_delete"),
            "can_delete must not exist in PluginCapabilities"
        );
        assert!(
            !json.contains("requires_root"),
            "requires_root must not exist in PluginCapabilities"
        );
        assert!(
            !json.contains("supported_platforms"),
            "supported_platforms must not exist in PluginCapabilities"
        );
    }
}
