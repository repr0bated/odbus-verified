# op-grpc-bridge - Specification

## Overview
**Crate**: `op-grpc-bridge`  
**Location**: `crates/op-grpc-bridge`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-grpc-bridge"
version = "0.1.0"
edition.workspace = true
license.workspace = true
description = "Bidirectional D-Bus <-> gRPC bridge with event chain integration"
```

### Source Structure
```
op-grpc-bridge/src/sync_engine.rs
op-grpc-bridge/src/proto_gen.rs
op-grpc-bridge/src/lib.rs
op-grpc-bridge/src/grpc_server.rs
op-grpc-bridge/src/grpc_client.rs
op-grpc-bridge/src/dbus_watcher.rs
```

### Key Dependencies
```toml
# gRPC
tonic = { workspace = true }
prost = { workspace = true }
prost-types = { workspace = true }
tonic-reflection = { workspace = true }

# Async runtime
tokio = { workspace = true, features = ["full", "sync"] }
tokio-stream = { version = "0.1", features = ["sync"] }

# D-Bus
zbus = { workspace = true }

# Serialization
serde = { workspace = true }
serde_json = { workspace = true }
simd-json = { workspace = true }

# Internal crates
op-state-store = { path = "../op-state-store" }
```

### Binaries
```toml
# No binaries
```

### Features
```toml
# No features
```

## Documentation Files


## Module Structure
       6 Rust source files

### Main Modules
sync_engine
proto_gen
grpc_server
grpc_client
dbus_watcher

## Purpose
Bidirectional D-Bus <-> gRPC bridge with event chain integration

## Build Information
- **Edition**: edition.workspace = true
- **Version**: 0.1.0
- **License**: license.workspace = true

## Related Crates
Internal dependencies:
- op-state-store

---
*Generated from crate analysis*
