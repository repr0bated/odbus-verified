# op-cognitive - Specification

## Overview
**Crate**: `op-cognitive`  
**Location**: `crates/op-cognitive`

The bridge hosts `CognitiveServer` in process; this crate does not provide a
standalone cognitive listener or service. `op-mcp` remains the protocol library.

### Source naming cutover (2026-10-18)

- Rust imports use `op_cognitive`; schema consumers use
  `op_plugins::cognitive_plugin_schema()` and `state_plugins::cognitive`.
- The plugin ID is `cognitive`, its D-Bus object is
  `/org/opdbus/v1/plugins/cognitive`, and its typed registry interface is
  `org.opdbus.v1.plugins.Cognitive`. Generic calls still use `PluginV1.Call`.
- Capabilities use `cognitive.*`, typed MCP tool names use `plugin.cognitive.*`,
  blob resources use `blob://cognitive`, and subsystem OSCAL tags use `cognitive`.
  These are exact contracts, not aliases; caller identity and grant enforcement
  are unchanged. Generated gRPC services derive the new names from this schema.
- Configuration uses `COGNITIVE_*`. Database paths and collection defaults are
  unchanged; renaming a configuration key does not migrate stored data.
- Router user-container options serialize `cognitive_endpoint`. The endpoint's
  transport behavior is unchanged.

This source cutover is not part of the already-built deployment. A release
using it must compile the affected consumers, seal and persist a matching
catalog, and integrate the matching environment, capability grants, MCP toolset
manifest and mounted workspace links together. Regenerate catalog-pinned
3tchedFS specialization artifacts from that catalog; never hand-edit their
schema hashes. Historical receipts and retired standalone-service denylists
retain their original names. No compatibility shim or second listener is added.

## Quick Reference

### From Cargo.toml
```toml
[package]
name = "op-cognitive"
version = "0.1.0"
edition = "2021"

[dependencies]
```

### Source Structure
```
op-cognitive/src/cognitive_tools.rs
op-cognitive/src/lib.rs
op-cognitive/src/memory_store.rs
op-cognitive/src/server.rs
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

`op-cognitive` exposes code-aware retrieval over the Qdrant/Voyage indexes
used by local coding clients. Retrieval is intentionally split by embedding
space:

- Rust solo: `rust_lang_rust_lsp_voyage_code_3`, queried with `voyage-code-3`
  when `COGNITIVE_VOYAGE_MODEL=voyage-code-3`.
- Other LSP/spec groups: `repos_lsp_*_voyage_4_lite` and
  `repos_specs_docs_voyage_4_lite`, queried with the Voyage 4 family. These
  default to `voyage-4` at query time and are compatible with
  `voyage-4-lite` indexes.

For context-aware completion, the MCP path should ask Kiro/LSP for exact local
symbol and hover context first, then call `code_context` with
`retrieval_mode="completion"`. Completion retrieval embeds the query once,
queries the compatible configured collections, returns top-k vector hits, and
does not rerank unless `COGNITIVE_RERANK_MODE=always`.

Deep chat/edit context can use `retrieval_mode="deep"` with a larger fetch
window. Rerank is only enabled for deep mode when
`COGNITIVE_RERANK_MODE=auto` or `always`.

### Vector Environment

```sh
COGNITIVE_VOYAGE_API_KEY=<key>
# Falls back to VOYAGE_API_KEY, then VOYAGE_API_KEY_RUST, then key file.

COGNITIVE_COMPLETION_COLLECTIONS=repos_lsp_c_cpp_voyage_4_lite,repos_lsp_go_voyage_4_lite,repos_lsp_java_voyage_4_lite,repos_lsp_python_voyage_4_lite,repos_lsp_typescript_voyage_4_lite,repos_specs_docs_voyage_4_lite
COGNITIVE_RUST_COLLECTION=rust_lang_rust_lsp_voyage_code_3
COGNITIVE_COMPLETION_TOP_K=12
COGNITIVE_DEEP_TOP_K=50
COGNITIVE_RERANK_MODE=auto
COGNITIVE_KIRO_LSP_STATE_DIR=/home/jeremy/git/logs/kiro-lsp-state
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
