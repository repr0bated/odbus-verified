# op-agents - Specification

## Overview
**Crate**: `op-agents`  
**Location**: `crates/op-agents`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-agents"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
```

### Source Structure
```
op-agents/src/agents/aiml/prompt_engineer.rs
op-agents/src/agents/aiml/mod.rs
op-agents/src/agents/aiml/mlops_engineer.rs
op-agents/src/agents/aiml/ml_engineer.rs
op-agents/src/agents/aiml/data_scientist.rs
op-agents/src/agents/aiml/data_engineer.rs
op-agents/src/agents/aiml/ai_engineer.rs
op-agents/src/agents/analysis/security_auditor.rs
op-agents/src/agents/analysis/performance.rs
op-agents/src/agents/analysis/mod.rs
op-agents/src/agents/analysis/debugger.rs
op-agents/src/agents/analysis/code_reviewer.rs
op-agents/src/agents/architecture/mod.rs
op-agents/src/agents/architecture/graphql_architect.rs
op-agents/src/agents/architecture/frontend_developer.rs
op-agents/src/agents/architecture/backend_architect.rs
op-agents/src/agents/business/sales_automator.rs
op-agents/src/agents/business/payment_integration.rs
op-agents/src/agents/business/mod.rs
op-agents/src/agents/business/legal_advisor.rs
```

### Key Dependencies
```toml
# Internal crates
op-core = { workspace = true }
op-http = { workspace = true }

# Async runtime
tokio = { workspace = true }
async-trait = { workspace = true }
futures = { workspace = true }

# Serialization
serde = { workspace = true }
simd-json = { workspace = true }
serde_yaml = { workspace = true }
toml = { workspace = true }

# Error handling
anyhow = { workspace = true }
thiserror = { workspace = true }

# D-Bus
```

### Binaries
```toml
[[bin]]
name = "dbus-agent"
path = "src/bin/dbus-agent.rs"

[[bin]]
name = "op-agent-manager"
path = "src/bin/dbus-agent-manager.rs"
```

### Features
```toml
# No features
```

## Documentation Files


## Module Structure
     130 Rust source files

### Main Modules
dbus_service
agent_catalog
agent_registry
router

## Purpose
Secure agent registry and D-Bus agent implementations for op-dbus-v2

## Build Information
- **Edition**: edition.workspace = true
- **Version**: version.workspace = true
- **License**: license.workspace = true

## Related Crates
Internal dependencies:


---
*Generated from crate analysis*
