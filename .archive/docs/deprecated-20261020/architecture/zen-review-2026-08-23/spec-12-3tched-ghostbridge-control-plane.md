# Spec 12: `3tched-ghostbridge-control-plane`

**Spec Path**: [`.kiro/specs/3tched-ghostbridge-control-plane/requirements.md`](file:///srv/git/odbus/.kiro/specs/3tched-ghostbridge-control-plane/requirements.md)  
**Domain**: Ingress Gateway & gRPC-Web Proxy  
**Status**: **PASS (Verified)**

---

## Requirements Verification Matrix

| Requirement | Requirement Statement | Code Implementation | Status |
|---|---|---|:---:|
| **REQ-1** | GhostBridge provides gRPC-Web ingress with 5-byte framing and header validation. | [`crates/op-grpc-bridge/src/server.rs:1-150`](file:///srv/git/odbus/crates/op-grpc-bridge/src/server.rs#L1-L150). | **PASS** |
| **REQ-2** | Outbound requests authenticated with local identity sled token. | [`crates/op-grpc-bridge/src/identity_sled_dispatch.rs`](file:///srv/git/odbus/crates/op-grpc-bridge/src/identity_sled_dispatch.rs). | **PASS** |
| **REQ-3** | Replay window cache prevents token replay within 300s TTL. | [`crates/op-grpc-bridge/src/oracle_assertion.rs:45-80`](file:///srv/git/odbus/crates/op-grpc-bridge/src/oracle_assertion.rs#L45-L80). | **PASS** |
| **REQ-4** | Identity sled updates track monotonic sequence numbers across reconnects. | [`crates/op-identity/src/lib.rs`](file:///srv/git/odbus/crates/op-identity/src/lib.rs). | **PASS** |
