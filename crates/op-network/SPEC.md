# op-network - Specification

## Overview
**Crate**: `op-network`  
**Location**: `crates/op-network`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-network"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
```

### Source Structure
```
op-network/src/rtnetlink.rs
op-network/src/proxmox.rs
op-network/src/plugin.rs
op-network/src/ovsdb.rs
op-network/src/ovs_netlink.rs
op-network/src/ovs_error.rs
op-network/src/ovs_capabilities.rs
op-network/src/openflow.rs
op-network/src/lib.rs
```

### Key Dependencies
```toml
tokio = { workspace = true }
serde = { workspace = true }
simd-json = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
async-trait = { workspace = true }
futures = "0.3"
rtnetlink = { workspace = true }
log = { workspace = true }

# HTTP client for Proxmox API
reqwest = { workspace = true }

# Netlink (Generic Netlink for OVS)
netlink-sys = "0.8"
netlink-packet-core = "0.7"
netlink-packet-generic = "0.3"
netlink-packet-utils = "0.5"
netlink-packet-route = "0.19"
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
rtnetlink
proxmox
plugin
ovsdb
ovs_netlink
ovs_error
ovs_capabilities
openflow

## Purpose
Native networking: OpenFlow (all versions, pure Rust), OVSDB JSON-RPC, rtnetlink, Proxmox API, container networking

## Build Information
- **Edition**: edition.workspace = true
- **Version**: version.workspace = true
- **License**: license.workspace = true

## Related Crates
Internal dependencies:
- op-core

---
*Generated from crate analysis*
