# `privacy_network.proto`

- **Crate:** `op-grpc-bridge`
- **Path:** `crates/op-grpc-bridge/proto/privacy_network.proto`
- **Package:** `operation.privacy.v1`
- **Imports:** `google/protobuf/{timestamp,struct}.proto`

Privacy/WireGuard network provisioning and topology surface. Ensures the privacy
network exists, provisions users, generates key material, and configures packet routing.

## Services

### `PrivacyNetworkService`

| RPC | Request | Response | Stream |
|---|---|---|---|
| `EnsurePrivacyNetwork` | `EnsurePrivacyNetworkRequest` | `EnsurePrivacyNetworkResponse` | - |
| `GetNetworkStatus` | `GetNetworkStatusRequest` | `GetNetworkStatusResponse` | - |
| `ProvisionUser` | `ProvisionUserRequest` | `ProvisionUserResponse` | - |
| `GetPrivacyWireGuardConfig` | `GetPrivacyWireGuardConfigRequest` | `GetPrivacyWireGuardConfigResponse` | - |
| `ManageComponent` | `ManageComponentRequest` | `ManageComponentResponse` | - |
| `GetNetworkTopology` | `GetNetworkTopologyRequest` | `GetNetworkTopologyResponse` | - |
| `HealthCheck` | `HealthCheckRequest` | `HealthCheckResponse` | - |
| `ConfigurePacketRouting` | `ConfigurePacketRoutingRequest` | `ConfigurePacketRoutingResponse` | - |
| `GenerateWireGuardKeyPair` | `GenerateWireGuardKeyPairRequest` | `GenerateWireGuardKeyPairResponse` | - |

## Notes

- Ties into A.N.N.A. Scribe identity notarization and Netmaker WireGuard addressing
  (see cognitive-mcp gateway at `100.90.37.254`).
- Key generation results are sensitive; private keys must never be logged.

## Gaps / Assumptions

- Message field-level shapes are not enumerated here; consult the source for exact fields.
