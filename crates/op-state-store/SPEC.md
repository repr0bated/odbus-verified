# op-state-store - Specification

## Overview
**Crate**: `op-state-store`  
**Location**: `crates/op-state-store`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-state-store"
version = "0.1.0"
edition.workspace = true
license.workspace = true
description = "MCP Execution State Store - Persistent job ledger and state tracking"
```

### Source Structure
```
op-state-store/src/state_store.rs
op-state-store/src/sqlite_store.rs
op-state-store/src/schema_validator.rs
op-state-store/src/redis_stream.rs
op-state-store/src/plugin_schema.rs
op-state-store/src/metrics.rs
op-state-store/src/lib.rs
op-state-store/src/execution_job.rs
op-state-store/src/event_chain.rs
op-state-store/src/error.rs
op-state-store/src/disaster_recovery.rs
```

### Key Dependencies
```toml
tokio = { workspace = true, features = ["full"] }
sqlx = { workspace = true, features = ["sqlite", "runtime-tokio", "chrono", "json"] }
redis = { workspace = true, features = ["tokio-comp"] }
serde = { workspace = true }
simd-json = { workspace = true }
chrono = { workspace = true }
uuid = { workspace = true }
tracing = { workspace = true }
md5 = "0.7"
opentelemetry = { workspace = true }
prometheus = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
async-trait = { workspace = true }
regex = { workspace = true }
lazy_static = { workspace = true }
zbus = { workspace = true }
serde_json = { workspace = true }
jsonschema = { version = "0.29", default-features = false }
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
      11 Rust source files

### Main Modules
state_store
sqlite_store
schema_validator
redis_stream
plugin_schema
metrics
execution_job
event_chain
error
disaster_recovery

## Purpose
MCP Execution State Store - Persistent job ledger and state tracking

## Build Information
- **Edition**: edition.workspace = true
- **Version**: 0.1.0
- **License**: license.workspace = true

## Related Crates
Internal dependencies:


---
*Generated from crate analysis*
