# op-state - Specification

## Overview
**Crate**: `op-state`  
**Location**: `crates/op-state`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-state"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
```

### Source Structure
```
op-state/src/schema_validator.rs
op-state/src/plugtree.rs
op-state/src/mod.rs
op-state/src/authority.rs
op-state/src/crypto.rs
op-state/src/dbus_plugin_base.rs
op-state/src/lib.rs
op-state/src/manager.rs
op-state/src/plugin.rs
op-state/src/plugin_workflow.rs
op-state/src/dbus_server.rs
```

### Key Dependencies
```toml
op-core = { path = "../op-core" }
op-snowball = { path = "../op-snowball" }
op-jsonrpc = { path = "../op-jsonrpc" }
op-state-store = { path = "../op-state-store" }
tokio = { workspace = true }
serde = { workspace = true }
simd-json = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
async-trait = { workspace = true }
zbus = { workspace = true }
chrono = { workspace = true }
sha2 = { workspace = true }
quick-xml = { workspace = true }
rand = { workspace = true }
base64 = { workspace = true }
log = { workspace = true }
aes-gcm = { workspace = true }
argon2 = { workspace = true }
```

### Binaries
```toml
# No binaries
```

### Features
```toml
[features]
default = []
mcp = []
```

## Documentation Files


## Module Structure
      11 Rust source files

### Main Modules
schema_validator
plugtree
mod
authority
crypto
dbus_plugin_base
manager
plugin
plugin_workflow
dbus_server

## Purpose
State management system with plugin infrastructure, crypto, and schema validation

## Build Information
- **Edition**: edition.workspace = true
- **Version**: version.workspace = true
- **License**: license.workspace = true

## Related Crates
Internal dependencies:
- op-core
- op-snowball
- op-jsonrpc
- op-state-store

---
*Generated from crate analysis*
