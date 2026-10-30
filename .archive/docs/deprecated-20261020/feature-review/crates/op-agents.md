# op-agents Feature Review

## Summary
- Status: Partial
- Build: `cargo check -p op-agents` passed
- Tests in tree: 32
- Static incompleteness markers: 3
- Patch / backup artifacts in tree: 0
- Purpose: Secure agent registry and D-Bus agent implementations for op-dbus-v2
- Assessment: op-agents builds, but the codebase still exposes unfinished paths or contract drift relative to its advertised purpose.

## Spec References
- `crates/crates/op-agents/SPEC.md`
- `crates/crates/SPECS/01-op-agents.md`

## Coded Features
- Public/module surface: agent_catalog, agent_registry, agents, dbus_service, router, security
- Source files under `src/` recursively: 130

## Alignment Review
- Compared against `crates/crates/op-agents/SPEC.md` and `crates/crates/SPECS/01-op-agents.md` plus the crate source tree.

## Missing Or Risky Areas
- The agent catalog/registry is substantial, but TODO markers remain around routing and health-check startup. Static scan indicates usable scaffolding, not a fully finished agent runtime.
- Static scan found 3 TODO/stub/placeholder markers in this crate.

## Verification Notes
- `cargo check -p op-agents` passed
- Static scan counted 32 test markers and 3 TODO/stub markers in this crate.

