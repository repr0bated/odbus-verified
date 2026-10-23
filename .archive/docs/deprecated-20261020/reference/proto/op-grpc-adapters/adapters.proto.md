# `adapters.proto`

- **Crate:** `op-grpc-adapters`
- **Path:** `crates/op-grpc-adapters/proto/adapters.proto`
- **Package:** `op.adapters.v1`

Adapter-layer transports for mail, Netmaker (WireGuard mesh), and a message queue.

## Services

### `MailService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `ListFolders` | `ListFoldersRequest` | `ListFoldersResponse` | - |
| `ListMessages` | `ListMessagesRequest` | `ListMessagesResponse` | - |
| `GetMessage` | `GetMessageRequest` | `GetMessageResponse` | - |
| `SearchMessages` | `SearchMessagesRequest` | `SearchMessagesResponse` | - |
| `MoveMessage` | `MoveMessageRequest` | `MoveMessageResponse` | - |
| `DeleteMessage` | `DeleteMessageRequest` | `DeleteMessageResponse` | - |
| `StreamInbox` | `StreamInboxRequest` | `MailEvent` | server |
| `SendMessage` | `SendMessageRequest` | `SendMessageResponse` | - |

### `NetmakerService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `GetServerHealth` | `HealthRequest` | `HealthResponse` | - |
| `ListNetworks` | `ListNetworksRequest` | `ListNetworksResponse` | - |
| `GetNetwork` | `GetNetworkRequest` | `GetNetworkResponse` | - |
| `ListNodes` | `ListNodesRequest` | `ListNodesResponse` | - |
| `GetNode` | `GetNodeRequest` | `GetNodeResponse` | - |
| `ListHosts` | `ListHostsRequest` | `ListHostsResponse` | - |
| `JoinNetwork` | `JoinNetworkRequest` | `JoinNetworkResponse` | - |
| `LeaveNetwork` | `LeaveNetworkRequest` | `LeaveNetworkResponse` | - |
| `RestartService` | `RestartServiceRequest` | `RestartServiceResponse` | - |
| `RunCommand` | `RunCommandRequest` | `RunCommandResponse` | - |
| `StreamEvents` | `StreamEventsRequest` | `NetmakerEvent` | server |

### `MqService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `Publish` | `PublishRequest` | `PublishResponse` | - |
| `Subscribe` | `SubscribeRequest` | `MqMessage` | server |
| `ListTopics` | `ListTopicsRequest` | `ListTopicsResponse` | - |

## Notes

- `MailService` here is the adapter transport; the bridge-level projection is
  [`op-grpc-bridge/mail.proto`](../op-grpc-bridge/mail.proto.md).
- `NetmakerService` underpins the privacy-network/WireGuard flows.

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
