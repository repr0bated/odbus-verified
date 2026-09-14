# op-llm - Specification

## Overview
**Crate**: `op-llm`  
**Location**: `crates/op-llm`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-llm"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
```

### Source Structure
```
op-llm/src/perplexity.rs
op-llm/src/huggingface.rs
op-llm/src/headless_oauth.rs
op-llm/src/gemini.rs
op-llm/src/chat.rs
op-llm/src/antigravity_replay.rs
op-llm/src/antigravity.rs
op-llm/src/anthropic.rs
op-llm/src/gcloud_adc.rs
op-llm/src/provider.rs
op-llm/src/lib.rs
op-llm/src/pty_bridge.rs
op-llm/src/gemini_cli.rs
op-llm/src/mcp_proxy.rs
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
reqwest = { workspace = true }
chrono = { workspace = true }
rsa = "0.9.9"
sha2.workspace = true
base64.workspace = true
jsonwebtoken = "9"
uuid = { version = "1.0", features = ["v4"] }
dirs = "5.0"
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
      14 Rust source files

### Main Modules
perplexity
huggingface
headless_oauth
gemini
chat
antigravity_replay
antigravity
anthropic
gcloud_adc
provider

## Purpose
LLM provider integration with dynamic model discovery for HuggingFace

## Build Information
- **Edition**: edition.workspace = true
- **Version**: version.workspace = true
- **License**: license.workspace = true

## Related Crates
Internal dependencies:


---
*Generated from crate analysis*
