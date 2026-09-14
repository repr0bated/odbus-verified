# op-cache - Specification

## Overview
**Crate**: `op-cache`  
**Location**: `crates/op-cache`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-cache"
version = "0.1.0"
edition = "2021"
description = "BTRFS-based caching with NUMA awareness and gRPC services"
license = "MIT"
```

### Source Structure
```
op-cache/src/grpc/server.rs
op-cache/src/grpc/orchestrator_service.rs
op-cache/src/grpc/mod.rs
op-cache/src/grpc/cache_service.rs
op-cache/src/grpc/agent_service.rs
op-cache/src/btrfs_cache.rs
op-cache/src/agent_registry.rs
op-cache/src/agent.rs
op-cache/src/workstack_cache.rs
op-cache/src/workflow_tracker.rs
op-cache/src/workflow_executor.rs
op-cache/src/workflow_cache.rs
op-cache/src/snapshot_manager.rs
op-cache/src/pattern_tracker.rs
op-cache/src/orchestrator.rs
op-cache/src/numa.rs
op-cache/src/lib.rs
op-cache/src/capability_resolver.rs
```

### Key Dependencies
```toml
anyhow = "1.0"
bincode = "1.3"
chrono = { version = "0.4", features = ["serde"] }
futures = { workspace = true }
log = "0.4"
num_cpus = "1.16"
prost = { workspace = true }
rusqlite = { workspace = true, features = ["bundled"] }
serde = { version = "1.0", features = ["derive"] }
simd-json = { workspace = true }
sha2 = "0.10"
tokio = { version = "1.0", features = ["full"] }
tokio-stream = "0.1"
tonic = { workspace = true }
tracing = "0.1"
uuid = { version = "1.0", features = ["v4"] }
zstd = "0.13"

tonic-build = { workspace = true }
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
      18 Rust source files

### Main Modules
btrfs_cache
agent_registry
agent
workstack_cache
workflow_tracker
workflow_executor
workflow_cache
snapshot_manager
pattern_tracker
orchestrator

## Purpose
BTRFS-based caching with NUMA awareness and gRPC services

## Build Information
- **Edition**: 2021
- **Version**: 0.1.0
- **License**: MIT

## Related Crates
Internal dependencies:


---
*Generated from crate analysis*
