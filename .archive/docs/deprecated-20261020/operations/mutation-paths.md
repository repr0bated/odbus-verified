# Mutation Paths

## Supported Mutation Sources

- gRPC mutation requests
- JSON-RPC mutation requests
- Internal mutation requests

All should converge on the same state mutation core.

## Canonical Ingress

`org.opdbus.StateManager.ApplyContractMutation`

Implementation path:

- `op-grpc-bridge/src/sync_engine.rs` validates contract envelope and forwards mutation over D-Bus.
- `op-state/src/dbus_server.rs` receives mutation and calls:
  - `StateManager::apply_state_single_plugin`

## Processing Stages

1. Envelope validation (required contract fields)
2. Materialization (schema defaults + deep merge)
3. Optional schema validation warning stage
4. Plugin diff calculation
5. Plugin apply
6. Persistence and footprinting

## Operational Guarantees

- One write ingress surface in strict mode.
- Shared D-Bus connection + bounded in-flight calls in bridge.
- Same apply logic for gRPC and JSON-RPC sources.

## Recovery/Observability Notes

- Apply checkpoints are created per plugin.
- Apply results and failures are logged.
- Footprints are recorded to snowball sender when enabled.
