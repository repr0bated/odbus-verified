# op-gateway - Specification

## Overview
**Crate**: `op-gateway`  
**Location**: `crates/op-gateway`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-gateway"
version = "0.1.0"
edition = "2021"
description = "MCP Gateway with WireGuard authentication and smart routing"
```

### Source Structure
```
op-gateway/src/wireguard_auth.rs
op-gateway/src/mcp_gateway.rs
op-gateway/src/lib.rs
op-gateway/src/error.rs
op-gateway/src/encrypted_storage.rs
```

### Key Dependencies
```toml
# Async
tokio = { version = "1", features = ["full"] }

# Serialization
serde = { version = "1", features = ["derive"] }
simd-json = "0.13"

# Crypto
ring = "0.17"
x25519-dalek = "2.0"
chacha20poly1305 = "0.10"
argon2 = { version = "0.5", features = ["std"] }
blake2 = "0.10"
zeroize = { version = "1.6", features = ["zeroize_derive"] }
base64 = "0.22"
hex = "0.4"

# Database
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite"] }
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
SECURITY-MODEL.md

## Module Structure
       5 Rust source files

### Main Modules
wireguard_auth
mcp_gateway
error
encrypted_storage

## Purpose
MCP Gateway with WireGuard authentication and smart routing

## Build Information
- **Edition**: 2021
- **Version**: 0.1.0
- **License**: 

## Related Crates
Internal dependencies:


---
*Generated from crate analysis*
