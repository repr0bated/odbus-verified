# `mail.proto`

- **Crate:** `op-grpc-bridge`
- **Path:** `crates/op-grpc-bridge/proto/mail.proto`
- **Package:** `operation.mail.v1`
- **Imports:** `google/protobuf/{timestamp,struct}.proto`

Mail projection surface exposed by the bridge. Read/send mail and query mail-server
health through the D-Bus mail objects.

## Services

### `MailService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `SendEmail` | `SendEmailRequest` | `SendEmailResponse` | - |
| `GetInbox` | `GetInboxRequest` | `GetInboxResponse` | - |
| `GetMessage` | `GetMessageRequest` | `GetMessageResponse` | - |
| `GetMailStatus` | `GetMailStatusRequest` | `GetMailStatusResponse` | - |
| `ListMailAccounts` | `ListMailAccountsRequest` | `ListMailAccountsResponse` | - |
| `AdminMailAction` | `AdminMailActionRequest` | `AdminMailActionResponse` | - |
| `CheckMailServer` | `CheckMailServerRequest` | `CheckMailServerResponse` | - |

## Notes

- Overlaps functionally with `op-grpc-adapters` [`adapters.proto`](../op-grpc-adapters/adapters.proto.md)
  `MailService` (`op.adapters.v1`). This one is the bridge-level projection;
  the adapters one is the adapter-layer transport. Confirm which the client should target.

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
