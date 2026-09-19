#![recursion_limit = "512"]

//! OP State Store - schema, in-memory state, and event-chain primitives.
//!
//! Features:
//! - In-memory StateStore implementation for ephemeral runtime wiring
//! - Prometheus metrics
//! - Plugin schema catalog with JSON Schema 2026 support
//! - Disaster recovery export/import
//! - OpenTelemetry tracing integration
//! - Snowball-style event chain for compliance and reproducibility
//! - Schema-aware canonical hashing with Merkle batching

pub mod disaster_recovery;
pub mod error;
pub mod event_chain;
pub mod exact_schema;
pub mod execution_job;
pub mod memory_store;
pub mod metrics;
pub mod plugin_schema;
pub mod public_state;
pub mod schema_projection;
pub mod schema_validator;
pub mod state_schema_shape;
pub mod state_store;

pub use disaster_recovery::{
    get_global_dependencies, get_plugin_dependencies, DisasterRecoveryExport, HostInfo,
    PluginStateExport, RestoreResult, SystemDependency,
};
pub use error::StateStoreError;
pub use event_chain::{
    ActionOrigin, ChainConfig, ChainEvent, ChainVerificationResult, Decision, DenyReason,
    EventBatch, EventChain, MerkleProof, OperationType, StateSnapshot, TagImmutabilityProof,
};
pub use execution_job::{ExecutionJob, ExecutionResult, ExecutionStatus};
pub use memory_store::MemoryStore;
pub use plugin_schema::{
    dialects, CapabilityDecl, Constraint, FieldSchema, FieldType, FieldWriteDecl, MethodDecl,
    PluginCapabilities, PluginSchema, ReadOnlyCondition, SchemaCatalog, SchemaLoadError,
    SchemaRegistry, SideEffect, SignalDecl, ValidationResult as SchemaValidationResult,
    DEFAULT_SCHEMA_DIALECT,
};
pub use public_state::{PublicStatePolicy, PublicStateProjectionError};
pub use schema_validator::{
    canonicalize_json, SchemaValidator, ValidationError, ValidationReport, ValidatorError,
};
pub use state_store::StateStore;

use serde::{Deserialize, Serialize};

/// A stored object for export/import
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StoredObject {
    pub id: String,
    pub object_type: String,
    pub namespace: String,
    pub data: simd_json::OwnedValue,
}

/// Export data structure for disaster recovery
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CanonicalDbExport {
    pub objects: Vec<StoredObject>,
    pub executions: Vec<simd_json::OwnedValue>,
    pub snowball: Vec<simd_json::OwnedValue>,
}
