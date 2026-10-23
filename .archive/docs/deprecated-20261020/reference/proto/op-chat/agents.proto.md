# `agents.proto`

- **Crate:** `op-chat`
- **Path:** `crates/op-chat/proto/agents.proto`
- **Package:** `op_chat.agents`

Per-agent service surface for the chat runtime. Each specialized agent exposes its own
service; the generic `AgentService` handles sessions and execution.

## Services

### `AgentService`
Session lifecycle and execution.

| RPC | Request | Response | Stream |
|---|---|---|---|
| `StartSession` | `StartSessionRequest` | `StartSessionResponse` | - |
| `EndSession` | `EndSessionRequest` | `EndSessionResponse` | - |
| `Execute` | `ExecuteRequest` | `ExecuteResponse` | - |
| `ExecuteStream` | `ExecuteRequest` | `ExecuteChunk` | server |
| `BatchExecute` | `BatchExecuteRequest` | `ExecuteResponse` | server |
| `Session` | `SessionMessage` | `SessionMessage` | bidi |

### `MemoryAgent`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Remember` | `RememberRequest` | `RememberResponse` | - |
| `Recall` | `RecallRequest` | `RecallResponse` | - |
| `Forget` | `ForgetRequest` | `ForgetResponse` | - |
| `List` | `ListRequest` | `ListResponse` | - |
| `Search` | `SearchRequest` | `SearchResponse` | - |
| `BulkRemember` | `RememberRequest` | `BulkResponse` | client |
| `BulkRecall` | `BulkRecallRequest` | `RecallResponse` | server |

### `SequentialThinkingAgent`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `StartChain` | `StartChainRequest` | `StartChainResponse` | - |
| `Think` | `ThinkRequest` | `ThinkResponse` | - |
| `ThinkStream` | `StartChainRequest` | `ThinkResponse` | server |
| `Conclude` | `ConcludeRequest` | `ConcludeResponse` | - |
| `GetChain` | `GetChainRequest` | `GetChainResponse` | - |

### `ContextManagerAgent`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Save` | `SaveContextRequest` | `SaveContextResponse` | - |
| `Load` | `LoadContextRequest` | `LoadContextResponse` | - |
| `List` | `ListContextsRequest` | `ListContextsResponse` | - |
| `Delete` | `DeleteContextRequest` | `DeleteContextResponse` | - |
| `Export` | `ExportRequest` | `ExportChunk` | server |
| `Import` | `ImportChunk` | `ImportResponse` | client |

### `RustProAgent`
Cargo tooling agent.

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Check` | `CargoRequest` | `CargoResponse` | - |
| `Fmt` | `CargoRequest` | `CargoResponse` | - |
| `Build` | `CargoRequest` | `CargoOutput` | server |
| `Test` | `CargoRequest` | `CargoOutput` | server |
| `Clippy` | `CargoRequest` | `CargoOutput` | server |
| `Run` | `CargoRequest` | `CargoOutput` | server |
| `Doc` | `CargoRequest` | `CargoOutput` | server |

### `BackendArchitectAgent`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Analyze` | `AnalyzeRequest` | `AnalyzeResponse` | - |
| `Design` | `DesignRequest` | `DesignResponse` | - |
| `Review` | `ReviewRequest` | `ReviewResponse` | - |
| `Suggest` | `SuggestRequest` | `SuggestResponse` | - |
| `Document` | `DocumentRequest` | `DocumentChunk` | server |

## Notes

- The richer orchestration-oriented equivalents live in
  [`orchestration.proto`](./orchestration.proto.md) (package `op_chat.orchestration`),
  which supersede several of these agent services with lifecycle + execution splits.

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
