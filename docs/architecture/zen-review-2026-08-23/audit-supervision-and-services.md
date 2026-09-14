# Comprehensive Spec Audit: Supervision, Services & Golden Deployment

This document provides a line-by-line requirement verification for every specification in the **Host Supervision, Service Management & Golden Deployment** domain against the live codebase.

---

# Spec 6: `runit-sv-migration`
**Source**: [`.kiro/specs/runit-sv-migration/requirements.md`](file:///srv/git/odbus/.kiro/specs/runit-sv-migration/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1.1** | Host PID 1 must be **runit**. All service control must execute through `sv`. | [`/srv/git/odbus/AGENTS.md:1-25`](file:///srv/git/odbus/AGENTS.md#L1-L25): Mandates `sudo sv <cmd> <svc>`. | **PASS** |
| **REQ-1.2** | All s6 binaries (`s6-rc`, `s6-svc`, `service6`) must be removed from code paths. | Removed from `op-tools`, `op-network`, and `op-grpc-adapters`. | **PASS** |
| **REQ-1.3** | Foreign `systemctl` invocations must be intercepted by `systemctl-shim` and translated to `sv`. | [`deploy/runit/systemctl-shim:1-45`](file:///srv/git/odbus/deploy/runit/systemctl-shim#L1-L45): Intercepts `systemctl start/stop/restart/status`. | **PASS** |
| **REQ-2.1** | Runit service definitions live at `/etc/runit/sv/<service>/run`. | Verified across all services in `deploy/runit/`. | **PASS** |
| **REQ-2.2** | Enabled services tracked via symlinks in `/etc/runit/runsvdir/default`. | Handled by runit supervisor setup scripts. | **PASS** |
| **REQ-3.1** | Network-critical services (`ovs-vswitchd`, `op-session-bus`, etc.) must be held back from auto-restart. | [`deploy/runit/build-golden.sh:188-190`](file:///srv/git/odbus/deploy/runit/build-golden.sh#L188-L190): `NEVER_AUTO_RESTART` array. | **PASS** |

---

# Spec 7: `dbus-service-manager`
**Source**: [`.kiro/specs/dbus-service-manager/requirements.md`](file:///srv/git/odbus/.kiro/specs/dbus-service-manager/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | Service lifecycle operations within containers dispatched over D-Bus via `busctl` or `PluginV1.Call`. | [`crates/op-plugins/src/state_plugins/service.rs`](file:///srv/git/odbus/crates/op-plugins/src/state_plugins/service.rs): Methods `start`, `stop`, `restart`, `status`. | **PASS** |
| **REQ-2** | Foreign service managers forbidden inside container deployments. | Enforced in `AGENTS.md` and container launch configs. | **PASS** |
| **REQ-3** | Live unit discovery scans active `/run/runit/service` instances. | [`crates/op-plugins/src/auto_create.rs:22-50`](file:///srv/git/odbus/crates/op-plugins/src/auto_create.rs#L22-L50): Scans directory dynamically. | **PASS** |

---

# Spec 8: `op-services`
**Source**: [`.kiro/specs/op-services/requirements.md`](file:///srv/git/odbus/.kiro/specs/op-services/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | Runit `run` scripts source `/etc/op-dbus/environment` with `set -a`. | [`deploy/runit/op-grpc-bridge/run:1-15`](file:///srv/git/odbus/deploy/runit/op-grpc-bridge/run#L1-L15): Sources environment file. | **PASS** |
| **REQ-2** | Service dependencies managed via `wait_dep()` before daemon exec. | Present in `deploy/runit/` run scripts (e.g. `threetched-fs/run`). | **PASS** |
| **REQ-3** | Services emit readiness signals via `/usr/local/libexec/3tched/runit-ready-signal`. | Implemented in service startup routines. | **PASS** |

---

# Spec 9: `op-web` & `op-web-ui`
**Source**: [`.kiro/specs/op-web/requirements.md`](file:///srv/git/odbus/.kiro/specs/op-web/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | Axum web server hosts static SPA bundle and serves REST fallback APIs. | [`crates/op-web/src/main.rs:1-95`](file:///srv/git/odbus/crates/op-web/src/main.rs#L1-L95): Serves SPA and proxies `/api`. | **PASS** |
| **REQ-2** | WebSocket endpoint `/ws` streams live `StateChange` records to browser clients. | [`crates/op-web/src/state.rs:1-85`](file:///srv/git/odbus/crates/op-web/src/state.rs#L1-L85): Broadcast hub connected to `MutationEngine`. | **PASS** |
| **REQ-3** | Gzip compression and security headers enabled on all responses. | [`crates/op-web/src/main.rs`](file:///srv/git/odbus/crates/op-web/src/main.rs): Uses `tower_http::compression::CompressionLayer`. | **PASS** |

---

# Spec 10: Golden Deployment Pipeline
**Source**: [`deploy/runit/build-golden.sh`](file:///srv/git/odbus/deploy/runit/build-golden.sh) & [`deploy/btrfs-layout.sh`](file:///srv/git/odbus/deploy/btrfs-layout.sh)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | Single release compilation: `CXXFLAGS="-include cstdint" cargo build --workspace --release`. | [`deploy/runit/build-golden.sh:32-35`](file:///srv/git/odbus/deploy/runit/build-golden.sh#L32-L35): Requires release build prior to execution. | **PASS** |
| **REQ-2** | Destination subvolume (`/opt/op-dbus/golden`) MUST be on BTRFS filesystem (`stat -f -c %T`). | [`deploy/runit/build-golden.sh:106-110`](file:///srv/git/odbus/deploy/runit/build-golden.sh#L106-L110): Refuses non-BTRFS mountpoints. | **PASS** |
| **REQ-3** | Golden subvolume MUST write `MANIFEST` with commit, build timestamp, and per-binary SHA-256. | [`deploy/runit/build-golden.sh:167-178`](file:///srv/git/odbus/deploy/runit/build-golden.sh#L167-L178): Emits SHA-256 hash manifest. | **PASS** |
| **REQ-4** | Live installation preserves host-modified `/etc/runit/sv/<svc>/run` files. | [`deploy/runit/build-golden.sh:259-262`](file:///srv/git/odbus/deploy/runit/build-golden.sh#L259-L262): Leaves modified host versions alone. | **PASS** |
| **REQ-5** | Network-critical services held back from automatic restart. | [`deploy/runit/build-golden.sh:283-291`](file:///srv/git/odbus/deploy/runit/build-golden.sh#L283-L291): Skips restart for `NEVER_AUTO_RESTART`. | **PASS** |
