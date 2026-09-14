# `cognitive.proto`

- **Crate:** `op-cognitive-mcp`
- **Path:** `crates/op-cognitive-mcp/proto/cognitive.proto`
- **Package:** `operation.cognitive.v1`

The cognitive MCP gateway contract. `cognitive-mcp` (`:3003`, Netmaker WireGuard IP
`100.90.37.254`) is the universal gateway for all external clients (NotebookLM, Droid,
Cursor, Codex, Junie, Gemini CLI). Exposes notebook/source management, query, and
diagnostics tools.

## Services

### `CognitiveToolService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `AskQuestion` | `AskQuestionRequest` | `AskQuestionResponse` | - |
| `QueryNotebook` | `QueryNotebookRequest` | `QueryNotebookResponse` | - |
| `ListNotebooks` | `ListNotebooksRequest` | `ListNotebooksResponse` | - |
| `GetNotebook` | `GetNotebookRequest` | `GetNotebookResponse` | - |
| `CreateNotebook` | `CreateNotebookRequest` | `CreateNotebookResponse` | - |
| `BatchCreateNotebooks` | `BatchCreateNotebooksRequest` | `BatchCreateNotebooksResponse` | - |
| `AddSource` | `AddSourceRequest` | `AddSourceResponse` | - |
| `AddFolder` | `AddFolderRequest` | `AddFolderResponse` | - |
| `ListSources` | `ListSourcesRequest` | `ListSourcesResponse` | - |
| `GetSourceContent` | `GetSourceContentRequest` | `GetSourceContentResponse` | - |
| `RemoveSource` | `RemoveSourceRequest` | `RemoveSourceResponse` | - |
| `GenerateDataTable` | `GenerateDataTableRequest` | `GenerateDataTableResponse` | - |
| `GeminiQuery` | `GeminiQueryRequest` | `GeminiQueryResponse` | - |
| `GetToolProfile` | `GetToolProfileRequest` | `GetToolProfileResponse` | - |
| `Doctor` | `DoctorRequest` | `DoctorResponse` | - |
| `GetQueryHistory` | `GetQueryHistoryRequest` | `GetQueryHistoryResponse` | - |
| `GetHealth` | `GetHealthRequest` | `GetHealthResponse` | - |
| `SetupAuth` | `SetupAuthRequest` | `SetupAuthResponse` | - |

## Notes

- **Gateway rule (settled):** point external clients here, never at `compact-mcp`
  (`127.0.0.1:11436`, loopback/chatbot only) and never at `op-assistant-grpc` directly.
- The cognitive/embedding projection boundary is documented in the architecture overview.

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
