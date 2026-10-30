# op-core Specification

**Version**: 0.1.0  
**Status**: Active Development  
**Last Updated**: 2026-02-16

## Table of Contents

1. [Purpose & Scope](#purpose--scope)
2. [Architecture](#architecture)
3. [Core Types](#core-types)
4. [API Contracts](#api-contracts)
5. [Error Handling](#error-handling)
6. [Execution Tracking](#execution-tracking)
7. [Security Model](#security-model)
8. [Configuration](#configuration)
9. [Testing Strategy](#testing-strategy)
10. [Integration Points](#integration-points)
11. [Performance Considerations](#performance-considerations)
12. [Future Enhancements](#future-enhancements)

---

## 1. Purpose & Scope

### 1.1 Purpose

`op-core` is the foundational crate for the operation-dbus project. It provides:

- **Core Types**: Common data structures used across all crates (BusType, ServiceInfo, ChatMessage, etc.)
- **Error Handling**: Unified error types and Result aliases
- **Execution Tracking**: Audit trail for all tool and agent executions
- **Security Model**: IP-based access zones and security levels
- **Configuration**: Environment variable loading and management
- **D-Bus Types**: Service, object, interface, method, signal, and property information

### 1.2 Scope

**In Scope:**
- Core data types shared across all crates
- Error types and conversion traits
- Execution record tracking and lifecycle
- Security level and access zone definitions
- Configuration loading from `/etc/op-dbus/environment`
- D-Bus introspection data structures
- Tool and agent definition types
- Chat message types for LLM interactions

**Out of Scope:**
- Actual D-Bus communication (handled by `op-introspection`)
- Tool execution logic (handled by `op-tools`)
- State management (handled by `op-state`)
- LLM provider implementations (handled by `op-llm`)
- Plugin implementations (handled by `op-plugins`)

### 1.3 Design Principles

1. **Zero Dependencies on Other op-* Crates**: `op-core` is the foundation and must not depend on any other op-* crates
2. **Minimal External Dependencies**: Only essential dependencies (serde, chrono, uuid, thiserror, zbus)
3. **SIMD JSON**: Use `simd-json` instead of `serde_json` for 2-3x performance improvement
4. **Type Safety**: Strong typing with enums for all categorical data
5. **Serialization First**: All types must be serializable for persistence and network transport
6. **Immutability Where Possible**: Prefer immutable types and builder patterns

---

## 2. Architecture

### 2.1 Module Structure

```
op-core/
├── src/
│   ├── lib.rs              # Public API exports
│   ├── types.rs            # Core data types (BusType, ServiceInfo, etc.)
│   ├── error.rs            # Error types and Result alias
│   ├── execution.rs        # Execution tracking (ExecutionRecord, ExecutionStatus)
│   ├── security.rs         # Security model (SecurityLevel, AccessZone)
│   ├── config.rs           # Configuration loading
│   ├── message.rs          # Message types (future: inter-component messaging)
│   ├── connection.rs       # Connection types (future: connection pooling)
│   └── self_identity.rs    # System identity (future: distributed identity)
└── Cargo.toml
```

### 2.2 Dependency Graph

```
op-core (no op-* dependencies)
  ├── serde (serialization)
  ├── simd-json (fast JSON)
  ├── uuid (unique IDs)
  ├── chrono (timestamps)
  ├── thiserror (error types)
  ├── anyhow (error handling)
  ├── zbus (D-Bus types)
  ├── tokio (async runtime)
  └── tracing (logging)
```

### 2.3 Component Relationships

```
┌─────────────────────────────────────────────────────────────┐
│                         op-core                              │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐   │
│  │  Types   │  │  Errors  │  │ Security │  │  Config  │   │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘   │
│       │             │              │              │          │
│       └─────────────┴──────────────┴──────────────┘          │
│                          │                                    │
│                  ┌───────▼────────┐                          │
│                  │   Execution    │                          │
│                  │    Tracking    │                          │
│                  └────────────────┘                          │
│                                                               │
└─────────────────────────────────────────────────────────────┘
                          │
                          │ Used by
                          ▼
        ┌─────────────────────────────────────┐
        │  All other op-* crates depend on    │
        │  op-core for types and errors       │
        └─────────────────────────────────────┘
```

---

## 3. Core Types

### 3.1 D-Bus Types

#### 3.1.1 BusType

Represents the type of D-Bus connection.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BusType {
    #[default]
    System,
    Session,
}
```

**Usage:**
- `System`: System bus (`/var/run/dbus/system_bus_socket`) - requires root or specific permissions
- `Session`: User session bus - per-user, no special permissions required

**Serialization:**
```json
"system"  // BusType::System
"session" // BusType::Session
```

#### 3.1.2 ServiceInfo

Information about a D-Bus service.

```rust
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
```

**Fields:**
- `name`: D-Bus service name (e.g., "org.freedesktop.NetworkManager")
- `bus_type`: Which bus the service is on
- `activatable`: Can be started on-demand via D-Bus activation
- `active`: Currently running
- `pid`: Process ID if active
- `uid`: User ID of the process

**Example:**
```json
{
  "name": "org.freedesktop.NetworkManager",
  "bus_type": "system",
  "activatable": true,
  "active": true,
  "pid": 1234,
  "uid": 0
}
```

#### 3.1.3 ObjectInfo

Information about a D-Bus object path.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectInfo {
    pub path: String,
    pub interfaces: Vec<InterfaceInfo>,
    #[serde(default)]
    pub children: Vec<String>,
}
```

**Fields:**
- `path`: Object path (e.g., "/org/freedesktop/NetworkManager")
- `interfaces`: List of interfaces implemented by this object
- `children`: Child object paths (for hierarchical objects)

#### 3.1.4 InterfaceInfo

Information about a D-Bus interface.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceInfo {
    pub name: String,
    pub methods: Vec<MethodInfo>,
    pub signals: Vec<SignalInfo>,
    pub properties: Vec<PropertyInfo>,
}
```

**Fields:**
- `name`: Interface name (e.g., "org.freedesktop.NetworkManager.Device")
- `methods`: Callable methods
- `signals`: Emitted signals
- `properties`: Readable/writable properties

#### 3.1.5 MethodInfo

Information about a D-Bus method.

```rust
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
```

**Fields:**
- `name`: Method name (e.g., "GetDevices")
- `in_args`: Input arguments
- `out_args`: Output arguments (return values)
- `annotations`: D-Bus annotations (e.g., deprecated, no-reply)

#### 3.1.6 SignalInfo

Information about a D-Bus signal.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalInfo {
    pub name: String,
    pub args: Vec<ArgInfo>,
}
```

**Fields:**
- `name`: Signal name (e.g., "DeviceAdded")
- `args`: Signal arguments

#### 3.1.7 PropertyInfo

Information about a D-Bus property.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyInfo {
    pub name: String,
    pub signature: String,
    pub access: PropertyAccess,
}
```

**Fields:**
- `name`: Property name (e.g., "State")
- `signature`: D-Bus type signature (e.g., "u" for uint32, "s" for string)
- `access`: Read, Write, or ReadWrite

**PropertyAccess:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PropertyAccess {
    Read,
    Write,
    ReadWrite,
}
```

#### 3.1.8 ArgInfo

Information about a method/signal argument.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArgInfo {
    pub name: Option<String>,
    pub signature: String,
    pub direction: ArgDirection,
}
```

**Fields:**
- `name`: Argument name (optional in D-Bus introspection)
- `signature`: D-Bus type signature
- `direction`: In or Out

**ArgDirection:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ArgDirection {
    #[default]
    In,
    Out,
}
```

### 3.2 Tool Types

#### 3.2.1 ToolDefinition

Defines a tool that can be executed.

```rust
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
```

**Fields:**
- `name`: Tool name (e.g., "list_services")
- `description`: Human-readable description
- `input_schema`: JSON Schema for input validation
- `schema_version`: Schema version (default: "1.0")
- `category`: Tool category (e.g., "dbus", "system", "network")
- `tags`: Searchable tags
- `namespace`: Tool namespace (e.g., "dbus", "systemd")

**Example:**
```json
{
  "name": "list_services",
  "description": "List all D-Bus services on the system bus",
  "input_schema": {
    "type": "object",
    "properties": {
      "bus_type": {
        "type": "string",
        "enum": ["system", "session"],
        "default": "system"
      }
    }
  },
  "schema_version": "1.0",
  "category": "dbus",
  "tags": ["dbus", "discovery", "services"],
  "namespace": "dbus"
}
```

#### 3.2.2 ToolRequest

Request to execute a tool.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRequest {
    pub id: String,
    pub tool_name: String,
    pub arguments: OwnedValue,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}
```

**Fields:**
- `id`: Unique request ID (UUID)
- `tool_name`: Name of tool to execute
- `arguments`: Tool arguments (JSON object)
- `timeout_ms`: Optional timeout in milliseconds

**Constructor:**
```rust
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
```

#### 3.2.3 ToolResult

Result of tool execution.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub id: String,
    pub success: bool,
    pub content: OwnedValue,
    #[serde(default)]
    pub error: Option<String>,
    pub execution_time_ms: u64,
}
```

**Fields:**
- `id`: Request ID (matches ToolRequest.id)
- `success`: Whether execution succeeded
- `content`: Result data (JSON value)
- `error`: Error message if failed
- `execution_time_ms`: Execution duration

**Constructors:**
```rust
impl ToolResult {
    pub fn success(id: impl Into<String>, content: OwnedValue, exec_time: u64) -> Self;
    pub fn error(id: impl Into<String>, error: impl Into<String>, exec_time: u64) -> Self;
}
```

### 3.3 Agent Types

#### 3.3.1 AgentDefinition

Defines an agent.

```rust
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
```

**Fields:**
- `id`: Unique agent ID
- `name`: Agent name
- `description`: What the agent does
- `capabilities`: List of capabilities (e.g., "network_management", "service_control")
- `tools`: List of tool names the agent can use
- `model`: LLM model to use (e.g., "gpt-4", "claude-3-opus")
- `config`: Agent-specific configuration

#### 3.3.2 AgentStatus

Agent execution status.

```rust
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
```

**States:**
- `Idle`: Agent created but not executing
- `Running`: Currently executing a task
- `Paused`: Execution paused (can be resumed)
- `Error`: Encountered an error
- `Stopped`: Permanently stopped

### 3.4 Chat Types

#### 3.4.1 ChatMessage

Message in a chat conversation.

```rust
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
```

**Fields:**
- `id`: Unique message ID
- `role`: Who sent the message (User, Assistant, System, Tool)
- `content`: Message text
- `timestamp`: When the message was created
- `tool_calls`: Tool calls made by the assistant
- `metadata`: Additional metadata

**Constructors:**
```rust
impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self;
    pub fn assistant(content: impl Into<String>) -> Self;
    pub fn system(content: impl Into<String>) -> Self;
}
```

#### 3.4.2 ChatRole

Role of a chat participant.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    User,
    Assistant,
    System,
    Tool,
}
```

**Roles:**
- `User`: Human user input
- `Assistant`: AI assistant response
- `System`: System messages (instructions, context)
- `Tool`: Tool execution results

#### 3.4.3 ToolCall

Tool call within a chat message.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub tool_name: String,
    pub arguments: OwnedValue,
    #[serde(default)]
    pub result: Option<ToolResult>,
}
```

**Fields:**
- `id`: Unique tool call ID
- `tool_name`: Name of tool to call
- `arguments`: Tool arguments
- `result`: Tool execution result (populated after execution)

### 3.5 Health & Monitoring Types

#### 3.5.1 HealthStatus

Overall system health.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub healthy: bool,
    pub version: String,
    pub uptime_secs: u64,
    pub components: HashMap<String, ComponentHealth>,
}
```

**Fields:**
- `healthy`: Overall health (true if all components healthy)
- `version`: System version
- `uptime_secs`: Seconds since startup
- `components`: Health of individual components

#### 3.5.2 ComponentHealth

Health of a single component.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentHealth {
    pub name: String,
    pub status: ComponentStatus,
    #[serde(default)]
    pub message: Option<String>,
    pub last_check: DateTime<Utc>,
}
```

**Fields:**
- `name`: Component name
- `status`: Health status
- `message`: Optional status message
- `last_check`: When health was last checked

#### 3.5.3 ComponentStatus

Component health status.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ComponentStatus {
    Healthy,
    Degraded,
    Unhealthy,
    #[default]
    Unknown,
}
```

**States:**
- `Healthy`: Component functioning normally
- `Degraded`: Component functioning but with issues
- `Unhealthy`: Component not functioning
- `Unknown`: Health status unknown

### 3.6 Schema Reference Types

#### 3.6.1 ObjectSchemaRef

Reference to a D-Bus object schema stored in StateStore.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectSchemaRef {
    pub object_type: String,
    pub namespace: String,
    pub path: String,
    pub schema_hash: String,
}
```

**Fields:**
- `object_type`: Type of object (e.g., "dbus_interface", "dbus_service")
- `namespace`: D-Bus service name (e.g., "org.freedesktop.NetworkManager")
- `path`: D-Bus object path (e.g., "/org/freedesktop/NetworkManager")
- `schema_hash`: SHA-256 hash of the interface schema for integrity verification

**Purpose:**
Used to link plugins to their discovered D-Bus interfaces. These schemas are persisted and restored during disaster recovery.

**Constructor:**
```rust
impl ObjectSchemaRef {
    pub fn new(
        object_type: impl Into<String>,
        namespace: impl Into<String>,
        path: impl Into<String>,
        schema_hash: impl Into<String>,
    ) -> Self;
}
```

---

## 4. API Contracts

### 4.1 Public API Surface

All types in `op-core` are public and re-exported from `lib.rs`:

```rust
// Core types
pub mod types;
pub use types::*;

// Error handling
pub mod error;
pub use error::{Error, Result};

// Execution tracking
pub mod execution;
pub use execution::*;

// Security
pub mod security;
pub use security::*;

// Configuration
pub mod config;
pub use config::*;
```

### 4.2 Type Guarantees

All public types guarantee:

1. **Serialization**: All types implement `Serialize` and `Deserialize`
2. **Debug**: All types implement `Debug` for logging
3. **Clone**: Most types implement `Clone` for easy copying
4. **Send + Sync**: All types are thread-safe (where applicable)

### 4.3 Stability Guarantees

- **Semver**: Breaking changes only in major versions
- **Deprecation**: Deprecated items marked with `#[deprecated]` and kept for one major version
- **Additions**: New fields added with `#[serde(default)]` to maintain backward compatibility

---

## 5. Error Handling

### 5.1 Error Type

```rust
#[derive(Error, Debug)]
pub enum Error {
    #[error("DBus error: {0}")]
    Dbus(#[from] zbus::Error),

    #[error("DBus FDO error: {0}")]
    DbusFdo(#[from] zbus::fdo::Error),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Introspection error: {0}")]
    Introspection(String),

    #[error("Tool execution error: {0}")]
    ToolExecution(String),

    #[error("Plugin error: {0}")]
    Plugin(String),

    #[error("Agent error: {0}")]
    Agent(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] simd_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Internal error: {0}")]
    Internal(String),
}
```

### 5.2 Result Type Alias

```rust
pub type Result<T> = std::result::Result<T, Error>;
```

### 5.3 Error Construction Helpers

```rust
impl Error {
    pub fn connection(msg: impl Into<String>) -> Self;
    pub fn introspection(msg: impl Into<String>) -> Self;
    pub fn tool_execution(msg: impl Into<String>) -> Self;
    pub fn plugin(msg: impl Into<String>) -> Self;
    pub fn agent(msg: impl Into<String>) -> Self;
    pub fn not_found(msg: impl Into<String>) -> Self;
    pub fn internal(msg: impl Into<String>) -> Self;
}
```

### 5.4 Error Conversion

```rust
impl From<anyhow::Error> for Error {
    fn from(err: anyhow::Error) -> Self {
        Error::Internal(err.to_string())
    }
}
```

### 5.5 Error Handling Patterns

**Pattern 1: Early Return**
```rust
fn do_something() -> Result<()> {
    let value = get_value()?;
    process(value)?;
    Ok(())
}
```

**Pattern 2: Context**
```rust
fn do_something() -> Result<()> {
    get_value()
        .map_err(|e| Error::internal(format!("Failed to get value: {}", e)))?;
    Ok(())
}
```

**Pattern 3: Match**
```rust
match do_something() {
    Ok(result) => handle_success(result),
    Err(Error::NotFound(_)) => handle_not_found(),
    Err(Error::PermissionDenied(_)) => handle_permission_denied(),
    Err(e) => handle_other_error(e),
}
```

---

## 6. Execution Tracking

### 6.1 ExecutionStatus

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Timeout,
}
```

### 6.2 ExecutionRecord

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub id: String,
    pub trace_id: String,
    pub tool_name: String,
    pub input_summary: Option<simd_json::OwnedValue>,
    pub status: ExecutionStatus,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<u64>,
    pub output_summary: Option<String>,
    pub error: Option<String>,
    pub success: bool,
    pub initiated_by: Option<String>,
    pub metadata: HashMap<String, String>,
}
```

### 6.3 ExecutionRecord Lifecycle

```rust
impl ExecutionRecord {
    // Create new record
    pub fn new(tool_name: &str, trace_id: Option<String>) -> Self;
    
    // Lifecycle methods
    pub fn start(&mut self);
    pub fn complete(&mut self, output: Option<String>);
    pub fn fail(&mut self, error: String);
    pub fn cancel(&mut self);
    pub fn timeout(&mut self);
    
    // Query methods
    pub fn is_running(&self) -> bool;
    pub fn is_complete(&self) -> bool;
    pub fn is_failed(&self) -> bool;
}
```

### 6.4 Usage Example

```rust
use op_core::execution::{ExecutionRecord, ExecutionStatus};

// Create execution record
let mut record = ExecutionRecord::new("list_services", None);

// Start execution
record.start();

// Execute tool
match execute_tool() {
    Ok(result) => {
        record.complete(Some(result));
    }
    Err(e) => {
        record.fail(e.to_string());
    }
}

// Record is now complete with timing and status
assert_eq!(record.status, ExecutionStatus::Completed);
assert!(record.duration_ms.is_some());
```

---

*This is Part 1 of the op-core specification. Continue to Part 2 for Security Model, Configuration, Testing Strategy, Integration Points, Performance Considerations, and Future Enhancements.*
