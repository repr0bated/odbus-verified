# Schema-as-Code Hierarchy

## Principle

**Plugins own schemas. Consumers re-export.**

Rust types ARE the schema. Validation happens at parse time via `TryFrom`, `serde`, and constructor validation.

## Hierarchy

```
op-plugins/src/state_plugins/*.rs   ← SOURCE OF TRUTH for config schemas
op-plugins/src/systemd.rs           ← SOURCE OF TRUTH for service schema
    │
    ├── ServiceName, ServiceDef, ServiceType
    ├── ExecCommand, RestartPolicy, RestartCondition
    ├── ResourceLimits, ValidationError
    │
    ▼
op-services/src/schema/mod.rs       ← RE-EXPORTS + internal state machine
    │
    ├── pub use op_plugins::systemd::*
    ├── + ServiceState (enum: Stopped/Starting/Running/Stopping/Failed)
    ├── + ServiceStatus (runtime status with pid, error, started_at)

op-gateway/src/wireguard_auth.rs    ← RUNTIME types (not config)
    │
    ├── WireGuardSession (auth session, not WG config)
    ├── WireGuardStats (metrics)
    ├── WireGuardAuthManager (runtime manager)
```

## Correct Separation

| Layer | Purpose | Example |
|-------|---------|---------|
| Plugin schema | What you DECLARE (config) | `ServiceDef`, `WireGuardInterface` |
| Runtime types | What you OBSERVE (state) | `ServiceStatus`, `WireGuardSession` |

## Plugin Schemas (Source of Truth)

| Plugin | Schema Types |
|--------|--------------|
| systemd | ServiceName, ServiceDef, ServiceType, ExecCommand, RestartPolicy |
| wireguard | WireGuardState, WireGuardInterface, WireGuardPeer |
| net | NetworkConfig, InterfaceConfig |
| lxc | LxcState, ContainerInfo |
| openflow | OpenFlowConfig |
| users | UsersState, UserConfig |
| full_system | FullSystemState (aggregator - queries other plugins) |

## Rules

1. **Never duplicate schema types** - re-export from plugin
2. **Runtime types are separate** - state machines, sessions, metrics live in consumer crates
3. **Validation in constructors** - `ServiceName::new()` validates, not a separate `validate()` call
4. **serde for parsing** - `#[serde(try_from = "String")]` for validated newtypes
