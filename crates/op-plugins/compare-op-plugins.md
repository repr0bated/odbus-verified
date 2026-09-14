# compare-op-plugins

**Date**: 2026-04-05  
**Spec files analyzed**: SPEC.md  
**Analysis mode**: Current system state specification with spec-alignment notes

---

## Summary

| Category | Count |
|---|---|
| Rust source files | 49 |
| Proto files | 0 |
| Binary targets | 0 |
| UI files | 0 |
| Root-declared modules | 11 |
| Partial artifacts | 0 |
| Spec-listed source files | 20 |
| Spec-listed but missing | 0 |
| Extra implementation files | 29 |

## Current Implementation Overview

- Plugin system with state management, domain plugins, and snowball footprints
- Internal crate integrations: op-core, op-dbus-model, op-state, op-state-store, op-snowball, op-network, op-dynamic-loader, op-execution-tracker.

## Module / File Comparison

| Module or File | State | Current Role | Notes |
|---|---|---|---|
| `src/state_plugins/systemd_networkd.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/state_plugins/systemd_networkd.rs |
| `src/state_plugins/mod.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/state_plugins/mod.rs |
| `src/state_plugins/adc.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/state_plugins/adc.rs |
| `src/state_plugins/agent_config.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/state_plugins/agent_config.rs |
| `src/state_plugins/config.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/state_plugins/config.rs |
| `src/state_plugins/dinit.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/state_plugins/dinit.rs |
| `src/state_plugins/dnsresolver.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/state_plugins/dnsresolver.rs |
| `src/state_plugins/endpoint.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/state_plugins/endpoint.rs |
| `src/state_plugins/full_system.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/state_plugins/full_system.rs |
| `src/state_plugins/gcloud_adc.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/state_plugins/gcloud_adc.rs |
| `src/state_plugins/hardware.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/state_plugins/hardware.rs |
| `src/state_plugins/keypair.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/state_plugins/keypair.rs |
| `src/state_plugins/keyring.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/state_plugins/keyring.rs |
| `src/state_plugins/login1.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/state_plugins/login1.rs |
| `src/state_plugins/lxc.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/state_plugins/lxc.rs |
| `src/state_plugins/mcp.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/state_plugins/mcp.rs |
| `src/state_plugins/net.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/state_plugins/net.rs |
| `src/state_plugins/netmaker.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/state_plugins/netmaker.rs |
| `src/state_plugins/openflow.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/state_plugins/openflow.rs |
| `src/state_plugins/openflow_obfuscation.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/state_plugins/openflow_obfuscation.rs |
| `root` | ✅ Present | root source group | src/auto_create.rs, src/builtin.rs, src/chat.rs, src/default_registry.rs, src/dynamic_loading.rs, src/lib.rs, src/plugin.rs, src/registry.rs, ... (+3 more) |
| `state_plugins` | ✅ Present | state_plugins group | src/state_plugins/adc.rs, src/state_plugins/agent_config.rs, src/state_plugins/config.rs, src/state_plugins/dinit.rs, src/state_plugins/dnsresolver.rs, src/state_plugins/endpoint.rs, src/state_plugins/full_system.rs, src/state_plugins/gcloud_adc.rs, ... (+30 more) |

## Feature / Capability Comparison

| Capability | State | Evidence | Source |
|---|---|---|---|
| registry | ✅ Implemented | src/registry.rs | SPEC main module |
| auto_create | ✅ Implemented | src/auto_create.rs | SPEC main module |
| builtin | ✅ Implemented | src/builtin.rs | SPEC main module |
| chat | ✅ Implemented | src/chat.rs | SPEC main module |
| dynamic_loading | ✅ Implemented | src/dynamic_loading.rs | SPEC main module |
| plugin | ✅ Implemented | src/plugin.rs | SPEC main module |
| state | ✅ Implemented | src/state.rs | SPEC main module |
| systemd | ✅ Implemented | src/state_plugins/systemd.rs | SPEC main module |
| default_registry | ✅ Implemented | src/default_registry.rs | SPEC main module |

## Dependencies Comparison

### Internal Workspace Dependencies
- `op-core` - documented in SPEC
- `op-dbus-model` - not listed in SPEC dependency block
- `op-state` - documented in SPEC
- `op-state-store` - documented in SPEC
- `op-snowball` - documented in SPEC
- `op-network` - documented in SPEC
- `op-dynamic-loader` - documented in SPEC
- `op-execution-tracker` - documented in SPEC

### External Runtime Dependencies
- `tokio` - documented in SPEC
- `serde` - documented in SPEC
- `simd-json` - documented in SPEC
- `anyhow` - documented in SPEC
- `thiserror` - documented in SPEC
- `tracing` - documented in SPEC
- `async-trait` - documented in SPEC
- `zbus` - documented in SPEC
- `chrono` - documented in SPEC
- `log` - documented in SPEC
- `reqwest` - documented in SPEC
- `sha2` - documented in SPEC
- `md5` - not listed in SPEC dependency block
- `uuid` - not listed in SPEC dependency block
- `dirs` - not listed in SPEC dependency block
- `parking_lot` - not listed in SPEC dependency block

### Development and Build Dependencies
- `dev:tempfile`

## Notes and Observations

- Local documentation files present: SPEC.md.
- Current implementation contains 29 Rust source files beyond the explicit spec/design source inventory.
- Root module declarations found in `lib.rs`/`main.rs`: auto_create, builtin, chat, dynamic_loading, plugin, registry, service_def, state, default_registry, state_plugins, state_publisher.
- 5 runtime dependencies are present in `Cargo.toml` but omitted from the SPEC dependency block.
