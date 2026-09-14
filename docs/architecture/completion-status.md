# Architecture Completion Status

This document tracks the broader execution items and current completion state.

## Completed

- Canonical mutation path hardened:
  `gRPC/JSON-RPC -> D-Bus ApplyContractMutation -> StateManager -> plugin apply`.
- Schema materialization is active in apply paths.
- Strict schema validation mode exists (`OP_DBUS_STRICT_SCHEMA_VALIDATION`).
- Plugin schema coverage enforcement exists (`OP_DBUS_REQUIRE_PLUGIN_SCHEMA`).
- Systemd->dinit service direction implemented with compatibility alias support.

## Finalized From Partial

### Enforcement Layer

Now production-default strict unless overridden:

- `OP_DBUS_STRICT_SCHEMA_VALIDATION` default: enabled outside tests.
- `OP_DBUS_REQUIRE_PLUGIN_SCHEMA` default: enabled outside tests.

This means invalid materialized state and missing plugin schema entries are fail-fast by default.

### WireGuard-First Scope

Runtime mode added:

- `OP_DBUS_WG_ONLY=true`

Auto-load set in this mode:

- `config`
- `service`
- `dinit`
- `net`
- `wireguard`

This keeps runtime focused on WireGuard/network/service foundations while preserving the rest of the codebase.

### Transport Architecture Validation

Current path and controls are now documented and implemented in code with strict-write options, shared D-Bus connection handling, and bounded in-flight mutation calls.

## Remaining Non-Plugin Work

- Full all-crates review report.
- Identity/signup/magic-link product flow implementation.
- Final crate-boundary rationalization.
- Transition-doc migration and stale-doc archival.
