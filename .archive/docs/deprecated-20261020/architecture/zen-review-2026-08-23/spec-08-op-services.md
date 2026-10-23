# Spec 08: `op-services`

**Spec Path**: [`.kiro/specs/op-services/requirements.md`](file:///srv/git/odbus/.kiro/specs/op-services/requirements.md)  
**Domain**: Host Daemon Lifecycle & Supervision  
**Status**: **PASS (Verified)**

---

## Requirements Verification Matrix

| Requirement | Requirement Statement | Code Implementation | Status |
|---|---|---|:---:|
| **REQ-1** | Daemon `run` scripts source `/etc/op-dbus/environment` with `set -a`. | [`deploy/runit/op-grpc-bridge/run:1-15`](file:///srv/git/odbus/deploy/runit/op-grpc-bridge/run#L1-L15): Sources environment definitions. | **PASS** |
| **REQ-2** | Daemon dependencies orchestrated via `wait_dep()` before exec. | Present across runit service definitions in `deploy/runit/`. | **PASS** |
| **REQ-3** | Readiness signals emitted via `/usr/local/libexec/3tched/runit-ready-signal`. | Implemented in service startup orchestration. | **PASS** |
| **REQ-4** | Log supervision writes to standard runit `log/run` rotating loggers (`svlogd`). | [`deploy/runit/op-grpc-bridge/log/run`](file:///srv/git/odbus/deploy/runit/op-grpc-bridge/log/run). | **PASS** |
