# op-tools - Specification

## Overview
**Crate**: `op-tools`  
**Location**: `crates/op-tools`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-tools"
version = "0.1.0"
edition.workspace = true
license.workspace = true
description = "Tool registry and execution for op-dbus-v2"
```

### Source Structure
```
op-tools/src/bin/op-packagekit-install.rs
op-tools/src/builtin/dinit.rs
op-tools/src/builtin/system.rs
op-tools/src/builtin/shell_tool.rs
op-tools/src/builtin/shell.rs
op-tools/src/builtin/self_tools.rs
op-tools/src/builtin/rtnetlink_tools.rs
op-tools/src/builtin/response_tools.rs
op-tools/src/builtin/respond_tool.rs
op-tools/src/builtin/procfs.rs
op-tools/src/builtin/packagekit.rs
op-tools/src/builtin/ovsdb.rs
op-tools/src/builtin/ovs_tools.rs
op-tools/src/builtin/ovs.rs
op-tools/src/builtin/openflow_tools.rs
op-tools/src/builtin/mod.rs
op-tools/src/builtin/lxc_tools.rs
op-tools/src/builtin/gcloud_tools.rs
op-tools/src/builtin/file.rs
op-tools/src/builtin/error_reporting_tool.rs
```

### Key Dependencies
```toml
# Async runtime
tokio = { workspace = true, features = ["full", "sync"] }
async-trait = { workspace = true }

# Serialization
serde = { workspace = true }
simd-json = { workspace = true }
serde_json = { workspace = true }

# Error handling
anyhow = { workspace = true }
thiserror = { workspace = true }

# Logging
tracing = { workspace = true }
clap = { workspace = true }
futures = { workspace = true }

# Time
chrono = { workspace = true }
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
      47 Rust source files

### Main Modules
validation_tests
validation
tool
security
router
registry
orchestration_plugin
mcptools
executor
dynamic_tool

## Purpose
Tool registry and execution for op-dbus-v2

## Build Information
- **Edition**: edition.workspace = true
- **Version**: 0.1.0
- **License**: license.workspace = true

## Related Crates
Internal dependencies:
- op-core
- op-introspection
- op-inspector
- op-network
- op-http
- op-execution-tracker

---
*Generated from crate analysis*
