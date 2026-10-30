# `internal_agents.proto`

- **Crate:** `op-mcp`
- **Path:** `crates/op-mcp/proto/internal_agents.proto`
- **Package:** `op_agents`

Internal agent RPC surface used by the MCP crate. Mirrors the orchestration split
(lifecycle, execution, memory, sequential thinking, context, Rust tooling) at a smaller
scope than [`op-chat/orchestration.proto`](../op-chat/orchestration.proto.md).

## Services

### `AgentLifecycle`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Start` | `StartRequest` | `StartResponse` | - |
| `Stop` | `StopRequest` | `StopResponse` | - |
| `Health` | `HealthRequest` | `HealthResponse` | - |
| `WatchStatus` | `WatchRequest` | `AgentStatus` | server |

### `AgentExecution`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Execute` | `ExecuteRequest` | `ExecuteResponse` | - |
| `BatchExecute` | `ExecuteRequest` | `ExecuteResponse` | bidi |
| `StreamExecute` | `ExecuteRequest` | `ExecuteChunk` | server |

### `MemoryService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Remember` | `RememberRequest` | `RememberResponse` | - |
| `Recall` | `RecallRequest` | `RecallResponse` | - |
| `Forget` | `ForgetRequest` | `ForgetResponse` | - |
| `List` | `ListRequest` | `ListResponse` | - |
| `Search` | `SearchRequest` | `SearchResponse` | - |
| `BulkRemember` | `RememberRequest` | `BulkResponse` | client |

### `SequentialThinkingService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `StartChain` | `ChainRequest` | `ChainResponse` | - |
| `AddThought` | `ThoughtRequest` | `ThoughtResponse` | - |
| `StreamThinking` | `ChainRequest` | `ThoughtResponse` | server |
| `Conclude` | `ConcludeRequest` | `ConcludeResponse` | - |

### `ContextManagerService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Save` | `SaveContextRequest` | `SaveContextResponse` | - |
| `Load` | `LoadContextRequest` | `LoadContextResponse` | - |
| `List` | `ListContextRequest` | `ListContextResponse` | - |
| `Delete` | `DeleteContextRequest` | `DeleteContextResponse` | - |
| `Export` | `ExportRequest` | `ExportChunk` | server |
| `Import` | `ImportChunk` | `ImportResponse` | client |

### `RustProService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Check` | `CargoRequest` | `CargoResponse` | - |
| `Build` | `CargoRequest` | `CargoOutput` | server |
| `Test` | `CargoRequest` | `CargoOutput` | server |
| `Clippy` | `CargoRequest` | `CargoOutput` | server |
| `Format` | `CargoRequest` | `CargoResponse` | - |
| `Doc` | `CargoRequest` | `CargoOutput` | server |

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
