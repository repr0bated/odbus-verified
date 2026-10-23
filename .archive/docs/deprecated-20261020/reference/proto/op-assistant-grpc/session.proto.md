# `session.proto`

- **Crate:** `op-assistant-grpc`
- **Path:** `crates/op-assistant-grpc/proto/assistant/session.proto`
- **Package:** `assistant.v1`
- **Imports:** `google/protobuf/{timestamp,struct}.proto`, `assistant/common.proto`

Conversation session lifecycle, history, and messaging.

## Services

### `SessionService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `ListSessions` | `ListSessionsRequest` | `ListSessionsResponse` | - |
| `GetSession` | `GetSessionRequest` | `Session` | - |
| `CreateSession` | `CreateSessionRequest` | `Session` | - |
| `DeleteSession` | `DeleteSessionRequest` | `Empty` | - |
| `GetSessionHistory` | `GetSessionHistoryRequest` | `SessionHistory` | - |
| `SendMessage` | `SendMessageRequest` | `Message` | - |

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
