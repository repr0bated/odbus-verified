# `orchestration.proto`

- **Crate:** `op-chat`
- **Path:** `crates/op-chat/proto/orchestration.proto`
- **Package:** `op_chat.orchestration`

The full multi-agent orchestration contract. Splits agent concerns into lifecycle,
execution, memory, sequential thinking, context management, Rust tooling, backend
architecture, and workstack services. Supersedes the simpler
[`agents.proto`](./agents.proto.md) surface.

## Services

### `AgentLifecycle`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `StartSession` | `StartSessionRequest` | `StartSessionResponse` | - |
| `EndSession` | `EndSessionRequest` | `EndSessionResponse` | - |
| `HealthCheck` | `HealthCheckRequest` | `HealthCheckResponse` | - |
| `WatchAgents` | `WatchAgentsRequest` | `AgentStatusEvent` | server |
| `Shutdown` | `ShutdownRequest` | `ShutdownResponse` | - |

### `AgentExecution`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Execute` | `ExecuteRequest` | `ExecuteResponse` | - |
| `ExecuteStream` | `ExecuteRequest` | `ExecuteChunk` | server |
| `BatchExecute` | `BatchExecuteRequest` | `ExecuteResponse` | server |
| `Cancel` | `CancelRequest` | `CancelResponse` | - |

### `MemoryService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Remember` | `RememberRequest` | `RememberResponse` | - |
| `Recall` | `RecallRequest` | `RecallResponse` | - |
| `Forget` | `ForgetRequest` | `ForgetResponse` | - |
| `List` | `ListKeysRequest` | `ListKeysResponse` | - |
| `Search` | `SearchMemoryRequest` | `SearchMemoryResponse` | - |
| `BulkRemember` | `RememberRequest` | `BulkOperationResponse` | client |
| `BulkRecall` | `BulkRecallRequest` | `RecallResponse` | server |
| `BulkForget` | `BulkForgetRequest` | `BulkOperationResponse` | - |

### `SequentialThinkingService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `StartChain` | `StartChainRequest` | `StartChainResponse` | - |
| `AddThought` | `AddThoughtRequest` | `AddThoughtResponse` | - |
| `ThinkStream` | `StartChainRequest` | `ThoughtEvent` | server |
| `Conclude` | `ConcludeRequest` | `ConcludeResponse` | - |
| `GetChain` | `GetChainRequest` | `GetChainResponse` | - |
| `ForkChain` | `ForkChainRequest` | `ForkChainResponse` | - |

### `ContextManagerService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Save` | `SaveContextRequest` | `SaveContextResponse` | - |
| `Load` | `LoadContextRequest` | `LoadContextResponse` | - |
| `List` | `ListContextsRequest` | `ListContextsResponse` | - |
| `Delete` | `DeleteContextRequest` | `DeleteContextResponse` | - |
| `Export` | `ExportContextRequest` | `ExportChunk` | server |
| `Import` | `ImportChunk` | `ImportContextResponse` | client |
| `Merge` | `MergeContextsRequest` | `MergeContextsResponse` | - |

### `RustProService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Check` | `CargoRequest` | `CargoResponse` | - |
| `Fmt` | `CargoRequest` | `CargoResponse` | - |
| `Version` | `VersionRequest` | `VersionResponse` | - |
| `Build` | `CargoRequest` | `CargoOutputLine` | server |
| `Test` | `CargoRequest` | `CargoOutputLine` | server |
| `Clippy` | `CargoRequest` | `CargoOutputLine` | server |
| `Run` | `CargoRequest` | `CargoOutputLine` | server |
| `Doc` | `CargoRequest` | `CargoOutputLine` | server |
| `Bench` | `CargoRequest` | `CargoOutputLine` | server |
| `Analyze` | `AnalyzeRequest` | `AnalyzeResponse` | - |

### `BackendArchitectService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Analyze` | `ArchitectAnalyzeRequest` | `ArchitectAnalyzeResponse` | - |
| `Design` | `ArchitectDesignRequest` | `ArchitectDesignResponse` | - |
| `Review` | `ArchitectReviewRequest` | `ArchitectReviewResponse` | - |
| `Suggest` | `ArchitectSuggestRequest` | `ArchitectSuggestResponse` | - |
| `Document` | `ArchitectDocumentRequest` | `DocumentChunk` | server |

### `WorkstackService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Execute` | `WorkstackExecuteRequest` | `WorkstackEvent` | server |
| `GetStatus` | `WorkstackStatusRequest` | `WorkstackStatusResponse` | - |
| `Cancel` | `WorkstackCancelRequest` | `WorkstackCancelResponse` | - |
| `Rollback` | `WorkstackRollbackRequest` | `WorkstackRollbackResponse` | - |
| `List` | `ListWorkstacksRequest` | `ListWorkstacksResponse` | - |

## Notes

- The workstack concept is shared with `op-cache` `OrchestratorService`/`CacheService`
  (see [`op_cache.proto`](../op-cache/op_cache.proto.md)).

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
