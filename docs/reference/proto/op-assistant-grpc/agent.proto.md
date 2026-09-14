# `agent.proto`

- **Crate:** `op-assistant-grpc`
- **Path:** `crates/op-assistant-grpc/proto/assistant/agent.proto`
- **Package:** `assistant.v1`
- **Imports:** `google/protobuf/{timestamp,struct}.proto`, `assistant/common.proto`

Assistant agent management and run execution.

## Services

### `AgentService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `ListAgents` | `ListAgentsRequest` | `ListAgentsResponse` | - |
| `GetAgent` | `GetAgentRequest` | `Agent` | - |
| `CreateAgent` | `CreateAgentRequest` | `Agent` | - |
| `UpdateAgent` | `UpdateAgentRequest` | `Agent` | - |
| `DeleteAgent` | `DeleteAgentRequest` | `Empty` | - |
| `StartRun` | `StartRunRequest` | `Run` | - |
| `StreamRunEvents` | `StreamRunEventsRequest` | `RunEvent` | server |
| `CancelRun` | `CancelRunRequest` | `Empty` | - |

## Notes

- Shared messages (`Empty`, `Agent`, `Run`, etc.) come from
  [`common.proto`](./common.proto.md).
- `op-assistant-grpc` is loopback/internal; external clients use `cognitive-mcp`.

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
