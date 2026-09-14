# op-plugins Feature Review

## Summary
- Status: Partial
- Build: `cargo check -p op-plugins` passed
- Tests in tree: 35
- Static incompleteness markers: 13
- Patch / backup artifacts in tree: 0
- Purpose: Plugin system with state management, domain plugins, and snowball footprints
- Assessment: op-plugins builds, but the codebase still exposes unfinished paths or contract drift relative to its advertised purpose.

## Spec References
- `crates/crates/op-plugins/SPEC.md`
- `crates/crates/SPECS/25-op-plugins.md`

## Coded Features
- Public/module surface: auto_create, builtin, chat, dynamic_loading, plugin, registry, service_def, state, default_registry, state_plugins, prelude
- Source files under `src/` recursively: 48

## Alignment Review
- Compared against `crates/crates/op-plugins/SPEC.md` and `crates/crates/SPECS/25-op-plugins.md` plus the crate source tree.

## Missing Or Risky Areas
- This is one of the widest surfaces in the workspace, but many state plugins still carry validation, rollback, or install gaps. Service-management logic also still shells out to `systemctl`, which is at odds with the project architecture spec.
- Static scan found 13 TODO/stub/placeholder markers in this crate.

## Verification Notes
- `cargo check -p op-plugins` passed
- Static scan counted 35 test markers and 13 TODO/stub markers in this crate.

