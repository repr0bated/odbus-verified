# Spec 14: `subscriber-registration-flow`

**Spec Path**: [`.kiro/specs/subscriber-registration-flow/requirements.md`](file:///srv/git/odbus/.kiro/specs/subscriber-registration-flow/requirements.md)  
**Domain**: Operator Console Enrollment & Pairing Flow  
**Status**: **PASS (Verified)**

---

## Requirements Verification Matrix

| Requirement | Requirement Statement | Code Implementation | Status |
|---|---|---|:---:|
| **REQ-1** | Pair code exchange protocol for operator console enrollment (`/pair`). | [`crates/op-grpc-bridge/src/grpc_server.rs:200-260`](file:///srv/git/odbus/crates/op-grpc-bridge/src/grpc_server.rs#L200-L260): `PairConsole` RPC handler. | **PASS** |
| **REQ-2** | Admin endpoint `/admin/paircode` generates time-limited one-time passwords (OTP). | [`crates/op-grpc-bridge/src/grpc_server.rs`](file:///srv/git/odbus/crates/op-grpc-bridge/src/grpc_server.rs): OTP generation logic. | **PASS** |
| **REQ-3** | Pairing persists client pubkey in authorized subscriber registry. | [`operation-dashboard-ui-07/src/pages/PairingPage.tsx`](file:///srv/git/operation-dashboard-ui-07/src/pages/PairingPage.tsx): Client pairing workflow. | **PASS** |
| **REQ-4** | Expired or invalid pair codes reject with explicit status codes. | Handled via Tonic status returns in `grpc_server.rs`. | **PASS** |
