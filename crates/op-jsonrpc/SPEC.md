# op-jsonrpc - Specification

## Overview
**Crate**: `op-jsonrpc`  
**Location**: `crates/op-jsonrpc`  
**Description**: JSON-RPC server with OVSDB and NonNet database support for op-dbus-v2

## Purpose

The `op-jsonrpc` crate provides a comprehensive JSON-RPC 2.0 server implementation with specialized support for Open vSwitch Database (OVSDB) integration and NonNet database management. It serves as the RPC communication layer for the operation-dbus system, enabling:

- **JSON-RPC 2.0 Protocol**: Standards-compliant RPC server over Unix sockets
- **OVSDB Integration**: Client for Open vSwitch database operations
- **NonNet Database**: State management for non-network plugins
- **Async Operations**: Non-blocking I/O with tokio runtime

This crate is essential for:
- Plugin communication via JSON-RPC
- Network configuration through OVSDB
- State persistence for non-network components
- Inter-process communication within operation-dbus

## Architecture

### Protocol Layer
Implements JSON-RPC 2.0 specification:
- Request/response message types
- Error handling with standard error codes
- Batch request support
- Notification messages (no response expected)

### Server Layer
Unix socket-based JSON-RPC server:
- Async request handling
- Method routing and dispatch
- Connection management
- Error recovery

### Database Integrations

#### OVSDB Client
Connects to Open vSwitch database for network operations:
- Schema introspection
- Transactional operations
- Monitor/update subscriptions
- Bridge and port management

#### NonNet Database
Custom database for non-network plugin state:
- Key-value storage
- Plugin configuration persistence
- State synchronization
- Query and update operations

## Key Components

### JsonRpcRequest
Represents a JSON-RPC 2.0 request.

```rust
pub struct JsonRpcRequest {
    pub jsonrpc: String,    // Protocol version ("2.0")
    pub method: String,     // Method name to invoke
    pub params: Value,      // Method parameters (JSON)
    pub id: Value,          // Request identifier
}
```

**Constructors**:
```rust
// Create with auto-generated ID
JsonRpcRequest::new("method_name", params)

// Create with specific ID
JsonRpcRequest::with_id("method_name", params, id)
```

### JsonRpcResponse
Represents a JSON-RPC 2.0 response.

```rust
pub struct JsonRpcResponse {
    pub jsonrpc: String,              // Protocol version ("2.0")
    pub result: Option<Value>,        // Success result
    pub error: Option<JsonRpcError>,  // Error details
    pub id: Value,                    // Request ID
}
```

**Constructors**:
```rust
// Success response
JsonRpcResponse::success(id, result)

// Error response
JsonRpcResponse::error(id, code, message)

// Error with additional data
JsonRpcResponse::error_with_data(id, code, message, data)
```

### JsonRpcError
Standard JSON-RPC error structure.

```rust
pub struct JsonRpcError {
    pub code: i32,           // Error code
    pub message: String,     // Error message
    pub data: Option<Value>, // Additional error data
}
```

**Standard Error Codes**:
- `-32700`: Parse error
- `-32600`: Invalid request
- `-32601`: Method not found
- `-32602`: Invalid params
- `-32603`: Internal error
- `-32000 to -32099`: Server-defined errors

### JsonRpcServer
Async JSON-RPC server over Unix sockets.

```rust
pub struct JsonRpcServer {
    // Server configuration and state
}
```

**Key Methods**:
- `new(socket_path)`: Create server bound to Unix socket
- `register_method(name, handler)`: Register RPC method handler
- `run()`: Start server event loop
- `shutdown()`: Graceful shutdown

### OvsdbClient
Client for Open vSwitch database operations.

```rust
pub struct OvsdbClient {
    // OVSDB connection and state
}
```

**Key Operations**:
- `connect(socket)`: Connect to OVSDB server
- `list_dbs()`: List available databases
- `get_schema(db)`: Retrieve database schema
- `transact(operations)`: Execute transactional operations
- `monitor(db, tables)`: Subscribe to table updates

### NonNetDb
Database for non-network plugin state.

```rust
pub struct NonNetDb {
    // Database state and storage
}
```

**Key Operations**:
- `new(path)`: Create/open database at path
- `get(key)`: Retrieve value by key
- `set(key, value)`: Store key-value pair
- `delete(key)`: Remove key
- `list()`: List all keys
- `query(filter)`: Query with filter criteria

## Module Structure

### Core Modules
- **protocol**: JSON-RPC 2.0 message types and builders
- **server**: Unix socket server implementation
- **ovsdb**: OVSDB client and operations
- **nonnet**: NonNet database implementation

### Supporting Modules
- **ovsdb_jsonrpc**: OVSDB-specific JSON-RPC extensions
- **nonnet_staging**: Staging area for NonNet operations

## Dependencies

### Core Dependencies
- **op-core**: Core types and utilities
- **tokio**: Async runtime and I/O
- **serde**: Serialization framework
- **simd-json**: High-performance JSON parsing
- **uuid**: Request ID generation

### Error Handling
- **anyhow**: Flexible error handling
- **thiserror**: Custom error types

### Logging
- **tracing**: Structured logging and diagnostics

