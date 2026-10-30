# `memory.proto`

- **Crate:** `op-assistant-grpc`
- **Path:** `crates/op-assistant-grpc/proto/assistant/memory.proto`
- **Package:** `assistant.v1`
- **Imports:** `google/protobuf/{timestamp,struct}.proto`, `assistant/common.proto`

Assistant memory read/write/search with stats.

## Services

### `MemoryService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `ReadMemory` | `ReadMemoryRequest` | `ReadMemoryResponse` | - |
| `WriteMemory` | `WriteMemoryRequest` | `WriteMemoryResponse` | - |
| `DeleteMemory` | `DeleteMemoryRequest` | `DeleteMemoryResponse` | - |
| `SearchMemory` | `SearchMemoryRequest` | `SearchMemoryResponse` | - |
| `GetMemoryStats` | `GetMemoryStatsRequest` | `MemoryStats` | - |

## Notes

- Namespaced variant lives in [`namespace.proto`](./namespace.proto.md);
  persona/soul memory in [`soul.proto`](./soul.proto.md).

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
