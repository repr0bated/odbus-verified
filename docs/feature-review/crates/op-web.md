# op-web Feature Review

## Summary
- Status: Broken
- Build: `cargo check -p op-web` fails with `error[E0425]: cannot find value magic_url in this scope`
- Tests in tree: 24
- Static incompleteness markers: 46
- Patch / backup artifacts in tree: 6
- Purpose: Unified web server for op-dbus-v2 - consolidates all HTTP services
- Assessment: op-web is currently non-buildable, so its advertised feature set is not usable end to end.

## Spec References
- `crates/crates/op-web/SPEC.md`
- `.kiro/specs/op-web/requirements.md`
- `.kiro/specs/op-web/design.md`
- `.kiro/specs/op-web/tasks.md`

## Coded Features
- Public/module surface: email, embedded_ui, groups_admin, handlers, mcp, mcp_agents, mcp_compact, mcp_discovery, middleware, orchestrator, privacy_container, privacy_openflow, privacy_network, privacy_routes, routes, sse, state, state_manager_client, users, websocket, wireguard
- Source files under `src/` recursively: 54

## Alignment Review
- Compared against `.kiro/specs/op-web/*` and the embedded UI design. The crate contains the intended HTTP/MCP/UI hosting pieces, but the actual product path is broken and the routing/build integration is internally inconsistent.

## Missing Or Risky Areas
- The crate does not compile because `crates/crates/op-web/src/email.rs` logs `magic_url` before it is defined.
- The Kiro design says `build.rs` should compile the UI with `npm run build`, but the real `build.rs` only checks whether `ui/dist/index.html` exists and never builds assets itself.
- There are two router implementations. `main.rs` uses `routes::create_router`, while `router.rs` is a smaller, effectively dead router that is not exported from `lib.rs` and can drift out of sync.
- The embedded UI path exists and `op-web/ui` can build independently, but the single-binary deployment claim is not true while the Rust crate fails to compile.
- `cargo check -p op-web` fails with `error[E0425]: cannot find value magic_url in this scope`.
- Static scan found 46 TODO/stub/placeholder markers in this crate.
- Static scan found 6 patch/backup artifact files checked into the crate tree.

## Verification Notes
- `cargo check -p op-web` fails with `error[E0425]: cannot find value magic_url in this scope`
- Static scan counted 24 test markers and 46 TODO/stub markers in this crate.
- Static scan also found 6 patch/backup artifacts in the crate tree.

