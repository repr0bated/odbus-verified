# op-mcp-aggregator - Specification

## Overview
**Crate**: `op-mcp-aggregator`  
**Location**: `crates/op-mcp-aggregator`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-mcp-aggregator"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
```

### Source Structure
```
op-mcp-aggregator/src/unused/context.rs
op-mcp-aggregator/src/config.rs
op-mcp-aggregator/src/compact.rs
op-mcp-aggregator/src/client.rs
op-mcp-aggregator/src/cache.rs
op-mcp-aggregator/src/aggregator.rs
op-mcp-aggregator/src/groups.rs
op-mcp-aggregator/src/lib.rs
op-mcp-aggregator/src/profile.rs
```

### Key Dependencies
```toml
# Workspace crates
op-core = { workspace = true }
op-tools = { workspace = true }
op-plugins = { workspace = true }

# Async runtime
tokio = { workspace = true, features = ["full", "sync"] }
futures = { workspace = true }
async-trait = { workspace = true }

# Serialization
serde = { workspace = true, features = ["derive"] }
simd-json = { workspace = true }
serde_yaml = { workspace = true }

# HTTP client for upstream MCP servers
reqwest = { workspace = true, features = ["json"] }

# Error handling
anyhow = { workspace = true }
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
README.md
CLEANUP-CONTEXT-AWARE.md

## Module Structure
       9 Rust source files

### Main Modules
config
compact
client
cache
aggregator
groups
profile

## Purpose
MCP Server Aggregator - proxies and aggregates multiple MCP servers behind a single endpoint

## Build Information
- **Edition**: edition.workspace = true
- **Version**: version.workspace = true
- **License**: license.workspace = true

## Related Crates
Internal dependencies:


---
*Generated from crate analysis*
