# compare-op-grpc-bridge

**Date**: 2026-04-05  
**Spec files analyzed**: SPEC.md  
**Analysis mode**: Current system state specification with spec-alignment notes

---

## Summary

| Category | Count |
|---|---|
| Rust source files | 8 |
| Proto files | 5 |
| Binary targets | 0 |
| UI files | 0 |
| Root-declared modules | 5 |
| Partial artifacts | 0 |
| Spec-listed source files | 6 |
| Spec-listed but missing | 0 |
| Extra implementation files | 2 |

## Current Implementation Overview

- Bidirectional D-Bus <-> gRPC bridge with event chain integration
- Internal crate integrations: op-core, op-state-store, op-identity, op-network.
- Protocol assets: 5 `.proto` files.

## Module / File Comparison

| Module or File | State | Current Role | Notes |
|---|---|---|---|
| `src/sync_engine.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/sync_engine.rs |
| `src/proto_gen.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/proto_gen.rs |
| `src/lib.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/lib.rs |
| `src/grpc_server.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/grpc_server.rs |
| `src/grpc_client.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/grpc_client.rs |
| `src/dbus_watcher.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/dbus_watcher.rs |
| `build.rs` | ✅ Present | build script | build.rs |
| `root` | ✅ Present | root source group | src/dbus_watcher.rs, src/grpc_client.rs, src/grpc_server.rs, src/lib.rs, src/proto_gen.rs, src/schema_engine.rs, src/sync_engine.rs |

## Feature / Capability Comparison

| Capability | State | Evidence | Source |
|---|---|---|---|
| sync_engine | ✅ Implemented | src/sync_engine.rs | SPEC main module |
| proto_gen | ✅ Implemented | src/proto_gen.rs | SPEC main module |
| grpc_server | ✅ Implemented | src/grpc_server.rs | SPEC main module |
| grpc_client | ✅ Implemented | src/grpc_client.rs | SPEC main module |
| dbus_watcher | ✅ Implemented | src/dbus_watcher.rs | SPEC main module |
| Protocol `mail.proto` | ✅ Implemented | proto/mail.proto | proto |
| Protocol `operation.proto` | ✅ Implemented | proto/operation.proto | proto |
| Protocol `privacy_network.proto` | ✅ Implemented | proto/privacy_network.proto | proto |
| Protocol `registration.proto` | ✅ Implemented | proto/registration.proto | proto |
| Protocol `registry.proto` | ✅ Implemented | proto/registry.proto | proto |

## Dependencies Comparison

### Internal Workspace Dependencies
- `op-core` - not listed in SPEC dependency block
- `op-state-store` - documented in SPEC
- `op-identity` - not listed in SPEC dependency block
- `op-network` - not listed in SPEC dependency block

### External Runtime Dependencies
- `tonic` - documented in SPEC
- `tonic-web` - not listed in SPEC dependency block
- `prost` - documented in SPEC
- `prost-types` - documented in SPEC
- `tonic-reflection` - documented in SPEC
- `tonic-health` - not listed in SPEC dependency block
- `tokio` - documented in SPEC
- `tokio-stream` - documented in SPEC
- `zbus` - documented in SPEC
- `serde` - documented in SPEC
- `serde_json` - documented in SPEC
- `simd-json` - documented in SPEC
- `tracing` - not listed in SPEC dependency block
- `anyhow` - not listed in SPEC dependency block
- `thiserror` - not listed in SPEC dependency block
- `async-trait` - not listed in SPEC dependency block
- `uuid` - not listed in SPEC dependency block
- `chrono` - not listed in SPEC dependency block
- `futures` - not listed in SPEC dependency block
- `async-stream` - not listed in SPEC dependency block
- `base64` - not listed in SPEC dependency block

### Development and Build Dependencies
- `dev:tokio-test`
- `build:tonic-build`

## Notes and Observations

- Local documentation files present: SPEC.md.
- Current implementation contains 2 Rust source files beyond the explicit spec/design source inventory.
- Root module declarations found in `lib.rs`/`main.rs`: dbus_watcher, grpc_client, grpc_server, proto_gen, schema_engine.
- RPC or protocol definition files: proto/mail.proto, proto/operation.proto, proto/privacy_network.proto, proto/registration.proto, proto/registry.proto.
- 14 runtime dependencies are present in `Cargo.toml` but omitted from the SPEC dependency block.
