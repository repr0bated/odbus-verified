# op-workflows - Specification

## Overview
**Crate**: `op-workflows`  
**Location**: `crates/op-workflows`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-workflows"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
```

### Source Structure
```
op-workflows/src/builtin/tool_node.rs
op-workflows/src/builtin/mod.rs
op-workflows/src/builtin/definitions.rs
op-workflows/src/builtin/dbus_node.rs
op-workflows/src/builtin/plugin_node.rs
op-workflows/src/workflows.rs
op-workflows/src/orchestrator.rs
op-workflows/src/node.rs
op-workflows/src/lib.rs
op-workflows/src/flow.rs
op-workflows/src/engine.rs
op-workflows/src/context.rs
```

### Key Dependencies
```toml
op-core = { workspace = true }
op-plugins = { path = "../op-plugins" }
op-tools = { path = "../op-tools" }
tokio = { workspace = true }
serde = { workspace = true }
simd-json = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
async-trait = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
sha2 = { workspace = true }
hex = "0.4"
pocketflow_rs = "0.1"
op-execution-tracker = { path = "../op-execution-tracker" }
log = { workspace = true }
serde_json = { workspace = true }
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
      12 Rust source files

### Main Modules
workflows
orchestrator
node
flow
engine
context

## Purpose
Workflow engine with plugin/service nodes for op-dbus-v2

## Build Information
- **Edition**: edition.workspace = true
- **Version**: version.workspace = true
- **License**: license.workspace = true

## Related Crates
Internal dependencies:
- op-plugins
- op-tools
- op-execution-tracker

---
*Generated from crate analysis*
