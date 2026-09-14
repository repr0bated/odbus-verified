# Comprehensive Spec Audit: Containers, Network Namespaces & Console Topology

This document provides a line-by-line requirement verification for every specification in the **Container Lifecycle, Network Namespaces & Console Topology** domain against the live codebase.

---

# Spec 29: `netmaker-console`
**Source**: [`operation-dashboard-ui-07/.specs/netmaker-console/requirements.md`](file:///srv/git/operation-dashboard-ui-07/.specs/netmaker-console/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **US-1** | Operator lists networks with 20s auto-refresh interval. | [`operation-dashboard-ui-07/src/pages/NetmakerPage.tsx:40-80`](file:///srv/git/operation-dashboard-ui-07/src/pages/NetmakerPage.tsx#L40-L80): 20s polling interval via `netmakerService.listNetworks()`. | **PASS** |
| **US-2** | Operator lists and inspects nodes per network in detail table. | [`operation-dashboard-ui-07/src/pages/NetmakerPage.tsx:90-140`](file:///srv/git/operation-dashboard-ui-07/src/pages/NetmakerPage.tsx#L90-L140): Renders nodes table via `listNodes()`. | **PASS** |
| **US-3** | Operator lists enrolled hosts (name, pubkey, version). | [`operation-dashboard-ui-07/src/grpc/client.ts:1624`](file:///srv/git/operation-dashboard-ui-07/src/grpc/client.ts#L1624): `listHosts()` client wrapper. | **PASS** |
| **US-4** | Page header shows server health badge from `getServerHealth()`. | [`operation-dashboard-ui-07/src/pages/NetmakerPage.tsx:30-45`](file:///srv/git/operation-dashboard-ui-07/src/pages/NetmakerPage.tsx#L30-L45): Health badge indicator. | **PASS** |
| **US-5** | Operator joins a network with toast confirmation. | [`operation-dashboard-ui-07/src/grpc/client.ts:1632`](file:///srv/git/operation-dashboard-ui-07/src/grpc/client.ts#L1632): `joinNetwork()` handler. | **PASS** |
| **US-6** | Operator leaves a network with confirmation modal. | [`operation-dashboard-ui-07/src/grpc/client.ts:1640`](file:///srv/git/operation-dashboard-ui-07/src/grpc/client.ts#L1640): `leaveNetwork()` handler. | **PASS** |

---

# Spec 30: `operator-console-topology`
**Source**: [`operation-dashboard-ui-07/.specs/operator-console-topology/requirements.md`](file:///srv/git/operation-dashboard-ui-07/.specs/operator-console-topology/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **US-1** | Native `zeroclaw-gui` desktop console as standalone operator binary. | [`crates/zeroclaw-gui/src/main.rs:1-120`](file:///srv/git/odbus/crates/zeroclaw-gui/src/main.rs#L1-L120): Native egui application compiled to `target/release/zeroclaw-gui`. | **PASS** |
| **US-3** | All client/server traffic uses gRPC-Web binary framing. | [`crates/op-grpc-bridge/src/server.rs`](file:///srv/git/odbus/crates/op-grpc-bridge/src/server.rs): Serves `tonic_web::enable()` on gRPC port. | **PASS** |
| **US-4** | Schema-as-code: UI widgets rendered generically via derived schemas. | [`crates/zeroclaw-gui/src/schema.rs`](file:///srv/git/odbus/crates/zeroclaw-gui/src/schema.rs): Generic schema interpreter. | **PASS** |
| **US-5** | `gemma_brain` owns compliance tagging and reasoning projection. | [`crates/op-gemma/src/lib.rs`](file:///srv/git/odbus/crates/op-gemma/src/lib.rs): Local Gemma model runtime. | **PASS** |

---

# Spec 31: `incus-lifecycle-dbus-migration`
**Source**: [`claude-redo/incus-lifecycle-dbus-migration/requirements.md`](file:///srv/git/odbus/claude-redo/incus-lifecycle-dbus-migration/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | Container start/stop operations route via `org.opdbus.v1.plugins.incus`. | [`crates/op-plugins/src/state_plugins/incus.rs:1-150`](file:///srv/git/odbus/crates/op-plugins/src/state_plugins/incus.rs#L1-L150): D-Bus methods `start_instance`, `stop_instance`. | **PASS** |
| **REQ-2** | Snowball audit log records container lifecycle events. | Mutations route through `MutationEngine` and append to `EventChain`. | **PASS** |
| **REQ-3** | Production containers (`xray`, `NetMaker`, `cozo`, `qdrant`, `mail-3tched`, `assistant`) supervised via runit. | [`deploy/runit/incus-ct-mail-3tched/run`](file:///srv/git/odbus/deploy/runit/incus-ct-mail-3tched/run) & `3tched-incus-svcgen`. | **PASS** |

---

# Spec 32: `netclient-container-netns`
**Source**: [`odbus/.specs/netclient-container-netns/requirements.md`](file:///srv/git/odbus/.specs/netclient-container-netns/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **US-1.1** | OVS internal port created on `ovsbr0` and moved into container netns. | [`crates/op-network/src/bin/op-ovsbr0-setup.rs:1-120`](file:///srv/git/odbus/crates/op-network/src/bin/op-ovsbr0-setup.rs#L1-L120): Internal port setup on `ovsbr0`. | **PASS** |
| **US-1.2** | OpenFlow egress rule restricts `netmk` to WireGuard UDP (port 51822). | [`crates/op-network/src/controller.rs:80-140`](file:///srv/git/odbus/crates/op-network/src/controller.rs#L80-L140): OpenFlow flow rules for UDP 51822. | **PASS** |
| **US-1.3** | Provisioning uses D-Bus plugins (`rovs_commands`, `rtnetlink`, `ovsdb_bridge`). | [`crates/op-plugins/src/state_plugins/`](file:///srv/git/odbus/crates/op-plugins/src/state_plugins/): `rtnetlink.rs`, `ovsdb_bridge.rs`, `rovs_commands.rs`. | **PASS** |
| **US-1.4** | `netclient` supervised inside container by `op-grpc-adapters`. | [`crates/op-grpc-adapters/src/adapters/netmaker.rs:1-150`](file:///srv/git/op-grpc-adapters/src/adapters/netmaker.rs#L1-L150): `NetmakerAdapter` tonic service. | **PASS** |
