# `cognitive.proto`

- **Crate:** `op-cognitive`
- **Path:** `crates/op-cognitive/proto/cognitive.proto`
- **Package:** `operation.cognitive.v1`

The retained `CognitiveToolService` protocol contract exposes notebook/source
management, query and diagnostics tools. Cognitive runs in process in
`op-grpc-bridge`; the public MCP door is `https://<host>:8090/mcp`.
The package and service names above remain protocol names, not crate aliases.

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

- **Gateway rule (settled):** external clients use the authenticated bridge door,
  never a standalone cognitive service, compact-mode listener or `op-assistant-grpc`.
- The cognitive/embedding projection boundary is documented in the architecture overview.

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
