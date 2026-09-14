# op-cognitive-mcp - Specification

## Overview
**Crate**: `op-cognitive-mcp`  
**Location**: `crates/op-cognitive-mcp`

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-cognitive-mcp"
version = "0.1.0"
edition = "2021"

[dependencies]
```

### Source Structure
```
op-cognitive-mcp/src/cognitive_tools.rs
op-cognitive-mcp/src/main.rs
op-cognitive-mcp/src/lib.rs
op-cognitive-mcp/src/memory_store.rs
op-cognitive-mcp/src/server.rs
```

### Key Dependencies
```toml
op-core = { path = "../op-core" }
op-mcp = { path = "../op-mcp" }
op-dynamic-loader = { path = "../op-dynamic-loader" }
op-cache = { path = "../op-cache" }
serde = { version = "1.0", features = ["derive"] }
simd-json = { workspace = true }
tokio = { version = "1.0", features = ["full"] }
anyhow = "1.0"
tracing = "0.1"
axum = { version = "0.7", features = ["json", "http2"] }
tower = "0.4"
tower-http = { version = "0.5", features = ["cors"] }
uuid = { version = "1.0", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
async-trait = "0.1"
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
cognitive_tools
memory_store
server

## Purpose

`op-cognitive-mcp` exposes code-aware retrieval over the Qdrant/Voyage indexes
used by local coding clients. Retrieval is intentionally split by embedding
space:

- Rust solo: `rust_lang_rust_lsp_voyage_code_3`, queried with `voyage-code-3`
  when `COGNITIVE_MCP_VOYAGE_MODEL=voyage-code-3`.
- Other LSP/spec groups: `repos_lsp_*_voyage_4_lite` and
  `repos_specs_docs_voyage_4_lite`, queried with the Voyage 4 family. These
  default to `voyage-4` at query time and are compatible with
  `voyage-4-lite` indexes.

For context-aware completion, the MCP path should ask Kiro/LSP for exact local
symbol and hover context first, then call `code_context` with
`retrieval_mode="completion"`. Completion retrieval embeds the query once,
queries the compatible configured collections, returns top-k vector hits, and
does not rerank unless `COGNITIVE_MCP_RERANK_MODE=always`.

Deep chat/edit context can use `retrieval_mode="deep"` with a larger fetch
window. Rerank is only enabled for deep mode when
`COGNITIVE_MCP_RERANK_MODE=auto` or `always`.

### Vector Environment

```sh
COGNITIVE_MCP_VOYAGE_API_KEY=<key>
# Falls back to VOYAGE_API_KEY, then VOYAGE_API_KEY_RUST, then key file.

COGNITIVE_MCP_COMPLETION_COLLECTIONS=repos_lsp_c_cpp_voyage_4_lite,repos_lsp_go_voyage_4_lite,repos_lsp_java_voyage_4_lite,repos_lsp_python_voyage_4_lite,repos_lsp_typescript_voyage_4_lite,repos_specs_docs_voyage_4_lite
COGNITIVE_MCP_RUST_COLLECTION=rust_lang_rust_lsp_voyage_code_3
COGNITIVE_MCP_COMPLETION_TOP_K=12
COGNITIVE_MCP_DEEP_TOP_K=50
COGNITIVE_MCP_RERANK_MODE=auto
COGNITIVE_MCP_KIRO_LSP_STATE_DIR=/home/jeremy/git/logs/kiro-lsp-state
```


## Build Information
- **Edition**: 2021
- **Version**: 0.1.0
- **License**: 

## Related Crates
Internal dependencies:
- op-core
- op-mcp
- op-dynamic-loader
- op-cache

---
*Generated from crate analysis*
