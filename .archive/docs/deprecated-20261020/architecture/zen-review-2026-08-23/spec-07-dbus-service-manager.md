# Spec 07: `dbus-service-manager`

**Spec Path**: [`.kiro/specs/dbus-service-manager/requirements.md`](file:///srv/git/odbus/.kiro/specs/dbus-service-manager/requirements.md)  
**Domain**: Container Service Management over D-Bus  
**Status**: **PASS (Verified)**

---

## Requirements Verification Matrix

| Requirement | Requirement Statement | Code Implementation | Status |
|---|---|---|:---:|
| **REQ-1** | Service lifecycle operations inside containers dispatched over D-Bus via `PluginV1.Call`. | [`crates/op-plugins/src/state_plugins/service.rs`](file:///srv/git/odbus/crates/op-plugins/src/state_plugins/service.rs): Typed RPC methods `start`, `stop`, `restart`, `status`. | **PASS** |
| **REQ-2** | Foreign service managers forbidden inside container deployments; calls route to `busctl`. | Enforced in `AGENTS.md` and container runtime profiles. | **PASS** |
| **REQ-3** | Live service discovery scans active `/run/runit/service` instances. | [`crates/op-plugins/src/auto_create.rs:22-50`](file:///srv/git/odbus/crates/op-plugins/src/auto_create.rs#L22-L50): Dynamically scans active directory. | **PASS** |
| **REQ-4** | Service status reports PID, state, uptime, and supervision health. | Emitted via `service.status` method output. | **PASS** |
