# compare-op-jsonrpc

**Date**: 2026-04-05  
**Spec files analyzed**: SPEC.md  
**Analysis mode**: Current system state specification with spec-alignment notes

---

## Summary

| Category | Count |
|---|---|
| Rust source files | 8 |
| Proto files | 0 |
| Binary targets | 0 |
| UI files | 0 |
| Root-declared modules | 4 |
| Partial artifacts | 0 |
| Spec-listed source files | 0 |
| Spec-listed but missing | 0 |
| Extra implementation files | 8 |

## Current Implementation Overview

- JSON-RPC server with OVSDB and NonNet database support for op-dbus-v2
- Internal crate integrations: op-core.

## Module / File Comparison

| Module or File | State | Current Role | Notes |
|---|---|---|---|
| `root` | ✅ Present | root source group | src/lib.rs, src/nonnet.rs, src/nonnet_staging.rs, src/ovsdb.rs, src/ovsdb_jsonrpc.rs, src/ovsdb_rpc_call.rs, src/protocol.rs, src/server.rs |

## Feature / Capability Comparison

| Capability | State | Evidence | Source |
|---|---|---|---|
| Architecture | ❌ Missing | no clear source match for SPEC.md | SPEC.md |
| Key Components | ❌ Missing | no clear source match for SPEC.md | SPEC.md |
| Module Structure | ✅ Implemented | src/nonnet.rs | SPEC.md |

## Dependencies Comparison

### Internal Workspace Dependencies
- `op-core` - not listed in SPEC dependency block

### External Runtime Dependencies
- `tokio` - not listed in SPEC dependency block
- `serde` - not listed in SPEC dependency block
- `simd-json` - not listed in SPEC dependency block
- `anyhow` - not listed in SPEC dependency block
- `thiserror` - not listed in SPEC dependency block
- `tracing` - not listed in SPEC dependency block
- `uuid` - not listed in SPEC dependency block

### Development and Build Dependencies
- None

## Notes and Observations

- Local documentation files present: SPEC.md.
- Current implementation contains 8 Rust source files beyond the explicit spec/design source inventory.
- Root module declarations found in `lib.rs`/`main.rs`: nonnet, ovsdb, protocol, server.
