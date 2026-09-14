# compare-op-agents

**Date**: 2026-04-05  
**Spec files analyzed**: SPEC.md  
**Analysis mode**: Current system state specification with spec-alignment notes

---

## Summary

| Category | Count |
|---|---|
| Rust source files | 130 |
| Proto files | 0 |
| Binary targets | 2 |
| UI files | 0 |
| Root-declared modules | 6 |
| Partial artifacts | 0 |
| Spec-listed source files | 20 |
| Spec-listed but missing | 0 |
| Extra implementation files | 110 |

## Current Implementation Overview

- Secure agent registry and D-Bus agent implementations for op-dbus-v2
- Internal crate integrations: op-core, op-http.

## Module / File Comparison

| Module or File | State | Current Role | Notes |
|---|---|---|---|
| `src/agents/aiml/prompt_engineer.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/agents/aiml/prompt_engineer.rs |
| `src/agents/aiml/mod.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/agents/aiml/mod.rs |
| `src/agents/aiml/mlops_engineer.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/agents/aiml/mlops_engineer.rs |
| `src/agents/aiml/ml_engineer.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/agents/aiml/ml_engineer.rs |
| `src/agents/aiml/data_scientist.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/agents/aiml/data_scientist.rs |
| `src/agents/aiml/data_engineer.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/agents/aiml/data_engineer.rs |
| `src/agents/aiml/ai_engineer.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/agents/aiml/ai_engineer.rs |
| `src/agents/analysis/security_auditor.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/agents/analysis/security_auditor.rs |
| `src/agents/analysis/performance.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/agents/analysis/performance.rs |
| `src/agents/analysis/mod.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/agents/analysis/mod.rs |
| `src/agents/analysis/debugger.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/agents/analysis/debugger.rs |
| `src/agents/analysis/code_reviewer.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/agents/analysis/code_reviewer.rs |
| `src/agents/architecture/mod.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/agents/architecture/mod.rs |
| `src/agents/architecture/graphql_architect.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/agents/architecture/graphql_architect.rs |
| `src/agents/architecture/frontend_developer.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/agents/architecture/frontend_developer.rs |
| `src/agents/architecture/backend_architect.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/agents/architecture/backend_architect.rs |
| `src/agents/business/sales_automator.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/agents/business/sales_automator.rs |
| `src/agents/business/payment_integration.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/agents/business/payment_integration.rs |
| `src/agents/business/mod.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/agents/business/mod.rs |
| `src/agents/business/legal_advisor.rs` | ✅ Present | Declared in source inventory from spec/design docs | src/agents/business/legal_advisor.rs |
| `agents` | ✅ Present | agents group | src/agents/aiml/ai_engineer.rs, src/agents/aiml/data_engineer.rs, src/agents/aiml/data_scientist.rs, src/agents/aiml/ml_engineer.rs, src/agents/aiml/mlops_engineer.rs, src/agents/aiml/mod.rs, src/agents/aiml/prompt_engineer.rs, src/agents/analysis/code_reviewer.rs, ... (+88 more) |
| `bin` | ✅ Present | bin group | src/bin/dbus-agent-manager.rs, src/bin/dbus-agent.rs |
| `generator` | ✅ Present | generator group | src/generator/md_parser.rs, src/generator/mod.rs, src/generator/template.rs |
| `root` | ✅ Present | root source group | src/agent_catalog.rs, src/agent_registry.rs, src/dbus_service.rs, src/lib.rs, src/router.rs |
| `security` | ✅ Present | security group | src/security/mod.rs, src/security/profiles.rs, src/security/sandbox.rs, src/security/validation.rs |
| `unified` | ✅ Present | unified group | src/unified/agent_trait.rs, src/unified/execution/base.rs, src/unified/execution/golang.rs, src/unified/execution/javascript.rs, src/unified/execution/mod.rs, src/unified/execution/python.rs, src/unified/execution/rust.rs, src/unified/execution/shell.rs, ... (+12 more) |

## Feature / Capability Comparison

| Capability | State | Evidence | Source |
|---|---|---|---|
| dbus_service | ✅ Implemented | src/dbus_service.rs | SPEC main module |
| agent_catalog | ✅ Implemented | src/agent_catalog.rs | SPEC main module |
| agent_registry | ✅ Implemented | src/agent_registry.rs | SPEC main module |
| router | ✅ Implemented | src/router.rs | SPEC main module |
| Binary `dbus-agent` | ✅ Implemented | src/bin/dbus-agent.rs | Cargo bin target |
| Binary `op-agent-manager` | ✅ Implemented | src/bin/dbus-agent-manager.rs | Cargo bin target |

## Dependencies Comparison

### Internal Workspace Dependencies
- `op-core` - documented in SPEC
- `op-http` - documented in SPEC

### External Runtime Dependencies
- `tokio` - documented in SPEC
- `async-trait` - documented in SPEC
- `futures` - documented in SPEC
- `serde` - documented in SPEC
- `simd-json` - documented in SPEC
- `serde_yaml` - documented in SPEC
- `toml` - documented in SPEC
- `anyhow` - documented in SPEC
- `thiserror` - documented in SPEC
- `zbus` - not listed in SPEC dependency block
- `uuid` - not listed in SPEC dependency block
- `chrono` - not listed in SPEC dependency block
- `tracing` - not listed in SPEC dependency block
- `tracing-subscriber` - not listed in SPEC dependency block
- `regex` - not listed in SPEC dependency block
- `shell-words` - not listed in SPEC dependency block
- `axum` - not listed in SPEC dependency block

### Development and Build Dependencies
- `dev:tempfile`

## Notes and Observations

- Local documentation files present: SPEC.md.
- Current implementation contains 110 Rust source files beyond the explicit spec/design source inventory.
- Root module declarations found in `lib.rs`/`main.rs`: agent_catalog, agent_registry, agents, dbus_service, router, security.
- 8 runtime dependencies are present in `Cargo.toml` but omitted from the SPEC dependency block.
