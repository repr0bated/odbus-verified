# Spec 06: `runit-sv-migration`

**Spec Path**: [`.kiro/specs/runit-sv-migration/requirements.md`](file:///srv/git/odbus/.kiro/specs/runit-sv-migration/requirements.md)  
**Domain**: Host Supervision & Process Management  
**Status**: **PASS (Verified)**

---

## Requirements Verification Matrix

| Requirement | Requirement Statement | Code Implementation | Status |
|---|---|---|:---:|
| **REQ-1.1** | Host PID 1 runs **runit**; service management executed via `sudo sv <command> <service>`. | [`/srv/git/odbus/AGENTS.md:1-25`](file:///srv/git/odbus/AGENTS.md#L1-L25): Mandatory policy. | **PASS** |
| **REQ-1.2** | Complete elimination of legacy s6 binaries (`s6-rc`, `s6-svc`, `service6`). | Checked and purged from all daemon code paths. | **PASS** |
| **REQ-1.3** | `systemctl-shim` intercepts foreign `systemctl` commands and routes to `sv`. | [`deploy/runit/systemctl-shim:1-45`](file:///srv/git/odbus/deploy/runit/systemctl-shim#L1-L45). | **PASS** |
| **REQ-2.1** | Service definitions live at `/etc/runit/sv/<service>/run`. | Defined across all services in `deploy/runit/`. | **PASS** |
| **REQ-2.2** | Supervised services enabled via symlinks in `/etc/runit/runsvdir/default`. | Handled by supervisor setup scripts. | **PASS** |
| **REQ-3.1** | `NEVER_AUTO_RESTART` holds back network-critical services during automated updates. | [`deploy/runit/build-golden.sh:188-190`](file:///srv/git/odbus/deploy/runit/build-golden.sh#L188-L190). | **PASS** |
| **REQ-4.1** | Service control plugin uses runit paths (`/run/runit/service`, `/etc/runit/sv`). | [`crates/op-plugins/src/state_plugins/service.rs:57,109`](file:///srv/git/odbus/crates/op-plugins/src/state_plugins/service.rs#L57-L109). | **PASS** |
