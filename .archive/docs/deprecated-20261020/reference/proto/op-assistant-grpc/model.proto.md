# `model.proto`

- **Crate:** `op-assistant-grpc`
- **Path:** `crates/op-assistant-grpc/proto/assistant/model.proto`
- **Package:** `assistant.v1`
- **Imports:** `google/protobuf/struct.proto`, `assistant/common.proto`

Model listing and hot-switching.

## Services

### `ModelService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `ListModels` | `ListModelsRequest` | `ListModelsResponse` | - |
| `GetModel` | `GetModelRequest` | `Model` | - |
| `SwitchModel` | `SwitchModelRequest` | `Model` | - |

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
