# State Flow Architecture

## Goal

Ensure all state changes follow one auditable path:

1. External mutation request (gRPC or JSON-RPC)
2. Canonical D-Bus ingress (`org.opdbus.StateManager.ApplyContractMutation`)
3. `StateManager::apply_state_single_plugin` / `apply_state`
4. Schema materialization + validation
5. Plugin diff/apply
6. Persistent state + audit + snowball footprint

## Canonical Write Path

The canonical write path is enforced in `op-grpc-bridge` (`sync_engine.rs`) and routed to D-Bus.

- Preferred ingress: `ApplyContractMutation`
- Strict mode env knobs:
  - `OP_DBUS_STRICT_WRITE_PATH`
  - `OP_DBUS_CANONICAL_WRITE_PATH`
  - `OP_DBUS_ALLOW_LEGACY_WRITE_FALLBACK`
  - `OP_DBUS_DBUS_MAX_IN_FLIGHT`

## Materialization Layer

`StateManager` now materializes desired state before apply:

- Contract payloads: fills missing contract envelope sections and deep-merges user payload.
- Non-contract payloads: generates plugin template from `SchemaRegistry` and deep-merges input.

This gives schema-coupled propagation at mutation time.

## Why This Matters

- Removes ad-hoc object creation logic.
- Makes plugin schema the source of default field propagation.
- Keeps mutation flow deterministic and auditable.
- Prepares semantic/privacy indexing with less drift across plugins.
