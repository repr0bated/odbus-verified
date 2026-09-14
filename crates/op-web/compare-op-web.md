# compare-op-web

**Date**: 2026-04-05  
**Spec files analyzed**: SPEC.md, ui/README.md  
**Analysis mode**: Current system state specification with spec-alignment notes

---

## Summary

| Category | Count |
|---|---|
| Rust source files | 56 |
| Proto files | 0 |
| Binary targets | 1 |
| UI files | 108 |
| Root-declared modules | 21 |
| Partial artifacts | 6 |
| Spec-listed source files | 20 |
| Spec-listed but missing | 0 |
| Extra implementation files | 36 |

## Current Implementation Overview

- Unified web server for op-dbus-v2 - consolidates all HTTP services
- Internal crate integrations: op-core, op-chat, op-llm, op-tools, op-agents, op-state, op-network, op-mcp, op-mcp-aggregator, op-state-store....
- Frontend assets: 108 files under `ui/src/`.

## Module / File Comparison

| Module or File | State | Current Role | Notes |
|---|---|---|---|
| `src/handlers/llm.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/handlers/llm.rs |
| `src/handlers/health.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/handlers/health.rs |
| `src/handlers/chat.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/handlers/chat.rs |
| `src/handlers/auth_bridge.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/handlers/auth_bridge.rs |
| `src/handlers/agents.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/handlers/agents.rs |
| `src/handlers/tools.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/handlers/tools.rs |
| `src/handlers/status.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/handlers/status.rs |
| `src/handlers/mod.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/handlers/mod.rs |
| `src/handlers/websocket.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/handlers/websocket.rs |
| `src/handlers/privacy.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/handlers/privacy.rs |
| `src/middleware/security.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/middleware/security.rs |
| `src/middleware/mod.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/middleware/mod.rs |
| `src/orchestrator/types.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/orchestrator/types.rs |
| `src/orchestrator/parsing.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/orchestrator/parsing.rs |
| `src/orchestrator/mod.rs` | ⚠️ Partial | Declared in source inventory from spec/design docs | src/orchestrator/mod.rs; partial artifacts: src/orchestrator/mod.rs.patch |
| `src/orchestrator/formatting.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/orchestrator/formatting.rs |
| `src/orchestrator/execution.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/orchestrator/execution.rs |
| `src/orchestrator/anti_hallucination.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/orchestrator/anti_hallucination.rs |
| `src/orchestrator/tools.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/orchestrator/tools.rs |
| `src/orchestrator/process.rs` | ⚠️ Partial | Declared in source inventory from spec/design docs | src/orchestrator/process.rs; partial artifacts: src/orchestrator/process.rs.patch |
| `bin` | ✅ Present | bin group | src/bin/op-dbus.rs |
| `build.rs` | ✅ Present | build script | build.rs |
| `handlers` | ✅ Present | handlers group | src/handlers/agents.rs, src/handlers/auth_bridge.rs, src/handlers/chat.rs, src/handlers/dashboard.rs, src/handlers/health.rs, src/handlers/llm.rs, src/handlers/logs.rs, src/handlers/mail.rs, ... (+9 more) |
| `middleware` | ✅ Present | middleware group | src/middleware/mod.rs, src/middleware/security.rs |
| `orchestrator` | ✅ Present | orchestrator group | src/orchestrator/anti_hallucination.rs, src/orchestrator/execution.rs, src/orchestrator/formatting.rs, src/orchestrator/mod.rs, src/orchestrator/parsing.rs, src/orchestrator/process.rs, src/orchestrator/tools.rs, src/orchestrator/types.rs |
| `root` | ✅ Present | root source group | src/email.rs, src/embedded_ui.rs, src/groups_admin.rs, src/lib.rs, src/main.rs, src/mcp.rs, src/mcp_agents.rs, src/mcp_compact.rs, ... (+15 more) |
| `routes` | ✅ Present | routes group | src/routes/admin.rs, src/routes/chat.rs, src/routes/llm.rs, src/routes/mod.rs |
| `ui/src` | ✅ Present | Frontend source tree | App.tsx, components/JsonRenderer.tsx, components/NavLink.tsx, components/chat/GenerativeBlock.tsx, components/chat/MessageBubble.tsx, components/chat/SystemPromptEditor.tsx, components/dashboard/EventDistribution.tsx, components/dashboard/ResourceGauge.tsx, ... (+100 more) |

## Feature / Capability Comparison

