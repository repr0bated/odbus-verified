# op-services Feature Review

## Summary
- Status: Partial
- Build: `cargo check -p op-services` passed
- Tests in tree: 0
- Static incompleteness markers: 0
- Patch / backup artifacts in tree: 0
- Purpose: System-wide service manager - systemd replacement with dinit backend
- Assessment: op-services builds, but the codebase still exposes unfinished paths or contract drift relative to its advertised purpose.

## Spec References
- `crates/crates/op-services/SPEC.md`
- `.kiro/specs/op-services/requirements.md`
- `.kiro/specs/op-services/design.md`
- `.kiro/specs/op-services/tasks.md`
- `.kiro/specs/dbus-service-manager/requirements.md`
- `.kiro/specs/dbus-service-manager/design.md`
- `.kiro/specs/dbus-service-manager/tasks.md`

## Coded Features
- Public/module surface: dbus, grpc, manager, schema, store
- Source files under `src/` recursively: 14

## Alignment Review
- Compared against `.kiro/specs/op-services/*`, `.kiro/specs/dbus-service-manager/*`, and the local crate spec. The crate has the basic shape of the requested service manager, but large parts of the Kiro contract are still skeletal or delegated elsewhere.

## Missing Or Risky Areas
- The gRPC surface is incomplete: `reload`, `create`, `delete`, `get`, `enable`, and `disable` all return `Status::unimplemented` in `crates/crates/op-services/src/grpc/server.rs`.
- Service startup does not resolve dependencies or run health checks. `ServiceManager::start` fetches a single service and starts it directly, without using `depends_on` or any health-check data from the schema.
- The D-Bus interface declares `service_state_changed`, but `run_dbus_server` never emits that signal; it only registers the object and waits forever.
- The audit log table exists, but nothing in the manager path calls `Store::audit`, so FR-6 style operation logging is not actually wired.
- The dinit path returns PID `0` from `DinitProxy::start_service`, so runtime status is not trustworthy when dinit is active.
- Schema ownership is muddy: `schema/mod.rs` re-exports `op_plugins::service_def`, and that source still shells out to `systemctl`, which conflicts with the repo native-first architecture.
- No crate-local unit/integration tests were found under `src/`, so runtime confidence comes mostly from compilation rather than behavioral proof.

## Verification Notes
- `cargo check -p op-services` passed
- Static scan counted 0 test markers and 0 TODO/stub markers in this crate.

