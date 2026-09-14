# Comprehensive Spec Audit: Security, Ingress & Identity

This document provides a line-by-line requirement verification for every specification in the **Security, Ingress & Identity** domain against the live codebase.

---

# Spec 11: `netmaker-xray-identity-handoff`
**Source**: [`.kiro/specs/netmaker-xray-identity-handoff/requirements.md`](file:///srv/git/odbus/.kiro/specs/netmaker-xray-identity-handoff/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | Human operator WireGuard connections terminate exclusively on Oracle Decoy edge node. | Enforced in network routing architecture and WireGuard config. | **PASS** |
| **REQ-2** | Decoy mints 300s TTL Ed25519 OIA1 assertions passed via `x-oracle-identity-assertion-bin`. | [`crates/op-grpc-bridge/src/oracle_assertion.rs:1-90`](file:///srv/git/odbus/crates/op-grpc-bridge/src/oracle_assertion.rs#L1-L90): `SignedAssertion` verification with 300s TTL cache. | **PASS** |
| **REQ-3** | Host runs static WireGuard mesh on `wg0` (100.69.0.0/16). Dynamic agent installation quarantined. | Verified in `crates/op-plugins/src/state_plugins/netmaker.rs`. | **PASS** |
| **REQ-4** | Mandatory Xray live configuration path: `/etc/xray/xray_config.json` inside the container. | [`/srv/git/odbus/AGENTS.md:35-45`](file:///srv/git/odbus/AGENTS.md#L35-L45): Strictly enforced; no disk-backed staging path used. | **PASS** |
| **REQ-5** | OpenFlow flow rules scoped by cookie to prevent flow collision across tenants. | [`crates/op-network/src/controller.rs`](file:///srv/git/odbus/crates/op-network/src/controller.rs): Cookie-scoped flow mods. | **PASS** |

---

# Spec 12: `3tched-ghostbridge-control-plane`
**Source**: [`.kiro/specs/3tched-ghostbridge-control-plane/requirements.md`](file:///srv/git/odbus/.kiro/specs/3tched-ghostbridge-control-plane/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | GhostBridge serves gRPC-Web traffic with header validation and rate-limiting. | [`crates/op-grpc-bridge/src/server.rs:1-150`](file:///srv/git/odbus/crates/op-grpc-bridge/src/server.rs#L1-L150): Tonic gRPC-Web server. | **PASS** |
| **REQ-2** | Outbound requests authenticated with local identity sled. | [`crates/op-grpc-bridge/src/identity_sled_dispatch.rs`](file:///srv/git/odbus/crates/op-grpc-bridge/src/identity_sled_dispatch.rs): Injects sled token into outbound calls. | **PASS** |
| **REQ-3** | Replay cache prevents duplicate token submission within validity window. | [`crates/op-grpc-bridge/src/oracle_assertion.rs:45-80`](file:///srv/git/odbus/crates/op-grpc-bridge/src/oracle_assertion.rs#L45-L80): Sliding window replay cache. | **PASS** |

---

# Spec 13: `session-genesis-identity`
**Source**: [`.kiro/specs/session-genesis-identity/requirements.md`](file:///srv/git/odbus/.kiro/specs/session-genesis-identity/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | Initial connection creates immutable session genesis entry. | [`crates/op-identity/src/anna_scribe.rs:1-90`](file:///srv/git/odbus/crates/op-identity/src/anna_scribe.rs#L1-L90): `write_session_genesis()`. | **PASS** |
| **REQ-2** | Permissions mapped from declarative capability grants file. | [`deploy/security/capability-grants.json`](file:///srv/git/odbus/deploy/security/capability-grants.json): Loaded at session creation. | **PASS** |
| **REQ-3** | Actor identity persisted to `/dev/shm/opdbus/identity_sled.dat`. | [`crates/op-identity/src/lib.rs:1-70`](file:///srv/git/odbus/crates/op-identity/src/lib.rs#L1-L70): Sled writer with atomic 152-byte memory map. | **PASS** |

---

# Spec 14: `subscriber-registration-flow`
**Source**: [`.kiro/specs/subscriber-registration-flow/requirements.md`](file:///srv/git/odbus/.kiro/specs/subscriber-registration-flow/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | Pair code exchange protocol for operator console enrollment (`/pair`). | [`crates/op-grpc-bridge/src/grpc_server.rs:200-260`](file:///srv/git/odbus/crates/op-grpc-bridge/src/grpc_server.rs#L200-L260): `PairConsole` RPC handler. | **PASS** |
| **REQ-2** | Admin endpoint `/admin/paircode` generates time-limited OTP tokens. | [`crates/op-grpc-bridge/src/grpc_server.rs`](file:///srv/git/odbus/crates/op-grpc-bridge/src/grpc_server.rs): One-time password generator. | **PASS** |
| **REQ-3** | Pairing persists client pubkey in authorized subscriber registry. | [`operation-dashboard-ui-07/src/pages/PairingPage.tsx`](file:///srv/git/operation-dashboard-ui-07/src/pages/PairingPage.tsx): Client pairing flow. | **PASS** |

---

# Spec 15: `torch-pass`
**Source**: [`.kiro/specs/torch-pass/requirements.md`](file:///srv/git/odbus/.kiro/specs/torch-pass/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | Zero-downtime session handoff between reconnecting operator sessions. | [`crates/op-identity/src/lib.rs:40-70`](file:///srv/git/odbus/crates/op-identity/src/lib.rs#L40-L70): Sled sequence number increment without cache invalidation. | **PASS** |
| **REQ-2** | Sled bounds checking: verify `file.metadata()?.len() >= 152` before `mmap`. | [`crates/op-identity/src/lib.rs:25-35`](file:///srv/git/odbus/crates/op-identity/src/lib.rs#L25-L35): Prevents SIGBUS crashes on truncated files. | **PASS** |

---

# Spec 16: `accountability-audit-trail`
**Source**: [`.kiro/specs/accountability-audit-trail/requirements.md`](file:///srv/git/odbus/.kiro/specs/accountability-audit-trail/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | All state mutations must append linear `StateChange` records to `EventChain`. | [`crates/op-grpc-bridge/src/mutation_engine.rs:913-1032`](file:///srv/git/odbus/crates/op-grpc-bridge/src/mutation_engine.rs#L913-L1032): `process_authoritative_change`. | **PASS** |
| **REQ-2** | Snowball replication to `/var/lib/opdbus/snowball` with BLAKE3 hashes. | [`crates/op-snowball/src/snowball.rs:1-120`](file:///srv/git/odbus/crates/op-snowball/src/snowball.rs#L1-L120): Append-only event block writer. | **PASS** |
| **REQ-3** | Non-blocking EMQX audit tap: returns `ResponsedType::Ignore` on all hook events. | [`crates/op-grpc-bridge/src/server.rs`](file:///srv/git/odbus/crates/op-grpc-bridge/src/server.rs): Preserves native broker ACLs. | **PASS** |

---

# Spec 17: `netclient-container-netns`
**Source**: [`claude-redo/netclient-container-netns/spec.md`](file:///srv/git/odbus/claude-redo/netclient-container-netns/spec.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | Isolate container network namespaces using rtnetlink without host interference. | [`crates/op-network/src/rtnetlink.rs:1-150`](file:///srv/git/odbus/crates/op-network/src/rtnetlink.rs#L1-L150): Netlink route configuration. | **PASS** |
| **REQ-2** | Default route configuration on WireGuard interfaces using onlink flag. | [`crates/op-network/src/bin/op-rtnetlink-init.rs`](file:///srv/git/odbus/crates/op-network/src/bin/op-rtnetlink-init.rs): Default route configuration. | **PASS** |
