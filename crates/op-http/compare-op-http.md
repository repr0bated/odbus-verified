# compare-op-http

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
| Spec-listed source files | 8 |
| Spec-listed but missing | 0 |
| Extra implementation files | 0 |

## Current Implementation Overview

- Central HTTP/TLS server for all op-dbus modules

## Module / File Comparison

| Module or File | State | Current Role | Notes |
|---|---|---|---|
| `src/tls.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/tls.rs |
| `src/server.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/server.rs |
| `src/router.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/router.rs |
| `src/request_filters.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/request_filters.rs |
| `src/middleware.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/middleware.rs |
| `src/metrics.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/metrics.rs |
| `src/lib.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/lib.rs |
| `src/health.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/health.rs |
| `root` | ✅ Present | root source group | src/health.rs, src/lib.rs, src/metrics.rs, src/middleware.rs, src/request_filters.rs, src/router.rs, src/server.rs, src/tls.rs |

## Feature / Capability Comparison

| Capability | State | Evidence | Source |
|---|---|---|---|
| tls | ✅ Implemented | src/tls.rs | SPEC main module |
| server | ✅ Implemented | src/server.rs | SPEC main module |
| router | ✅ Implemented | src/router.rs | SPEC main module |
| request_filters | ✅ Implemented | src/request_filters.rs | SPEC main module |
| middleware | ✅ Implemented | src/middleware.rs | SPEC main module |
| metrics | ✅ Implemented | src/metrics.rs | SPEC main module |
| health | ✅ Implemented | src/health.rs | SPEC main module |

## Dependencies Comparison

### Internal Workspace Dependencies
- None

### External Runtime Dependencies
- `tokio` - documented in SPEC
- `futures` - documented in SPEC
- `serde` - documented in SPEC
- `simd-json` - documented in SPEC
- `anyhow` - documented in SPEC
- `thiserror` - documented in SPEC
- `tracing` - documented in SPEC
- `axum` - documented in SPEC
- `tower` - documented in SPEC
- `tower-http` - documented in SPEC
- `hyper` - documented in SPEC
- `hyper-util` - not listed in SPEC dependency block
- `rustls` - not listed in SPEC dependency block
- `rustls-pemfile` - not listed in SPEC dependency block
- `tokio-rustls` - not listed in SPEC dependency block
- `chrono` - not listed in SPEC dependency block
- `gethostname` - not listed in SPEC dependency block

### Development and Build Dependencies
- None

## Notes and Observations

- Local documentation files present: SPEC.md.
- Root module declarations found in `lib.rs`/`main.rs`: middleware, router, server, tls.
- 6 runtime dependencies are present in `Cargo.toml` but omitted from the SPEC dependency block.
