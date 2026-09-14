# op-services - Specification

## Overview
**Crate**: `op-services`  
**Location**: `crates/op-services`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-services"
version = "0.1.0"
edition = "2021"
description = "System-wide service manager - systemd replacement with dinit backend"
```

### Source Structure
```
op-services/src/bin/systemctl.rs
op-services/src/bin/op-services.rs
op-services/src/bin/systemctl-native.rs
op-services/src/dbus/mod.rs
op-services/src/dbus/interface.rs
op-services/src/grpc/server.rs
op-services/src/grpc/mod.rs
op-services/src/manager/service_manager.rs
op-services/src/manager/process.rs
op-services/src/manager/mod.rs
op-services/src/manager/dinit_proxy.rs
op-services/src/schema/mod.rs
op-services/src/store/mod.rs
op-services/src/lib.rs
```

### Key Dependencies
```toml
# Schema source of truth
op-plugins = { path = "../op-plugins" }

# gRPC
tonic = "0.12"
prost = "0.13"
prost-types = "0.13"

# D-Bus
zbus = { version = "4.0", features = ["tokio"] }

# Database
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite"] }

# Async
tokio = { version = "1", features = ["full", "signal"] }
tokio-stream = "0.1"
futures = "0.3"

# Serialization
```

### Binaries
```toml
[[bin]]
name = "op-services"
path = "src/bin/op-services.rs"

[[bin]]
name = "systemctl"
path = "src/bin/systemctl.rs"

[[bin]]
name = "systemctl-native"
path = "src/bin/systemctl-native.rs"
```

### Features
```toml
# No features
```

## Documentation Files


## Module Structure
      14 Rust source files

### Main Modules


## Purpose
System-wide service manager - systemd replacement with dinit backend

## Build Information
- **Edition**: 2021
- **Version**: 0.1.0
- **License**: 

## Related Crates
Internal dependencies:
- op-plugins

---
*Generated from crate analysis*
