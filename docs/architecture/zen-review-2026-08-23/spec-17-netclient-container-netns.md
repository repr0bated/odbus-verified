# Spec 17: `netclient-container-netns`

**Spec Path**: [`claude-redo/netclient-container-netns/spec.md`](file:///srv/git/odbus/claude-redo/netclient-container-netns/spec.md)  
**Domain**: Container Network Namespaces & WireGuard Routing  
**Status**: **PASS (Verified)**

---

## Requirements Verification Matrix

| Requirement | Requirement Statement | Code Implementation | Status |
|---|---|---|:---:|
| **REQ-1** | Container network namespace isolation managed via rtnetlink without host interference. | [`crates/op-network/src/rtnetlink.rs:1-150`](file:///srv/git/odbus/crates/op-network/src/rtnetlink.rs#L1-L150). | **PASS** |
| **REQ-2** | Default route configuration on WireGuard interfaces using onlink flag. | [`crates/op-network/src/bin/op-rtnetlink-init.rs`](file:///srv/git/odbus/crates/op-network/src/bin/op-rtnetlink-init.rs). | **PASS** |
| **REQ-3** | OpenFlow rules restrict mesh traffic to WireGuard UDP (port 51822). | [`crates/op-network/src/controller.rs:80-140`](file:///srv/git/odbus/crates/op-network/src/controller.rs#L80-L140). | **PASS** |
| **REQ-4** | In-container process supervision orchestrated through `op-grpc-adapters`. | [`crates/op-grpc-adapters/src/adapters/netmaker.rs:1-150`](file:///srv/git/op-grpc-adapters/src/adapters/netmaker.rs#L1-L150). | **PASS** |
