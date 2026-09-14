//! op-state: State Plugin System
//!
//! Provides:
//! - StatePlugin trait for pluggable state management
//! - Crypto utilities for state hashing/signing
//! - Schema-catalog-backed validation
//! - Plugin tree for hierarchical state
//! - StateStore trait access via op-state-store
//! - Auto-plugin generation
//!
//! StateManager has been excised — mutations go through the MutationEngine
//! (the single write door), and the projected tree reads 1:1 from shm. There
//! is no "manager" because there is no desired-vs-actual gap to reconcile:
//! after mutation, the state IS what the mutation made it.

pub mod authority;
// pub mod auto_plugin;
pub mod crypto;
pub mod dbus_plugin_base;
pub mod dbus_server;
pub mod plugin;
pub mod plugin_workflow;
pub mod plugtree;
pub mod schema_validator;

pub use plugin::{
    ApplyResult, ChangeOperation, Checkpoint, DesiredState, DiffMetadata, PluginCapabilities,
    PluginMetadata, StateAction, StateChange, StateDiff, StatePlugin, StateSource, ValidationError,
    ValidationResult,
};
pub use plugtree::PlugTree;

// Re-export state store types
pub use op_state_store::{
    ExecutionJob, ExecutionResult, ExecutionStatus, PluginSchema, SchemaCatalog, SchemaRegistry,
    StateStore, StateStoreError,
};

/// Prelude for convenient imports
pub mod prelude {
    pub use super::plugin::{
        ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateAction, StateDiff,
        StatePlugin,
    };
    pub use super::plugtree::PlugTree;
    // State store types
    pub use op_state_store::{
        ExecutionJob, ExecutionStatus, PluginSchema, SchemaCatalog, SchemaRegistry, StateStore,
    };
}
