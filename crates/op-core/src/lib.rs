//! Core types and utilities for op-dbus-v2
//!
//! # Modules
//!
//! - `config`: Configuration management
//! - `error`: Error types and Result alias
//! - `security`: Security types (IP access, permissions)
//! - `self_identity`: Self repository identification
//! - `types`: Common types used across op-dbus-v2

pub mod config;
pub mod error;
pub mod execution;
pub mod projection_shm;
pub mod runit;
pub mod security;
pub mod self_identity;
pub mod state_publisher;
pub mod types;

// Re-exports
pub use error::{Error, Result};
pub use execution::{ExecutionRecord, ExecutionStats, ExecutionStatus, ExecutionTracker};
pub use security::{AccessZone, NetworkConfig, SecurityLevel};
pub use self_identity::{get_self_repo_path, SelfRepositoryInfo};
pub use types::{
    AgentDefinition, AgentStatus, ArgDirection, ArgInfo, BusType, ChatMessage, ChatRole,
    ComponentHealth, ComponentStatus, HealthStatus, InterfaceInfo, MethodInfo, ObjectInfo,
    ObjectSchemaRef, PropertyAccess, PropertyInfo, ServiceInfo, SignalInfo, ToolCall,
    ToolDefinition, ToolRequest, ToolResult,
};
