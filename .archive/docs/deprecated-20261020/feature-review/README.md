# Crates Feature Review

## Scope
- Rust workspace crates under `crates/crates/op-*`.
- Frontend apps under `crates/` and `crates/crates/op-web/ui`.
- Kiro specs under `.kiro/specs/*` where available.

## Method
- Static code review of module surfaces, TODO/stub markers, and checked-in patch artifacts.
- Build verification with `cargo check -p <crate>` for each Rust crate.
- Frontend verification with `npm ci`, `npx tsc --noEmit`, `npm test`, and `npm run build` where applicable.

## Result Summary
- Rust crates reviewed: 31
- Rust crates building individually: 28
- Rust crates currently failing to build: 3
- Root `crates/` frontend: typecheck/build/test pass after dependency install.
- Embedded `crates/crates/op-web/ui` frontend: typecheck/build/test pass, but feature completeness and backend alignment are still partial.

## Major Findings
- `op-web` is not currently buildable because `crates/crates/op-web/src/email.rs` references `magic_url` before it is defined. That blocks the single-binary web product path.
- The embedded `op-web/ui` app builds, but it is not actually feature-complete against the Kiro task list: several claimed completed features have no matching dependencies or source files, and large parts of its REST client target routes the backend does not expose.
- `op-services` only implements the basic shell of the Kiro service-manager spec. Core operations are still `unimplemented`, dependency ordering/health checks/audit logging are not wired, and its schema source still shells out to `systemctl`.
- `op-deployment` is non-buildable because of unsafe `simd_json::from_str` usage in `image_manager.rs`.
- `op-cognitive-mcp` is non-buildable because `lib.rs` declares a `dynamic_loader` module that does not exist.

## Review Matrix
| Target | Kind | Status | Notes |
|---|---|---|---|
| [op-agents](./crates/op-agents.md) | Rust crate | Builds | Partial |
| [op-snowball](./crates/op-snowball.md) | Rust crate | Builds | Buildable |
| [op-cache](./crates/op-cache.md) | Rust crate | Builds | Partial |
| [op-chat](./crates/op-chat.md) | Rust crate | Builds | Partial |
| [op-cognitive-mcp](./crates/op-cognitive-mcp.md) | Rust crate | Fails | Broken |
| [op-core](./crates/op-core.md) | Rust crate | Builds | Buildable |
| [op-dbus-mirror](./crates/op-dbus-mirror.md) | Rust crate | Builds | Buildable |
| [op-dbus-model](./crates/op-dbus-model.md) | Rust crate | Builds | Buildable |
| [op-deployment](./crates/op-deployment.md) | Rust crate | Fails | Broken |
| [op-dynamic-loader](./crates/op-dynamic-loader.md) | Rust crate | Builds | Buildable |
| [op-execution-tracker](./crates/op-execution-tracker.md) | Rust crate | Builds | Buildable |
| [op-gateway](./crates/op-gateway.md) | Rust crate | Builds | Buildable |
| [op-grpc-bridge](./crates/op-grpc-bridge.md) | Rust crate | Builds | Partial |
| [op-http](./crates/op-http.md) | Rust crate | Builds | Buildable |
| [op-identity](./crates/op-identity.md) | Rust crate | Builds | Partial |
| [op-inspector](./crates/op-inspector.md) | Rust crate | Builds | Buildable |
| [op-introspection](./crates/op-introspection.md) | Rust crate | Builds | Buildable |
| [op-jsonrpc](./crates/op-jsonrpc.md) | Rust crate | Builds | Buildable |
| [op-llm](./crates/op-llm.md) | Rust crate | Builds | Partial |
| [op-mcp](./crates/op-mcp.md) | Rust crate | Builds | Partial |
| [op-mcp-aggregator](./crates/op-mcp-aggregator.md) | Rust crate | Builds | Partial |
| [op-mcp-proxy](./crates/op-mcp-proxy.md) | Rust crate | Builds | Buildable |
| [op-ml](./crates/op-ml.md) | Rust crate | Builds | Partial |
| [op-network](./crates/op-network.md) | Rust crate | Builds | Partial |
| [op-plugins](./crates/op-plugins.md) | Rust crate | Builds | Partial |
| [op-services](./crates/op-services.md) | Rust crate | Builds | Partial |
| [op-state](./crates/op-state.md) | Rust crate | Builds | Partial |
| [op-state-store](./crates/op-state-store.md) | Rust crate | Builds | Partial |
| [op-tools](./crates/op-tools.md) | Rust crate | Builds | Partial |
| [op-web](./crates/op-web.md) | Rust crate | Fails | Broken |
| [op-workflows](./crates/op-workflows.md) | Rust crate | Builds | Partial |
| [root-crates-ui](./frontends/root-crates-ui.md) | Frontend app | Builds | Buildable prototype |
| [op-web-ui](./frontends/op-web-ui.md) | Embedded frontend app | Builds | Partial |

## Bundle Contents
- One per-crate feature review under `docs/feature-review/crates/`.
- Frontend-specific reviews under `docs/feature-review/frontends/`.

## Verification Commands Used
- `cargo check --workspace`
- `cargo check -p op-services`
- `cargo check -p op-web`
- Per-crate `cargo check -p <crate>` matrix (unique result set summarized here).
- `cd crates/crates/op-web/ui && npx tsc --noEmit && npm test && npm run build`
- `cd crates && npm ci && npx tsc --noEmit && npm test && npm run build`

