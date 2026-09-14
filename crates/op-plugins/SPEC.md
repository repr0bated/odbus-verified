# op-plugins - Specification

## Overview
**Crate**: `op-plugins`  
**Location**: `crates/op-plugins`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-plugins"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
```

### Source Structure
```
op-plugins/src/state_plugins/systemd_networkd.rs
op-plugins/src/state_plugins/mod.rs
op-plugins/src/state_plugins/adc.rs
op-plugins/src/state_plugins/agent_config.rs
op-plugins/src/state_plugins/config.rs
op-plugins/src/state_plugins/dinit.rs
op-plugins/src/state_plugins/dnsresolver.rs
op-plugins/src/state_plugins/endpoint.rs
op-plugins/src/state_plugins/full_system.rs
op-plugins/src/state_plugins/gcloud_adc.rs
op-plugins/src/state_plugins/hardware.rs
op-plugins/src/state_plugins/keypair.rs
op-plugins/src/state_plugins/keyring.rs
op-plugins/src/state_plugins/login1.rs
op-plugins/src/state_plugins/lxc.rs
op-plugins/src/state_plugins/mcp.rs
op-plugins/src/state_plugins/net.rs
op-plugins/src/state_plugins/netmaker.rs
op-plugins/src/state_plugins/openflow.rs
op-plugins/src/state_plugins/openflow_obfuscation.rs
```

### Key Dependencies
```toml
op-core = { path = "../op-core" }
op-state = { path = "../op-state" }
op-state-store = { path = "../op-state-store" }
op-snowball = { path = "../op-snowball" }
op-network = { path = "../op-network" }
op-dynamic-loader = { path = "../op-dynamic-loader" }
op-execution-tracker = { path = "../op-execution-tracker" }

tokio = { workspace = true }
serde = { workspace = true }
simd-json = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
async-trait = { workspace = true }
zbus = { workspace = true }
chrono = { workspace = true }
log = { workspace = true }
reqwest = { workspace = true }
sha2 = { workspace = true }
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
      45 Rust source files

### Main Modules
registry
auto_create
builtin
chat
dynamic_loading
plugin
state
systemd
default_registry

## Purpose
Plugin system with state management, domain plugins, and snowball footprints

## Build Information
- **Edition**: edition.workspace = true
- **Version**: version.workspace = true
- **License**: license.workspace = true

## Related Crates
Internal dependencies:
- op-core
- op-state
- op-state-store
- op-snowball
- op-network
- op-dynamic-loader
- op-execution-tracker

---
*Generated from crate analysis*
