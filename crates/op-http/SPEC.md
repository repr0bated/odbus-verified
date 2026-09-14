# op-http - Specification

## Overview
**Crate**: `op-http`  
**Location**: `crates/op-http`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-http"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
```

### Source Structure
```
op-http/src/tls.rs
op-http/src/server.rs
op-http/src/router.rs
op-http/src/request_filters.rs
op-http/src/middleware.rs
op-http/src/metrics.rs
op-http/src/lib.rs
op-http/src/health.rs
```

### Key Dependencies
```toml
# Async runtime
tokio = { workspace = true }
futures = { workspace = true }

# Serialization
serde = { workspace = true }
simd-json = { workspace = true }

# Error handling
anyhow = { workspace = true }
thiserror = { workspace = true }

# Logging
tracing = { workspace = true }

# HTTP server
axum = { workspace = true }
tower = { workspace = true }
tower-http = { workspace = true, features = ["cors", "fs", "trace", "compression-gzip", "compression-br", "timeout"] }
hyper = { workspace = true }
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
       8 Rust source files

### Main Modules
tls
server
router
request_filters
middleware
metrics
health

## Purpose
Central HTTP/TLS server for all op-dbus modules

## Build Information
- **Edition**: edition.workspace = true
- **Version**: version.workspace = true
- **License**: license.workspace = true

## Related Crates
Internal dependencies:


---
*Generated from crate analysis*
