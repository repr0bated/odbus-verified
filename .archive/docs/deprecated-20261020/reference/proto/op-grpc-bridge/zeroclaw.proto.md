# `zeroclaw.proto`

- **Crate:** `op-grpc-bridge`
- **Path:** `crates/op-grpc-bridge/src/grpc/zeroclaw.proto`
- **Package:** `zeroclaw`

Schema distribution for the Zeroclaw trusted-proxy path. Clients fetch the active schema
and subscribe to schema changes.

## Services

### `ZeroclawService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `GetSchema` | `GetSchemaRequest` | `SchemaResponse` | - |
| `WatchSchema` | `WatchSchemaRequest` | `SchemaEvent` | server |

## Notes

- Lives under `src/grpc/` (not the crate `proto/` dir) but is a project-owned contract.
- Related Zeroclaw messages are also referenced from
  [`operation.proto`](./operation.proto.md).

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
