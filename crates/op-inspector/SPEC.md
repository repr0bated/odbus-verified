# op-inspector - Specification

## Overview
**Crate**: `op-inspector`  
**Location**: `crates/op-inspector`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-inspector"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
```

### Source Structure
```
op-inspector/src/lib.rs
op-inspector/src/introspective_gadget.rs
op-inspector/src/gcloud.rs
op-inspector/src/datadump.rs
```

### Key Dependencies
```toml
op-core = { workspace = true }
op-introspection = { path = "../op-introspection" }
tokio = { workspace = true }
serde = { workspace = true }
simd-json = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
async-trait = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
regex = { workspace = true }
quick-xml = { workspace = true }
sha2 = { workspace = true }
base64 = { workspace = true }
serde_yaml = { workspace = true }
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
ADAPTER-WORKFLOW.md

## Module Structure
       4 Rust source files

### Main Modules
introspective_gadget
gcloud
datadump

## Purpose
Inspector Gadget - Universal object inspector with AI gap-filling and Proxmox introspection

## Build Information
- **Edition**: edition.workspace = true
- **Version**: version.workspace = true
- **License**: license.workspace = true

## Related Crates
Internal dependencies:
- op-introspection

---
*Generated from crate analysis*
