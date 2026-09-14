//! Common types used across op-dbus-v2

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue;
use simd_json::ValueBuilder;
use std::collections::HashMap;
use uuid::Uuid;

/// Bus type for DBus connections
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BusType {
    #[default]
    System,
    Session,
}

impl std::fmt::Display for BusType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BusType::System => write!(f, "system"),
            BusType::Session => write!(f, "session"),
        }
    }
}

/// DBus service information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceInfo {
    pub name: String,
    pub bus_type: BusType,
    pub activatable: bool,
    pub active: bool,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub uid: Option<u32>,
}

/// DBus object path information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectInfo {
    pub path: String,
    pub interfaces: Vec<InterfaceInfo>,
    #[serde(default)]
    pub children: Vec<String>,
}

/// DBus interface information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceInfo {
    pub name: String,
    pub methods: Vec<MethodInfo>,
    pub signals: Vec<SignalInfo>,
    pub properties: Vec<PropertyInfo>,
}

/// DBus method information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodInfo {
    pub name: String,
    #[serde(default)]
    pub in_args: Vec<ArgInfo>,
    #[serde(default)]
    pub out_args: Vec<ArgInfo>,
    #[serde(default)]
    pub annotations: HashMap<String, String>,
}

/// DBus signal information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalInfo {
    pub name: String,
    pub args: Vec<ArgInfo>,
}

/// DBus property information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyInfo {
    pub name: String,
    pub signature: String,
    pub access: PropertyAccess,
}

/// DBus method/signal argument
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArgInfo {
    pub name: Option<String>,
    pub signature: String,
    pub direction: ArgDirection,
}

/// Argument direction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ArgDirection {
    #[default]
    In,
    Out,
}

/// Property access mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PropertyAccess {
    Read,
    Write,
    ReadWrite,
}

/// Tool definition (local to avoid cycle)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: OwnedValue,
    #[serde(default)]
    pub schema_version: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub namespace: String,
}

/// Tool execution request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRequest {
    pub id: String,
    pub tool_name: String,
    pub arguments: OwnedValue,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

impl ToolRequest {
    pub fn new(tool_name: impl Into<String>, arguments: OwnedValue) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            tool_name: tool_name.into(),
            arguments,
            timeout_ms: None,
        }
    }
}

/// Tool execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub id: String,
    pub success: bool,
    pub content: OwnedValue,
    #[serde(default)]
    pub error: Option<String>,
    pub execution_time_ms: u64,
}

impl ToolResult {
    pub fn success(id: impl Into<String>, content: OwnedValue, exec_time: u64) -> Self {
        Self {
            id: id.into(),
            success: true,
            content,
            error: None,
            execution_time_ms: exec_time,
        }
    }

    pub fn error(id: impl Into<String>, error: impl Into<String>, exec_time: u64) -> Self {
        Self {
            id: id.into(),
            success: false,
            content: OwnedValue::null(),
            error: Some(error.into()),
            execution_time_ms: exec_time,
        }
    }
}

/// Agent definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub tools: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub config: HashMap<String, OwnedValue>,
}

/// Agent status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    #[default]
    Idle,
    Running,
    Paused,
    Error,
    Stopped,
}

/// Chat message for AI interactions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: ChatRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub metadata: HashMap<String, OwnedValue>,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: ChatRole::User,
            content: content.into(),
            timestamp: Utc::now(),
            tool_calls: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: ChatRole::Assistant,
            content: content.into(),
            timestamp: Utc::now(),
            tool_calls: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: ChatRole::System,
            content: content.into(),
            timestamp: Utc::now(),
            tool_calls: Vec::new(),
            metadata: HashMap::new(),
        }
    }
}

/// Chat role
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    User,
    Assistant,
    System,
    Tool,
}

/// Tool call within a chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub tool_name: String,
    pub arguments: OwnedValue,
    #[serde(default)]
    pub result: Option<ToolResult>,
}

/// System health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub healthy: bool,
    pub version: String,
    pub uptime_secs: u64,
    pub components: HashMap<String, ComponentHealth>,
}

/// Component health information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentHealth {
    pub name: String,
    pub status: ComponentStatus,
    #[serde(default)]
    pub message: Option<String>,
    pub last_check: DateTime<Utc>,
}

/// Component status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ComponentStatus {
    Healthy,
    Degraded,
    Unhealthy,
    #[default]
    Unknown,
}

// ============================================================================
// OBJECT SCHEMA REFERENCE (for D-Bus schema linking)
// ============================================================================

/// Reference to a D-Bus object schema stored in StateStore
///
/// Used to link plugins to their discovered D-Bus interfaces.
/// These schemas are persisted and restored during disaster recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectSchemaRef {
    /// Object type (e.g., "dbus_interface", "dbus_service")
    pub object_type: String,
    /// Namespace - typically D-Bus service name (e.g., "org.freedesktop.NetworkManager")
    pub namespace: String,
    /// D-Bus object path (e.g., "/org/freedesktop/NetworkManager")
    pub path: String,
    /// Hash of the interface schema for integrity verification
    pub schema_hash: String,
}

impl ObjectSchemaRef {
    pub fn new(
        object_type: impl Into<String>,
        namespace: impl Into<String>,
        path: impl Into<String>,
        schema_hash: impl Into<String>,
    ) -> Self {
        Self {
            object_type: object_type.into(),
            namespace: namespace.into(),
            path: path.into(),
            schema_hash: schema_hash.into(),
        }
    }
}
