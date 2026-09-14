# compare-op-services

**Date**: 2026-04-05  
**Spec files analyzed**: SPEC.md  
**Analysis mode**: Current system state specification with spec-alignment notes

---

## Summary

| Category | Count |
|---|---|
| Rust source files | 15 |
| Proto files | 1 |
| Binary targets | 3 |
| UI files | 0 |
| Root-declared modules | 5 |
| Partial artifacts | 0 |
| Spec-listed source files | 14 |
| Spec-listed but missing | 0 |
| Extra implementation files | 1 |

## Current Implementation Overview

- System-wide service manager - systemd replacement with dinit backend
- Internal crate integrations: op-plugins.
- Protocol assets: 1 `.proto` files.

## Module / File Comparison

| Module or File | State | Current Role | Notes |
|---|---|---|---|
| `src/bin/systemctl.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/bin/systemctl.rs |
| `src/bin/op-services.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/bin/op-services.rs |
| `src/bin/systemctl-native.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/bin/systemctl-native.rs |
| `src/dbus/mod.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/dbus/mod.rs |
| `src/dbus/interface.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/dbus/interface.rs |
| `src/grpc/server.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/grpc/server.rs |
| `src/grpc/mod.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/grpc/mod.rs |
| `src/manager/service_manager.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/manager/service_manager.rs |
| `src/manager/process.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/manager/process.rs |
| `src/manager/mod.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/manager/mod.rs |
| `src/manager/dinit_proxy.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/manager/dinit_proxy.rs |
| `src/schema/mod.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/schema/mod.rs |
| `src/store/mod.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/store/mod.rs |
| `src/lib.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/lib.rs |
| `bin` | ✅ Present | bin group | src/bin/op-services.rs, src/bin/systemctl-native.rs, src/bin/systemctl.rs |
| `build.rs` | ✅ Present | build script | build.rs |
| `dbus` | ✅ Present | dbus group | src/dbus/interface.rs, src/dbus/mod.rs |
| `grpc` | ✅ Present | grpc group | src/grpc/mod.rs, src/grpc/server.rs |
| `manager` | ✅ Present | manager group | src/manager/dinit_proxy.rs, src/manager/mod.rs, src/manager/process.rs, src/manager/service_manager.rs |
| `root` | ✅ Present | root source group | src/lib.rs |
| `schema` | ✅ Present | schema group | src/schema/mod.rs |
| `store` | ✅ Present | store group | src/store/mod.rs |

## Feature / Capability Comparison

| Capability | State | Evidence | Source |
|---|---|---|---|
| Protocol `services.proto` | ✅ Implemented | proto/services.proto | proto |
| Binary `op-services` | ✅ Implemented | src/bin/op-services.rs | Cargo bin target |
| Binary `systemctl` | ✅ Implemented | src/bin/systemctl.rs | Cargo bin target |
| Binary `systemctl-native` | ✅ Implemented | src/bin/systemctl-native.rs | Cargo bin target |

## Dependencies Comparison

### Internal Workspace Dependencies
- `op-plugins` - documented in SPEC

### External Runtime Dependencies
- `tonic` - documented in SPEC
- `prost` - documented in SPEC
- `prost-types` - documented in SPEC
- `zbus` - documented in SPEC
- `sqlx` - documented in SPEC
- `tokio` - documented in SPEC
- `tokio-stream` - documented in SPEC
- `futures` - documented in SPEC
- `serde` - not listed in SPEC dependency block
- `simd-json` - not listed in SPEC dependency block
- `serde_json` - not listed in SPEC dependency block
- `nix` - not listed in SPEC dependency block
- `libc` - not listed in SPEC dependency block
- `thiserror` - not listed in SPEC dependency block
- `anyhow` - not listed in SPEC dependency block
- `tracing` - not listed in SPEC dependency block
- `tracing-subscriber` - not listed in SPEC dependency block
- `chrono` - not listed in SPEC dependency block
- `toml` - not listed in SPEC dependency block

### Development and Build Dependencies
- `build:tonic-build`

## Notes and Observations

- Local documentation files present: SPEC.md.
- Current implementation contains 1 Rust source files beyond the explicit spec/design source inventory.
- Root module declarations found in `lib.rs`/`main.rs`: dbus, grpc, manager, schema, store.
- RPC or protocol definition files: proto/services.proto.
- 11 runtime dependencies are present in `Cargo.toml` but omitted from the SPEC dependency block.
