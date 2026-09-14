# compare-op-tools

**Date**: 2026-04-05  
**Spec files analyzed**: SPEC.md  
**Analysis mode**: Current system state specification with spec-alignment notes

---

## Summary

| Category | Count |
|---|---|
| Rust source files | 50 |
| Proto files | 0 |
| Binary targets | 0 |
| UI files | 0 |
| Root-declared modules | 10 |
| Partial artifacts | 3 |
| Spec-listed source files | 20 |
| Spec-listed but missing | 0 |
| Extra implementation files | 30 |

## Current Implementation Overview

- Tool registry and execution for op-dbus-v2
- Internal crate integrations: op-core, op-introspection, op-inspector, op-network, op-http, op-agents, op-state, op-execution-tracker.

## Module / File Comparison

| Module or File | State | Current Role | Notes |
|---|---|---|---|
| `src/bin/op-packagekit-install.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/bin/op-packagekit-install.rs |
| `src/builtin/dinit.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/dinit.rs |
| `src/builtin/system.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/system.rs |
| `src/builtin/shell_tool.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/shell_tool.rs |
| `src/builtin/shell.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/shell.rs |
| `src/builtin/self_tools.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/self_tools.rs |
| `src/builtin/rtnetlink_tools.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/rtnetlink_tools.rs |
| `src/builtin/response_tools.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/response_tools.rs |
| `src/builtin/respond_tool.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/respond_tool.rs |
| `src/builtin/procfs.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/procfs.rs |
| `src/builtin/packagekit.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/packagekit.rs |
| `src/builtin/ovsdb.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/ovsdb.rs |
| `src/builtin/ovs_tools.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/ovs_tools.rs |
| `src/builtin/ovs.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/ovs.rs |
| `src/builtin/openflow_tools.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/openflow_tools.rs |
| `src/builtin/mod.rs` | ⚠️ Partial | Declared in source inventory from spec/design docs | src/builtin/mod.rs; partial artifacts: src/builtin/mod.rs.fix, src/builtin/mod.rs.patch |
| `src/builtin/lxc_tools.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/lxc_tools.rs |
| `src/builtin/gcloud_tools.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/gcloud_tools.rs |
| `src/builtin/file.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/file.rs |
| `src/builtin/error_reporting_tool.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/builtin/error_reporting_tool.rs |
| `bin` | ✅ Present | bin group | src/bin/op-packagekit-install.rs |
| `builtin` | ✅ Present | builtin group | src/builtin/agent_tool.rs, src/builtin/anydesk.rs, src/builtin/code_search.rs, src/builtin/dbus.rs, src/builtin/dbus_hybrid.rs, src/builtin/dbus_introspection.rs, src/builtin/dbus_search_tool.rs, src/builtin/dbus_tool.rs, ... (+23 more) |
| `discovery` | ✅ Present | discovery group | src/discovery/mod.rs, src/discovery/projection_engine.rs, src/discovery/sources/agent.rs, src/discovery/sources/dbus.rs, src/discovery/sources/mod.rs, src/discovery/sources/plugin.rs |
| `root` | ✅ Present | root source group | src/builtin_old.rs, src/dynamic_tool.rs, src/executor.rs, src/lib.rs, src/mcptools.rs, src/orchestration_plugin.rs, src/registry.rs, src/router.rs, ... (+4 more) |

## Feature / Capability Comparison

| Capability | State | Evidence | Source |
|---|---|---|---|
| validation_tests | ✅ Implemented | src/validation_tests.rs | SPEC main module |
| validation | ✅ Implemented | src/validation.rs | SPEC main module |
| tool | ✅ Implemented | src/tool.rs | SPEC main module |
| security | ✅ Implemented | src/security.rs | SPEC main module |
| router | ✅ Implemented | src/router.rs | SPEC main module |
| registry | ✅ Implemented | src/registry.rs | SPEC main module |
| orchestration_plugin | ✅ Implemented | src/orchestration_plugin.rs | SPEC main module |
| mcptools | ✅ Implemented | src/mcptools.rs | SPEC main module |
| executor | ✅ Implemented | src/executor.rs | SPEC main module |
| dynamic_tool | ✅ Implemented | src/dynamic_tool.rs | SPEC main module |

## Dependencies Comparison

### Internal Workspace Dependencies
- `op-core` - not listed in SPEC dependency block
- `op-introspection` - not listed in SPEC dependency block
- `op-inspector` - not listed in SPEC dependency block
- `op-network` - not listed in SPEC dependency block
- `op-http` - not listed in SPEC dependency block
- `op-agents` - not listed in SPEC dependency block
- `op-state` - not listed in SPEC dependency block
- `op-execution-tracker` - not listed in SPEC dependency block

### External Runtime Dependencies
- `tokio` - documented in SPEC
- `async-trait` - documented in SPEC
- `serde` - documented in SPEC
- `simd-json` - documented in SPEC
- `serde_json` - documented in SPEC
- `anyhow` - documented in SPEC
- `thiserror` - documented in SPEC
- `tracing` - documented in SPEC
- `clap` - documented in SPEC
- `futures` - documented in SPEC
- `chrono` - documented in SPEC
- `uuid` - not listed in SPEC dependency block
- `zbus` - not listed in SPEC dependency block
- `axum` - not listed in SPEC dependency block
- `reqwest` - not listed in SPEC dependency block
- `lazy_static` - not listed in SPEC dependency block
- `async-recursion` - not listed in SPEC dependency block
- `dirs` - not listed in SPEC dependency block
- `jsonschema` - not listed in SPEC dependency block

### Development and Build Dependencies
- `dev:tokio-test`

## Notes and Observations

- Local documentation files present: SPEC.md.
- Transitional or partial artifacts detected: Cargo.toml.patch, src/builtin/mod.rs.fix, src/builtin/mod.rs.patch.
- Current implementation contains 30 Rust source files beyond the explicit spec/design source inventory.
- Root module declarations found in `lib.rs`/`main.rs`: builtin, discovery, dynamic_tool, mcptools, orchestration_plugin, registry, router, security, tool, validation.
- 16 runtime dependencies are present in `Cargo.toml` but omitted from the SPEC dependency block.
