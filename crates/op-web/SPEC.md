# op-web - Specification

## Overview
**Crate**: `op-web`  
**Location**: `crates/op-web`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-web"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
```

### Source Structure
```
op-web/src/handlers/llm.rs
op-web/src/handlers/health.rs
op-web/src/handlers/chat.rs
op-web/src/handlers/auth_bridge.rs
op-web/src/handlers/agents.rs
op-web/src/handlers/tools.rs
op-web/src/handlers/status.rs
op-web/src/handlers/mod.rs
op-web/src/handlers/websocket.rs
op-web/src/handlers/privacy.rs
op-web/src/middleware/security.rs
op-web/src/middleware/mod.rs
op-web/src/orchestrator/types.rs
op-web/src/orchestrator/parsing.rs
op-web/src/orchestrator/mod.rs
op-web/src/orchestrator/formatting.rs
op-web/src/orchestrator/execution.rs
op-web/src/orchestrator/anti_hallucination.rs
op-web/src/orchestrator/tools.rs
op-web/src/orchestrator/process.rs
```

### Key Dependencies
```toml
# Workspace crates
op-core = { workspace = true }
op-chat = { workspace = true }
op-llm = { path = "../op-llm" }
op-tools = { path = "../op-tools" }
op-agents = { path = "../op-agents" }
op-state = { workspace = true }
op-network = { workspace = true }
op-mcp = { path = "../op-mcp" }
op-mcp-aggregator = { path = "../op-mcp-aggregator" }
op-state-store = { path = "../op-state-store" }
op-identity = { workspace = true }
op-introspection = { workspace = true }
op-grpc-bridge = { path = "../op-grpc-bridge" }

# Rate limiting
tower_governor = "0.4"

# Web framework
axum = { workspace = true, features = ["ws", "macros", "tokio"] }
```

### Binaries
```toml
[[bin]]
name = "op-web-server"
path = "src/main.rs"
```

### Features
```toml
# No features
```

## Documentation Files


## Module Structure
      42 Rust source files

### Main Modules
websocket
users
system_prompt_loader
sse
server
router
mcp_compact
mcp_agents
mcp
embedded_ui

## Purpose
Unified web server for op-dbus-v2 - consolidates all HTTP services

## Deployment Architecture

### UI (`astral` incus container)
The frontend UI is **not** embedded/served directly by `op-web-server` on the host. It is served from within the **`astral`** incus container. The host `op-web` dinit service is currently stopped (`{-}` state), with the UI instance running inside astral.

### Leader Election (`ipenclaw` incus container)
Leader election is handled by **`openclaw`**, which runs inside the **`ipenclaw`** incus container. The `openclaw` service is visible in the host dinit service list and coordinates election across the system.

```
Host (dinit)
├── op-dbus          [running]  ← core D-Bus/gRPC backend
├── op-web           [stopped]  ← not serving on host
├── openclaw         [running]  ← election coordinator (in ipenclaw container)
└── incus            [running]
    ├── astral        ← serves the op-web UI frontend
    └── ipenclaw      ← runs openclaw for leader election
```

## Build Information
- **Edition**: edition.workspace = true
- **Version**: version.workspace = true
- **License**: license.workspace = true

## Related Crates
Internal dependencies:
- op-llm
- op-tools
- op-agents
- op-mcp
- op-mcp-aggregator
- op-state-store
- op-grpc-bridge

---
*Updated 2026-03-02 — deployment topology reflects astral/ipenclaw incus containers*
