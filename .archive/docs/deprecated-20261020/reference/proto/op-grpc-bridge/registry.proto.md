# `registry.proto`

- **Crate:** `op-grpc-bridge`
- **Path:** `crates/op-grpc-bridge/proto/registry.proto`
- **Package:** `operation.registry.v1`
- **Imports:** `google/protobuf/{timestamp,struct}.proto`

Service discovery and component registration for the bridge fabric. Components register
themselves, are discoverable by peers, and emit lifecycle events via a watch stream.

## Services

### `ComponentRegistry`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Register` | `RegisterRequest` | `RegisterResponse` | - |
| `Deregister` | `DeregisterRequest` | `DeregisterResponse` | - |
| `Discover` | `DiscoverRequest` | `DiscoverResponse` | - |
| `GetComponent` | `GetComponentRequest` | `GetComponentResponse` | - |
| `Watch` | `WatchRequest` | `RegistryEvent` | server |
| `Heartbeat` | `HeartbeatRequest` | `HeartbeatResponse` | - |

## Notes

- Registration still flows through the owning D-Bus object; the registry projects it.
- `Heartbeat` drives liveness; components missing heartbeats are candidates for
  deregistration.

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
