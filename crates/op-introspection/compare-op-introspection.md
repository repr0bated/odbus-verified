# compare-op-introspection

**Date**: 2026-04-05  
**Spec files analyzed**: SPEC.md  
**Analysis mode**: Current system state specification with spec-alignment notes

---

## Summary

| Category | Count |
|---|---|
| Rust source files | 10 |
| Proto files | 0 |
| Binary targets | 0 |
| UI files | 0 |
| Root-declared modules | 6 |
| Partial artifacts | 0 |
| Spec-listed source files | 10 |
| Spec-listed but missing | 0 |
| Extra implementation files | 0 |

## Current Implementation Overview

- DBus introspection capabilities for op-dbus-v2
- Internal crate integrations: op-core, op-snowball.

## Module / File Comparison

| Module or File | State | Current Role | Notes |
|---|---|---|---|
| `src/scanner.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/scanner.rs |
| `src/projection.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/projection.rs |
| `src/parser.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/parser.rs |
| `src/mod.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/mod.rs |
| `src/lib.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/lib.rs |
| `src/indexer_manager.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/indexer_manager.rs |
| `src/indexer.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/indexer.rs |
| `src/hierarchical.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/hierarchical.rs |
| `src/cpu_features.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/cpu_features.rs |
| `src/cache.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/cache.rs |
| `root` | ✅ Present | root source group | src/cache.rs, src/cpu_features.rs, src/hierarchical.rs, src/indexer.rs, src/indexer_manager.rs, src/lib.rs, src/mod.rs, src/parser.rs, ... (+2 more) |

## Feature / Capability Comparison

| Capability | State | Evidence | Source |
|---|---|---|---|
| scanner | ✅ Implemented | src/scanner.rs | SPEC main module |
| projection | ✅ Implemented | src/projection.rs | SPEC main module |
| parser | ✅ Implemented | src/parser.rs | SPEC main module |
| mod | ✅ Implemented | src/mod.rs | SPEC main module |
| indexer_manager | ✅ Implemented | src/indexer_manager.rs | SPEC main module |
| indexer | ✅ Implemented | src/indexer.rs | SPEC main module |
| hierarchical | ✅ Implemented | src/hierarchical.rs | SPEC main module |
| cpu_features | ✅ Implemented | src/cpu_features.rs | SPEC main module |
| cache | ✅ Implemented | src/cache.rs | SPEC main module |

## Dependencies Comparison

### Internal Workspace Dependencies
- `op-core` - documented in SPEC
- `op-snowball` - documented in SPEC

### External Runtime Dependencies
- `tokio` - documented in SPEC
- `serde` - documented in SPEC
- `simd-json` - documented in SPEC
- `zbus` - documented in SPEC
- `zbus_xml` - documented in SPEC
- `anyhow` - documented in SPEC
- `thiserror` - documented in SPEC
- `tracing` - documented in SPEC
- `futures` - documented in SPEC
- `async-trait` - documented in SPEC
- `quick-xml` - documented in SPEC
- `rusqlite` - documented in SPEC
- `chrono` - documented in SPEC
- `parking_lot` - documented in SPEC
- `sha2` - documented in SPEC
- `hex` - documented in SPEC

### Development and Build Dependencies
- None

## Notes and Observations

- Local documentation files present: SPEC.md.
- Root module declarations found in `lib.rs`/`main.rs`: cache, indexer, indexer_manager, parser, projection, scanner.
