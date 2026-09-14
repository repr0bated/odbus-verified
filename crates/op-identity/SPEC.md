# op-identity - Specification

## Overview
**Crate**: `op-identity`  
**Location**: `crates/op-identity`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-identity"
version = "0.1.0"
edition = "2021"

[dependencies]
```

### Source Structure
```
op-identity/src/session.rs
op-identity/src/lib.rs
op-identity/src/wg.rs
op-identity/src/token.rs
op-identity/src/wireguard.rs
op-identity/src/registration.rs
op-identity/src/gcloud_auth.rs
```

### Key Dependencies
```toml
anyhow = "1"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
simd-json = { workspace = true }
zbus = { version = "5.12", features = ["tokio"] }
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1.6", features = ["v4", "serde"] }
tracing = "0.1"
keyring = "2"
rusqlite = { workspace = true }
dirs = "5"
hostname = "0.4"
rand = { workspace = true }
base64 = { workspace = true }
x25519-dalek = { version = "2", features = ["static_secrets"] }
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
       7 Rust source files

### Main Modules
session
wg
token
wireguard
registration
gcloud_auth

## Purpose


## Build Information
- **Edition**: 2021
- **Version**: 0.1.0
- **License**: 

## Related Crates
Internal dependencies:


---
*Generated from crate analysis*
