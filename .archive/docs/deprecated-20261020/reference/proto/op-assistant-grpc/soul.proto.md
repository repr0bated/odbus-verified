# `soul.proto`

- **Crate:** `op-assistant-grpc`
- **Path:** `crates/op-assistant-grpc/proto/assistant/soul.proto`
- **Package:** `assistant.v1`
- **Imports:** `google/protobuf/{timestamp,struct}.proto`, `assistant/common.proto`

Persona/"soul" memory: durable identity-level memories for the assistant.

## Services

### `SoulService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `GetSoulMemory` | `GetSoulMemoryRequest` | `SoulMemory` | - |
| `UpdateSoulMemory` | `UpdateSoulMemoryRequest` | `SoulMemory` | - |
| `DeleteSoulMemory` | `DeleteSoulMemoryRequest` | `Empty` | - |
| `ListSoulMemories` | `ListSoulMemoriesRequest` | `ListSoulMemoriesResponse` | - |

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
