# `task.proto`

- **Crate:** `op-assistant-grpc`
- **Path:** `crates/op-assistant-grpc/proto/assistant/task.proto`
- **Package:** `assistant.v1`
- **Imports:** `google/protobuf/{timestamp,struct}.proto`, `assistant/common.proto`

Tool discovery and task execution (with streaming task events).

## Services

### `TaskService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `ListTools` | `ListToolsRequest` | `ListToolsResponse` | - |
| `ExecuteTask` | `ExecuteTaskRequest` | `TaskResult` | - |
| `StreamTaskExecution` | `StreamTaskExecutionRequest` | `TaskEvent` | server |
| `GetTaskResult` | `GetTaskResultRequest` | `TaskResult` | - |

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
