# compare-op-workflows

**Date**: 2026-04-05  
**Spec files analyzed**: SPEC.md  
**Analysis mode**: Current system state specification with spec-alignment notes

---

## Summary

| Category | Count |
|---|---|
| Rust source files | 13 |
| Proto files | 0 |
| Binary targets | 0 |
| UI files | 0 |
| Root-declared modules | 8 |
| Partial artifacts | 0 |
| Spec-listed source files | 12 |
| Spec-listed but missing | 0 |
| Extra implementation files | 1 |

## Current Implementation Overview

- Workflow engine with plugin/service nodes for op-dbus-v2
- Internal crate integrations: op-core, op-plugins, op-tools, op-execution-tracker.

## Module / File Comparison

| Module or File | State | Current Role | Notes |
|---|---|---|---|
| `src/builtin/tool_node.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/tool_node.rs |
| `src/builtin/mod.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/mod.rs |
| `src/builtin/definitions.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/definitions.rs |
| `src/builtin/dbus_node.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/dbus_node.rs |
| `src/builtin/plugin_node.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/plugin_node.rs |
| `src/workflows.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/workflows.rs |
| `src/orchestrator.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/orchestrator.rs |
| `src/node.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/node.rs |
| `src/lib.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/lib.rs |
| `src/flow.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/flow.rs |
| `src/engine.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/engine.rs |
| `src/context.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/context.rs |
| `builtin` | ✅ Present | builtin group | src/builtin/dbus_node.rs, src/builtin/definitions.rs, src/builtin/mod.rs, src/builtin/plugin_node.rs, src/builtin/tool_node.rs |
| `root` | ✅ Present | root source group | src/context.rs, src/engine.rs, src/flow.rs, src/history.rs, src/lib.rs, src/node.rs, src/orchestrator.rs, src/workflows.rs |

## Feature / Capability Comparison

| Capability | State | Evidence | Source |
|---|---|---|---|
| workflows | ✅ Implemented | src/workflows.rs | SPEC main module |
| orchestrator | ✅ Implemented | src/orchestrator.rs | SPEC main module |
| node | ✅ Implemented | src/node.rs | SPEC main module |
| flow | ✅ Implemented | src/flow.rs | SPEC main module |
| engine | ✅ Implemented | src/engine.rs | SPEC main module |
| context | ✅ Implemented | src/context.rs | SPEC main module |

## Dependencies Comparison

### Internal Workspace Dependencies
- `op-core` - documented in SPEC
- `op-plugins` - documented in SPEC
- `op-tools` - documented in SPEC
- `op-execution-tracker` - documented in SPEC

### External Runtime Dependencies
- `tokio` - documented in SPEC
- `serde` - documented in SPEC
- `simd-json` - documented in SPEC
- `anyhow` - documented in SPEC
- `thiserror` - documented in SPEC
- `tracing` - documented in SPEC
- `async-trait` - documented in SPEC
- `uuid` - documented in SPEC
- `chrono` - documented in SPEC
- `sha2` - documented in SPEC
- `hex` - documented in SPEC
- `pocketflow_rs` - documented in SPEC
- `log` - documented in SPEC
- `serde_json` - documented in SPEC

### Development and Build Dependencies
- None

## Notes and Observations

- Local documentation files present: SPEC.md.
- Current implementation contains 1 Rust source files beyond the explicit spec/design source inventory.
- Root module declarations found in `lib.rs`/`main.rs`: builtin, context, engine, flow, history, node, orchestrator, workflows.
