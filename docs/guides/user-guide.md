# User Guide & How-To — operation-dbus-proto

> Generated 2026-07-02. Practical, task-oriented guide for building, running,
> operating, and extending the control plane. For concepts see
> [`docs/overview/architecture.md`](../overview/architecture.md); for exact
> contracts see [`docs/reference/api-reference.md`](../reference/api-reference.md).

## 1. Audience

- **Operators** who build and run the control plane and inspect/mutate state.
- **Plugin developers** who add or migrate a state plugin.
- **Client integrators** who connect an MCP or gRPC client.

## 2. Prerequisites

- Artix Linux (or a compatible host) with **runit** supervision (`sv`), and access to the
  system D-Bus.
- Rust toolchain (edition 2021), Node.js + npm for the UI.
- Chimera Linux deps: `doas apk add rust cargo nodejs npm pkgconfig openssl-dev`.

> This workspace patches `zbus` to a local checkout (`[patch.crates-io]` in
> `Cargo.toml` points at `/home/jeremy/git/zbus/...`). If that path is not
> present, adjust the patch section before building.

## 3. Build

```bash
# Build the whole workspace
cargo build --workspace

# Release build
cargo build --workspace --release

# Build one crate (fast inner loop)
cargo check -p op-plugins
```

Frontend:

```bash
cd crates/op-web/ui && npm ci && npm run build:prod
```

## 4. Lint, format, and test (run before completing work)

```bash
# Rust
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features

# Frontend (from crates/)
cd crates && npm test
cd crates && npm run lint
cd crates && npm run typecheck
```

## 5. Run

```bash
# Unified web server + chat UI
cargo run --release -p op-web
```

Service definitions live under `deploy/runit/`. The control-plane services
(bridge, mirror, MCP gateway, plugin host) are started via `sv`, not by hand, in a
deployed environment.

## 6. Inspect and mutate state (D-Bus first)

Everything goes through D-Bus. Never shell out to `systemctl`, `ip`,
`ovs-vsctl`, or read config files for live state.

### 6.1 Discover a plugin object

```bash
busctl --system introspect org.opdbus.v1.plugins /org/opdbus/v1/plugins/<plugin_id>
```

### 6.2 Read state

- Call `get_state` / `get_schema` on the `PluginV1` interface, or
- Call the gRPC `PluginService.GetSchema` / `StateSync.GetState`, or
- Read the live projection from `/dev/shm` (1:1 direct read).

### 6.3 Mutate state

Call a `Mutation` method on the plugin object, or `StateSync.Mutate` on the
bridge with `plugin_id`, `path`, `op`, `value`, and a `capability_id`. Every
mutation is validated against the schema, then recorded in the event chain with
actor + capability attribution and a Merkle proof.

## 7. Connect a client

| Client | Point it at |
|---|---|
| NotebookLM, Droid, Cursor, Codex, Junie, Gemini CLI | `op-cognitive-mcp` at `:3003` (WireGuard `100.90.37.254`) — the universal gateway |
| Local chatbot only | `compact-mcp` at `127.0.0.1:11436` (never external) |
| Generic MCP | `op-mcp` (stdio / HTTP-SSE / WebSocket / gRPC; modes compact / agents / full) |

Do not create new shim services or point external clients at
`op-assistant-grpc` directly.

## 8. Add or migrate a state plugin

The plugin owns its schema; schemars derives it. Reference implementation:
`crates/op-plugins/src/state_plugins/unix_socket.rs`.

1. **State struct** — derive `schemars::JsonSchema`; add
   `#[schemars(extend("x-oscal-subid" = "..."))]` at struct and field level.
   ```rust
   #[derive(schemars::JsonSchema, serde::Serialize, serde::Deserialize)]
   #[schemars(extend("x-oscal-subid" = "obs.software.my-plugin.state@v1"))]
   struct MyPluginState { /* fields */ }
   ```
2. **Method input/output structs** — each derive `schemars::JsonSchema`.
3. **Schema function** — co-located `<plugin>_schema()` that calls
   `schemars_adapter::plugin_schema_from_json`, `apply_state_defaults`,
   `ensure_category_metadata_fields`, then inserts methods with
   `method_decl_from_schemars_with_output::<In, Out>()`.
4. **`schema()` impl** — return the schema function result (call
   `ensure_category_metadata_fields` once).
5. **Drift test** — use `schema_diffs()` to prove the derived schema matches the
   expected contract.
6. **Self-register** — add `inventory::submit!` so `DefaultPluginRegistry`
   discovers the plugin.
7. **Wire the module** — add `pub mod <plugin>;` (and any re-export) in
   `crates/op-plugins/src/state_plugins/mod.rs`.

Rules to respect:

- Do not define another plugin's schema inline; `plugin_schema_defs.rs` is a
  re-export aggregator plus shared helpers only.
- `PluginSchema.methods` is the method authority — not D-Bus introspection.
- `mut.*` subids require `actor_id` + `capability_id`; `evt.*` require
  `event_id`/`event_hash`.
- No CLI subprocesses, no config-file reads for live state, no polling loops in
  plugin/service code.

Verify:

```bash
cargo check -p op-plugins
cargo test -p op-plugins
cargo clippy -p op-plugins --all-targets --all-features -- -D warnings
```

## 9. Migration tiers (schemars pipeline)

From `.kiro/specs/schemars-to-reflection-plugin-pipeline`:

| Tier | State |
|---|---|
| **A — Complete** | State struct derives `JsonSchema`; `schema()` uses `plugin_schema_from_json`; all methods use `_with_output`; drift test present |
| **B — Methods only** | Method inputs typed; state hand-rolled via `simple_schema`/`any_field` |
| **C — Legacy** | State and methods use anonymous `any_field`/`json!({})` |

Target tier A. Migrating a plugin means typing the state struct and switching
`schema()` to the adapter, keeping the drift test green.

## 10. Working with the cognitive-mcp / embedding boundary

Per `.kiro/specs/voyage-plugin-cognitive-mcp-boundaries`:

- Configure Voyage embedding **only** through the `embedding_model` plugin
  (provider, API key, endpoint, model, dimensions). Do not add
  `VOYAGE_PUBLIC_URL` / `VOYAGE_MONGODB_URL` / `al-` prefix logic anywhere else.
- Consumers read runtime config from `/dev/shm/opdbus/projections/embedding_model`
  and `/dev/shm/opdbus/projections/cognitive_mcp`; env vars are a bootstrap-only
  fallback.
- `op-cognitive-mcp` reads projections, never writes them.

## 11. Common pitfalls

- **Bypassing D-Bus.** If you reach for `Command::new("systemctl")` etc., stop
  and find the D-Bus object. Bootstrap scripts are the only exception.
- **Inline schemas.** A schema defined inline in another plugin's file is never
  registered — the D-Bus object will have no contract.
- **Phantom reflection.** If a method shows in reflection but has no route, the
  blob/route wiring is wrong. Reflection advertises active blobs only.
- **Writing to `/dev/shm` from the wrong crate.** Only `op-grpc-bridge` writes
  projections.

## 12. Reference index

- Concepts & crate map → [`../overview/architecture.md`](../overview/architecture.md)
- Contracts & interfaces → [`../reference/api-reference.md`](../reference/api-reference.md)
- Plugin object blobs → [`../schema-coupled-plugin-blob-reflection-whitepaper.md`](../schema-coupled-plugin-blob-reflection-whitepaper.md)
- Traffic diagrams → [`../architecture-flow.md`](../architecture-flow.md)
- Subid taxonomy → [`../subid-taxonomy.md`](../subid-taxonomy.md)
- Kiro specs → `.kiro/specs/`
