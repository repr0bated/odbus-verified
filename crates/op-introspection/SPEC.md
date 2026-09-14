# op-introspection - Specification

## Overview
**Crate**: `op-introspection`  
**Location**: `crates/op-introspection`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-introspection"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
```

### Source Structure
```
op-introspection/src/scanner.rs
op-introspection/src/projection.rs
op-introspection/src/parser.rs
op-introspection/src/mod.rs
op-introspection/src/lib.rs
op-introspection/src/indexer_manager.rs
op-introspection/src/indexer.rs
op-introspection/src/hierarchical.rs
op-introspection/src/cpu_features.rs
op-introspection/src/cache.rs
```

### Key Dependencies
```toml
op-core = { workspace = true }
op-snowball = { path = "../op-snowball" }
tokio = { workspace = true }
serde = { workspace = true }
simd-json = { workspace = true }
zbus = { workspace = true }
zbus_xml = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
futures = { workspace = true }
async-trait = { workspace = true }
quick-xml = { workspace = true }
rusqlite = { workspace = true, features = ["bundled"] }
chrono = { workspace = true }
parking_lot = "0.12"
sha2 = { workspace = true }
hex = "0.4"
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
      10 Rust source files

### Main Modules
scanner
projection
parser
mod
indexer_manager
indexer
hierarchical
cpu_features
cache

## Purpose
DBus introspection capabilities for op-dbus-v2

## Build Information
- **Edition**: edition.workspace = true
- **Version**: version.workspace = true
- **License**: license.workspace = true

## Related Crates
Internal dependencies:
- op-snowball

---
*Generated from crate analysis*
