# op-core - Specification

## Overview
**Crate**: `op-core`  
**Location**: `crates/op-core`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-core"
version = "0.1.0"
edition.workspace = true
license.workspace = true
description = "Core types and utilities for op-dbus-v2"
```

### Source Structure
```
op-core/src/types.rs
op-core/src/self_identity.rs
op-core/src/security.rs
op-core/src/message.rs
op-core/src/lib.rs
op-core/src/execution.rs
op-core/src/error.rs
op-core/src/connection.rs
op-core/src/config.rs
```

### Key Dependencies
```toml
serde = { workspace = true }
simd-json = { version = "0.13", features = ["serde", "serde_impl"] }
uuid = { workspace = true }
chrono = { workspace = true }
tokio = { workspace = true, features = ["sync", "time"] }
tracing = { workspace = true }
thiserror = { workspace = true }
anyhow = { workspace = true }
zbus = { workspace = true }
op-execution-tracker = { path = "../op-execution-tracker" }

tokio = { workspace = true, features = ["full", "test-util"] }
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
       9 Rust source files

### Main Modules
types
self_identity
security
message
execution
error
connection
config

## Purpose
Core types and utilities for op-dbus-v2

## Build Information
- **Edition**: edition.workspace = true
- **Version**: 0.1.0
- **License**: license.workspace = true

## Related Crates
Internal dependencies:
- op-execution-tracker

---
*Generated from crate analysis*