## Usage

### Starting a JSON-RPC Server

```rust
use op_jsonrpc::{JsonRpcServer, JsonRpcRequest, JsonRpcResponse};
use simd_json::json;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create server
    let mut server = JsonRpcServer::new("/tmp/op-jsonrpc.sock").await?;
    
    // Register method handler
    server.register_method("echo", |req: JsonRpcRequest| async move {
        JsonRpcResponse::success(req.id, req.params)
    });
    
    // Run server
    server.run().await?;
    
    Ok(())
}
```

### OVSDB Integration

```rust
use op_jsonrpc::OvsdbClient;

// Connect to OVSDB
let mut client = OvsdbClient::connect("/var/run/openvswitch/db.sock").await?;

// List databases
let dbs = client.list_dbs().await?;
println!("Available databases: {:?}", dbs);

// Get schema
let schema = client.get_schema("Open_vSwitch").await?;

// Execute transaction
let result = client.transact("Open_vSwitch", vec![
    // Transaction operations
]).await?;
```

### NonNet Database Operations

```rust
use op_jsonrpc::NonNetDb;
use simd_json::json;

// Open database
let db = NonNetDb::new("/var/lib/op-dbus/nonnet.db").await?;

// Store plugin state
db.set("plugin.my-service.config", json!({
    "enabled": true,
    "port": 8080
})).await?;

// Retrieve state
let config = db.get("plugin.my-service.config").await?;

// Query with filter
let all_configs = db.query("plugin.*.config").await?;
```

### Method Registration

```rust
// Register multiple methods
server.register_method("add", |req| async move {
    let params = req.params.as_array().unwrap();
    let a = params[0].as_i64().unwrap();
    let b = params[1].as_i64().unwrap();
    JsonRpcResponse::success(req.id, json!(a + b))
});

server.register_method("get_status", |req| async move {
    JsonRpcResponse::success(req.id, json!({
        "status": "running",
        "uptime": 12345
    }))
});
```

## Protocol Compliance

### JSON-RPC 2.0 Specification
- ✅ Request/response format
- ✅ Error handling
- ✅ Batch requests
- ✅ Notifications
- ✅ Standard error codes

### OVSDB Protocol
- ✅ RFC 7047 compliance
- ✅ Transactional operations
- ✅ Monitor protocol
- ✅ Schema introspection

## Integration Points

### Operation-DBUS Architecture
```
D-Bus Services
     ↓
JSON-RPC Server (op-jsonrpc)
     ↓
├── OVSDB Client → Open vSwitch
└── NonNet DB → Plugin State
```

### Plugin Communication
Plugins communicate with core services via JSON-RPC:
1. Plugin connects to Unix socket
2. Sends JSON-RPC requests
3. Receives responses
4. Handles notifications

### Network Configuration
OVSDB integration enables:
- Bridge creation and management
- Port configuration
- Flow table operations
- Network topology queries

## Performance Considerations

### JSON Parsing
- **simd-json**: SIMD-accelerated parsing for high throughput
- **Zero-copy**: Minimize allocations where possible

### Connection Handling
- **Async I/O**: Non-blocking operations via tokio
- **Connection Pooling**: Reuse connections to OVSDB
- **Backpressure**: Handle slow clients gracefully

### Database Operations
- **Batching**: Group NonNet operations for efficiency
- **Caching**: Cache frequently accessed state
- **Indexing**: Optimize query performance

## Error Handling

### Protocol Errors
- Parse errors (malformed JSON)
- Invalid request structure
- Method not found
- Invalid parameters

### Database Errors
- OVSDB connection failures
- Transaction conflicts
- NonNet storage errors
- Schema validation failures

### Recovery Strategies
- Automatic reconnection to OVSDB
- Transaction retry with backoff
- Graceful degradation on errors
- Detailed error logging

## Testing

### Unit Tests
- Protocol message serialization
- Error response generation
- Method routing logic

### Integration Tests
- End-to-end RPC communication
- OVSDB transaction handling
- NonNet database operations

### Mock Support
- Mock OVSDB server for testing
- In-memory NonNet database
- Simulated network conditions

## Security Considerations

### Unix Socket Permissions
- Restrict socket file permissions
- Validate client credentials
- Rate limiting per connection

### Input Validation
- Validate all RPC parameters
- Sanitize database queries
- Prevent injection attacks

### OVSDB Security
- Secure connection to OVSDB
- Validate transaction operations
- Audit database modifications

## Future Enhancements

- **WebSocket Transport**: Support WebSocket in addition to Unix sockets
- **TLS Support**: Encrypted RPC communication
- **Authentication**: Client authentication and authorization
- **Batch Optimization**: Parallel batch request processing
- **Metrics**: Prometheus metrics for RPC operations
- **Tracing**: Distributed tracing integration
- **Schema Validation**: Automatic parameter validation from schemas
- **Code Generation**: Generate client stubs from method definitions

## Related Crates

- **op-core**: Core types and utilities
- **op-network**: Network configuration using OVSDB
- **op-plugins**: Plugin system using JSON-RPC
- **op-services**: Service management via RPC

---
*JSON-RPC 2.0 server with OVSDB and NonNet database support*
