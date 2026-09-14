# `registration.proto`

- **Crate:** `op-grpc-bridge`
- **Path:** `crates/op-grpc-bridge/proto/registration.proto`
- **Package:** `operation.registration.v1`
- **Imports:** `google/protobuf/{timestamp,struct}.proto`

User onboarding and registration. Magic-link auth, user provisioning, status queries,
and WireGuard config issuance.

## Services

### `RegistrationService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `SendMagicLink` | `SendMagicLinkRequest` | `SendMagicLinkResponse` | - |
| `VerifyMagicLink` | `VerifyMagicLinkRequest` | `VerifyMagicLinkResponse` | - |
| `RegisterUser` | `RegisterUserRequest` | `RegisterUserResponse` | - |
| `GetUserStatus` | `GetUserStatusRequest` | `GetUserStatusResponse` | - |
| `ListUsers` | `ListUsersRequest` | `ListUsersResponse` | - |
| `GetWireGuardConfig` | `GetWireGuardConfigRequest` | `GetWireGuardConfigResponse` | - |
| `AdminUserAction` | `AdminUserActionRequest` | `AdminUserActionResponse` | - |

## Notes

- Complements [`privacy_network.proto`](./privacy_network.proto.md): registration issues
  identity/config, privacy network wires the runtime tunnel.
- Magic-link tokens and issued WireGuard configs are sensitive material.

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