| Capability | State | Evidence | Source |
|---|---|---|---|
| websocket | ✅ Implemented | src/handlers/websocket.rs, src/websocket.rs | SPEC main module |
| users | ✅ Implemented | src/handlers/users.rs, src/users.rs | SPEC main module |
| system_prompt_loader | ✅ Implemented | src/system_prompt_loader.rs | SPEC main module |
| sse | ✅ Implemented | src/sse.rs | SPEC main module |
| server | ✅ Implemented | src/server.rs | SPEC main module |
| router | ✅ Implemented | src/router.rs | SPEC main module |
| mcp_compact | ✅ Implemented | src/mcp_compact.rs | SPEC main module |
| mcp_agents | ✅ Implemented | src/mcp_agents.rs | SPEC main module |
| mcp | ✅ Implemented | src/handlers/mcp.rs, src/mcp.rs | SPEC main module |
| embedded_ui | ✅ Implemented | src/embedded_ui.rs | SPEC main module |
| Binary `op-web-server` | ⚠️ Partial | src/main.rs | Cargo bin target |
| Frontend UI | ✅ Implemented | `ui/src/` contains 108 files | frontend |

## Dependencies Comparison

### Internal Workspace Dependencies
- `op-core` - documented in SPEC
- `op-chat` - documented in SPEC
- `op-llm` - documented in SPEC
- `op-tools` - documented in SPEC
- `op-agents` - documented in SPEC
- `op-state` - documented in SPEC
- `op-network` - documented in SPEC
- `op-mcp` - documented in SPEC
- `op-mcp-aggregator` - documented in SPEC
- `op-state-store` - documented in SPEC
- `op-identity` - documented in SPEC
- `op-introspection` - documented in SPEC
- `op-grpc-bridge` - documented in SPEC

### External Runtime Dependencies
- `tower_governor` - documented in SPEC
- `axum` - documented in SPEC
- `tokio` - not listed in SPEC dependency block
- `tower` - not listed in SPEC dependency block
- `tower-http` - not listed in SPEC dependency block
- `hyper` - not listed in SPEC dependency block
- `serde` - not listed in SPEC dependency block
- `simd-json` - not listed in SPEC dependency block
- `toml` - not listed in SPEC dependency block
- `futures` - not listed in SPEC dependency block
- `async-trait` - not listed in SPEC dependency block
- `tokio-stream` - not listed in SPEC dependency block
- `async-stream` - not listed in SPEC dependency block
- `uuid` - not listed in SPEC dependency block
- `chrono` - not listed in SPEC dependency block
- `tracing` - not listed in SPEC dependency block
- `tracing-subscriber` - not listed in SPEC dependency block
- `anyhow` - not listed in SPEC dependency block
- `thiserror` - not listed in SPEC dependency block
- `sysinfo` - not listed in SPEC dependency block
- `gethostname` - not listed in SPEC dependency block
- `lazy_static` - not listed in SPEC dependency block
- `regex` - not listed in SPEC dependency block
- `qrcode` - not listed in SPEC dependency block
- `image` - not listed in SPEC dependency block
- `base64` - not listed in SPEC dependency block
- `lettre` - not listed in SPEC dependency block
- `hex` - not listed in SPEC dependency block
- `zbus` - not listed in SPEC dependency block
- `ring` - not listed in SPEC dependency block
- `oauth2` - not listed in SPEC dependency block
- `reqwest` - not listed in SPEC dependency block
- `rust-embed` - not listed in SPEC dependency block
- `axum-embed` - not listed in SPEC dependency block
- `mime_guess` - not listed in SPEC dependency block
- `linemux` - not listed in SPEC dependency block

### Development and Build Dependencies
- None

## Notes and Observations

- Local documentation files present: SPEC.md, ui/README.md.
- Transitional or partial artifacts detected: src/chat_handler.rs.fix, src/main.rs.patch, src/orchestrator/mod.rs.patch, src/orchestrator/process.rs.patch, src/routes.rs.patch, src/routes/mod.rs.patch.
- Current implementation contains 36 Rust source files beyond the explicit spec/design source inventory.
- Root module declarations found in `lib.rs`/`main.rs`: email, embedded_ui, groups_admin, handlers, mcp, mcp_agents, mcp_compact, mcp_discovery, middleware, orchestrator, privacy_container, privacy_network, privacy_openflow, privacy_routes, routes, sse, state, state_manager_client, users, websocket....
- Frontend state is part of this crate via `ui/src/`; this is operational code, not just static assets.
- 34 runtime dependencies are present in `Cargo.toml` but omitted from the SPEC dependency block.
