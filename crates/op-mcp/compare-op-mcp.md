# compare-op-mcp

**Date**: 2026-04-05  
**Spec files analyzed**: SPEC.md, README.md, docs/ARCHITECTURE.md  
**Analysis mode**: Current system state specification with spec-alignment notes

---

## Summary

| Category | Count |
|---|---|
| Rust source files | 41 |
| Proto files | 2 |
| Binary targets | 3 |
| UI files | 0 |
| Root-declared modules | 8 |
| Partial artifacts | 2 |
| Spec-listed source files | 20 |
| Spec-listed but missing | 0 |
| Extra implementation files | 21 |

## Current Implementation Overview

- Unified MCP Protocol Server with multiple transport and mode support
- Internal crate integrations: op-core, op-tools, op-plugins, op-introspection, op-state, op-state-store.
- Protocol assets: 2 `.proto` files.

## Module / File Comparison

| Module or File | State | Current Role | Notes |
|---|---|---|---|
| `src/grpc/generated/op.mcp.v1.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/grpc/generated/op.mcp.v1.rs |
| `src/grpc/service.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/grpc/service.rs |
| `src/grpc/server.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/grpc/server.rs |
| `src/grpc/mod.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/grpc/mod.rs |
| `src/grpc/client.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/grpc/client.rs |
| `src/tools/systemd.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/tools/systemd.rs |
| `src/tools/system.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/tools/system.rs |
| `src/tools/shell.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/tools/shell.rs |
| `src/tools/response.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/tools/response.rs |
| `src/tools/ovs.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/tools/ovs.rs |
| `src/tools/mod.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/tools/mod.rs |
| `src/tools/filesystem.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/tools/filesystem.rs |
| `src/tools/plugin.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/tools/plugin.rs |
| `src/transport/websocket.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/transport/websocket.rs |
| `src/transport/stdio.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/transport/stdio.rs |
| `src/transport/http.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/transport/http.rs |
| `src/transport/mod.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/transport/mod.rs |
| `src/trait_agent_executor.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/trait_agent_executor.rs |
| `src/tool_registry.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/tool_registry.rs |
| `src/tool_adapter_orchestrated.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/tool_adapter_orchestrated.rs |
| `build.rs` | ✅ Present | build script | build.rs |
| `grpc` | ✅ Present | grpc group | src/grpc/client.rs, src/grpc/generated/op.mcp.v1.rs, src/grpc/mod.rs, src/grpc/server.rs, src/grpc/service.rs |
| `root` | ✅ Present | root source group | src/agents_main.rs, src/agents_server.rs, src/builtin_trait_agents.rs, src/compact.rs, src/compact_main.rs, src/config.rs, src/external_client.rs, src/http_server.rs, ... (+14 more) |
| `tools` | ✅ Present | tools group | src/tools/filesystem.rs, src/tools/mod.rs, src/tools/ovs.rs, src/tools/plugin.rs, src/tools/qdrant.rs, src/tools/response.rs, src/tools/shell.rs, src/tools/system.rs, ... (+1 more) |
| `transport` | ✅ Present | transport group | src/transport/http.rs, src/transport/mod.rs, src/transport/stdio.rs, src/transport/websocket.rs |

## Feature / Capability Comparison

| Capability | State | Evidence | Source |
|---|---|---|---|
| trait_agent_executor | ✅ Implemented | src/trait_agent_executor.rs | SPEC main module |
| tool_registry | ✅ Implemented | src/tool_registry.rs | SPEC main module |
| tool_adapter_orchestrated | ✅ Implemented | src/tool_adapter_orchestrated.rs | SPEC main module |
| tool_adapter | ✅ Implemented | src/tool_adapter.rs | SPEC main module |
| sse | ✅ Implemented | src/sse.rs | SPEC main module |
| server | ✅ Implemented | src/grpc/server.rs, src/server.rs | SPEC main module |
| router | ✅ Implemented | src/router.rs | SPEC main module |
| resources | ✅ Implemented | src/resources.rs | SPEC main module |
| request_handler | ✅ Implemented | src/request_handler.rs | SPEC main module |
| request_context | ✅ Implemented | src/request_context.rs | SPEC main module |
| Protocol `internal_agents.proto` | ✅ Implemented | proto/internal_agents.proto | proto |
| Protocol `mcp.proto` | ✅ Implemented | proto/mcp.proto | proto |
| Binary `op-mcp-server` | ✅ Implemented | src/main.rs | Cargo bin target |
| Binary `op-mcp-compact` | ✅ Implemented | src/compact_main.rs | Cargo bin target |
| Binary `op-mcp-agents` | ✅ Implemented | src/agents_main.rs | Cargo bin target |

## Dependencies Comparison

### Internal Workspace Dependencies
- `op-core` - not listed in SPEC dependency block
- `op-tools` - not listed in SPEC dependency block
- `op-plugins` - not listed in SPEC dependency block
- `op-introspection` - not listed in SPEC dependency block
- `op-state` - not listed in SPEC dependency block
- `op-state-store` - not listed in SPEC dependency block

### External Runtime Dependencies
- `anyhow` - documented in SPEC
- `async-trait` - documented in SPEC
- `chrono` - documented in SPEC
- `serde` - documented in SPEC
- `simd-json` - documented in SPEC
- `thiserror` - documented in SPEC
- `tracing` - documented in SPEC
- `tracing-subscriber` - documented in SPEC
- `tokio` - documented in SPEC
- `tokio-stream` - documented in SPEC
- `futures` - documented in SPEC
- `uuid` - documented in SPEC
- `prost-types` - not listed in SPEC dependency block
- `axum` - documented in SPEC
- `tower-http` - documented in SPEC
- `reqwest.workspace` - documented in SPEC
- `zbus` - not listed in SPEC dependency block
- `clap` - not listed in SPEC dependency block
- `tonic` - not listed in SPEC dependency block
- `prost` - not listed in SPEC dependency block

### Development and Build Dependencies
- `build:tonic-build`

## Notes and Observations

- Local documentation files present: README.md, SPEC.md, docs/ARCHITECTURE.md.
- Transitional or partial artifacts detected: src/agents_server.rs.patch, src/mod.rs.patch.
- Current implementation contains 21 Rust source files beyond the explicit spec/design source inventory.
- Root module declarations found in `lib.rs`/`main.rs`: agents_server, compact, protocol, resources, server, transport, tool_registry, grpc.
- Cargo feature flags: default, grpc.
- RPC or protocol definition files: proto/internal_agents.proto, proto/mcp.proto.
- 11 runtime dependencies are present in `Cargo.toml` but omitted from the SPEC dependency block.
