# op-mcp - Specification

## Overview
**Crate**: `op-mcp`  
**Location**: `crates/op-mcp`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-mcp"
version = "0.4.0"
edition = "2021"
description = "Unified MCP Protocol Server with multiple transport and mode support"
```

### Source Structure
```
op-mcp/src/grpc/generated/op.mcp.v1.rs
op-mcp/src/grpc/service.rs
op-mcp/src/grpc/server.rs
op-mcp/src/grpc/mod.rs
op-mcp/src/grpc/client.rs
op-mcp/src/tools/systemd.rs
op-mcp/src/tools/system.rs
op-mcp/src/tools/shell.rs
op-mcp/src/tools/response.rs
op-mcp/src/tools/ovs.rs
op-mcp/src/tools/mod.rs
op-mcp/src/tools/filesystem.rs
op-mcp/src/tools/plugin.rs
op-mcp/src/transport/websocket.rs
op-mcp/src/transport/stdio.rs
op-mcp/src/transport/http.rs
op-mcp/src/transport/mod.rs
op-mcp/src/trait_agent_executor.rs
op-mcp/src/tool_registry.rs
op-mcp/src/tool_adapter_orchestrated.rs
```

### Key Dependencies
```toml
# Core
anyhow = "1.0"
async-trait = "0.1"
chrono = "0.4"
serde = { version = "1.0", features = ["derive"] }
simd-json = { workspace = true }
thiserror = "1.0"
tracing = "0.1"
tracing-subscriber = "0.3"
tokio = { version = "1.0", features = ["full"] }
tokio-stream = { version = "0.1", features = ["sync"] }
futures = "0.3"
uuid = { version = "1.0", features = ["v4"] }

# HTTP/WebSocket
axum = { version = "0.7", features = ["ws"] }
tower-http = { version = "0.5", features = ["cors"] }
reqwest.workspace = true

# D-Bus (for agent executor)
```

### Binaries
```toml
[[bin]]
name = "op-mcp-server"
path = "src/main.rs"

[[bin]]
name = "op-mcp-compact"
path = "src/compact_main.rs"

[[bin]]
name = "op-mcp-agents"
path = "src/agents_main.rs"
```

### Features
```toml
[features]
default = ["grpc"]
grpc = ["tonic", "prost", "tonic-build"]
op-chat = ["dep:op-chat"]

[dependencies]
# Core
anyhow = "1.0"
async-trait = "0.1"
chrono = "0.4"
serde = { version = "1.0", features = ["derive"] }
```

## Documentation Files
README.md

## Module Structure
      39 Rust source files

### Main Modules
trait_agent_executor
tool_registry
tool_adapter_orchestrated
tool_adapter
sse
server
router
resources
request_handler
request_context

## Purpose
Unified MCP Protocol Server with multiple transport and mode support

## Build Information
- **Edition**: 2021
- **Version**: 0.4.0
- **License**: 

## Related Crates
Internal dependencies:
- op-core
- op-tools
- op-plugins
- op-introspection
- op-state
- op-state-store
- op-chat

---
*Generated from crate analysis*
