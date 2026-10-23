# `namespace.proto`

- **Crate:** `op-assistant-grpc`
- **Path:** `crates/op-assistant-grpc/proto/assistant/namespace.proto`
- **Package:** `assistant.v1`
- **Imports:** `google/protobuf/timestamp.proto`, `assistant/common.proto`

Namespaced memory management (per-scope memory partitions).

## Services

### `NamespaceMemoryService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `GetMemoryNamespace` | `GetMemoryNamespaceRequest` | `MemoryNamespace` | - |
| `SetMemoryNamespace` | `SetMemoryNamespaceRequest` | `MemoryNamespace` | - |
| `ClearMemoryNamespace` | `ClearMemoryNamespaceRequest` | `Empty` | - |
| `ListMemoryNamespaces` | `ListMemoryNamespacesRequest` | `ListMemoryNamespacesResponse` | - |

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
