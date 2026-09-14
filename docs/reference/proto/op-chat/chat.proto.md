# `chat.proto`

- **Crate:** `op-chat`
- **Path:** `crates/op-chat/proto/chat.proto`
- **Package:** `op_chat.chat`

Front-facing chat service: send a prompt, stream frames back, approve/cancel in-flight
actions (human-in-the-loop tool gating).

## Services

### `ChatService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Send` | `SendRequest` | `ChatFrame` | server |
| `Approve` | `ApproveRequest` | `ApproveResponse` | - |
| `Cancel` | `CancelRequest` | `CancelResponse` | - |

## Notes

- `Send` streams `ChatFrame`s (tokens, tool calls, approvals-required, completion).
- `Approve`/`Cancel` gate tool execution requested mid-stream.

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
