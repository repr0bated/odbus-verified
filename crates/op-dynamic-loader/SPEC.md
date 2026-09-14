# op-dynamic-loader - Specification

## Overview
**Crate**: `op-dynamic-loader`  
**Location**: `crates/op-dynamic-loader`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-dynamic-loader"
version = "0.1.0"
edition.workspace = true
license.workspace = true
description = "Dynamic Tool Loading Enhancement - Complements existing MCP tool loading with intelligent caching and execution tracking"
```

### Source Structure
```
op-dynamic-loader/src/loading_strategy.rs
op-dynamic-loader/src/lib.rs
op-dynamic-loader/src/execution_aware_loader.rs
op-dynamic-loader/src/error.rs
op-dynamic-loader/src/dynamic_registry.rs
```

### Key Dependencies
```toml
tokio = { workspace = true, features = ["full"] }
serde = { workspace = true, features = ["derive"] }
simd-json = { workspace = true }
chrono = { workspace = true, features = ["serde"] }
uuid = { workspace = true, features = ["v4", "serde"] }
thiserror = { workspace = true }
tracing = { workspace = true }
async-trait = { workspace = true }
lru = { workspace = true }
anyhow = { workspace = true }

# Internal dependencies
op-core = { path = "../op-core" }
op-tools = { path = "../op-tools" }
op-execution-tracker = { path = "../op-execution-tracker" }
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
       5 Rust source files

### Main Modules
loading_strategy
execution_aware_loader
error
dynamic_registry

## Purpose
Dynamic Tool Loading Enhancement - Complements existing MCP tool loading with intelligent caching and execution tracking

## Build Information
- **Edition**: edition.workspace = true
- **Version**: 0.1.0
- **License**: license.workspace = true

## Related Crates
Internal dependencies:
- op-core
- op-tools
- op-execution-tracker

---
*Generated from crate analysis*
