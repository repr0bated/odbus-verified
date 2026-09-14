//! Internal message types for actor communication

use crate::types::*;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

/// Message envelope for actor mailbox
#[derive(Debug)]
pub struct Message {
    pub id: String,
    pub kind: MessageKind,
    pub reply_to: Option<oneshot::Sender<Response>>,
}

impl Message {
    pub fn new(kind: MessageKind) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            kind,
            reply_to: None,
        }
    }

    pub fn with_reply(kind: MessageKind, reply_to: oneshot::Sender<Response>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            kind,
            reply_to: Some(reply_to),
        }
    }
}

/// Message types for the actor system
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum MessageKind {
    // Chat messages
    Chat(ChatRequest),
    ChatStream(ChatStreamRequest),

    // Tool operations
    ListTools,
    ExecuteTool(ToolRequest),

    // Agent operations
    ListAgents,
    StartAgent(String),
    StopAgent(String),
    AgentStatus(String),

    // Introspection
    Introspect(IntrospectRequest),
    ListServices(BusType),

    // DBus operations
    DbusCall(DbusCallRequest),
    DbusGetProperty(DbusPropertyRequest),
    DbusSetProperty(DbusPropertySetRequest),

    // System
    Health,
    Shutdown,

    // Plugin operations
    ListPlugins,
    LoadPlugin(String),
    UnloadPlugin(String),
}

/// Chat request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

/// Streaming chat request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatStreamRequest {
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub model: Option<String>,
}

/// Introspection request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntrospectRequest {
    pub bus_type: BusType,
    pub service: String,
    #[serde(default)]
    pub path: Option<String>,
}

/// DBus method call request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbusCallRequest {
    pub bus_type: BusType,
    pub destination: String,
    pub path: String,
    pub interface: String,
    pub method: String,
    #[serde(default)]
    pub args: Vec<simd_json::OwnedValue>,
}

/// DBus property get request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbusPropertyRequest {
    pub bus_type: BusType,
    pub destination: String,
    pub path: String,
    pub interface: String,
    pub property: String,
}

/// DBus property set request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbusPropertySetRequest {
    pub bus_type: BusType,
    pub destination: String,
    pub path: String,
    pub interface: String,
    pub property: String,
    pub value: simd_json::OwnedValue,
    pub signature: String,
}

/// Response from actor operations
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum Response {
    Success(simd_json::OwnedValue),
    Error { code: String, message: String },

    // Specific responses
    Tools(Vec<ToolDefinition>),
    ToolResult(ToolResult),

    Agents(Vec<AgentDefinition>),
    AgentStatus(AgentStatus),

    Services(Vec<ServiceInfo>),
    Introspection(ObjectInfo),

    Chat(ChatMessage),

    Health(HealthStatus),

    Plugins(Vec<PluginInfo>),

    Ack,
}

impl Response {
    pub fn success(value: impl Serialize) -> Self {
        Response::Success(simd_json::serde::to_owned_value(value).unwrap_or(simd_json::OwnedValue::Null))
    }

    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Response::Error {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn is_success(&self) -> bool {
        !matches!(self, Response::Error { .. })
    }
}

/// Plugin information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
    pub tools: Vec<String>,
}
