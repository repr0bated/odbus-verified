# Spec 11: `netmaker-xray-identity-handoff`

**Spec Path**: [`.kiro/specs/netmaker-xray-identity-handoff/requirements.md`](file:///srv/git/odbus/.kiro/specs/netmaker-xray-identity-handoff/requirements.md)  
**Domain**: Ingress, Identity Tokens & Xray Boundary  
**Status**: **PASS (Verified)**

---

## Requirements Verification Matrix

| Requirement | Requirement Statement | Code Implementation | Status |
|---|---|---|:---:|
| **REQ-1** | Human operator WireGuard connections terminate exclusively on Oracle Decoy edge node. | Enforced in WireGuard routing architecture. | **PASS** |
| **REQ-2** | Decoy mints 300s TTL Ed25519 OIA1 assertions passed via `x-oracle-identity-assertion-bin`. | [`crates/op-grpc-bridge/src/oracle_assertion.rs:1-90`](file:///srv/git/odbus/crates/op-grpc-bridge/src/oracle_assertion.rs#L1-L90): `SignedAssertion` verification. | **PASS** |
| **REQ-3** | Host runs static WireGuard mesh on `wg0` (100.69.0.0/16); dynamic agent installs quarantined. | [`crates/op-plugins/src/state_plugins/netmaker.rs`](file:///srv/git/odbus/crates/op-plugins/src/state_plugins/netmaker.rs). | **PASS** |
| **REQ-4** | Mandatory Xray live configuration path: `/etc/xray/xray_config.json` inside the container. | [`/srv/git/odbus/AGENTS.md:35-45`](file:///srv/git/odbus/AGENTS.md#L35-L45): Strictly enforced. | **PASS** |
| **REQ-5** | OpenFlow flow rules scoped by cookie to prevent flow collision across tenants. | [`crates/op-network/src/controller.rs`](file:///srv/git/odbus/crates/op-network/src/controller.rs). | **PASS** |
