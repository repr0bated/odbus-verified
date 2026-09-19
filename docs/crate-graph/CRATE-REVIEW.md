# OP-DBUS crate-by-crate review, traced into a Cozo graph

Date: 2026-09-19 · Baseline: `8d01947 feat(op-dbus): initialize verified source baseline`
Reviewer: Claude (Opus 5) · Method: static review of every workspace member plus a
generated dependency/module/plugin/service graph loaded into `op-cozo-store`.

Companion artifacts in this directory:

| File | What |
|---|---|
| `crate-graph.cozo` | CozoScript that `:put`s 1,259 nodes / 2,853 edges into the seed relations `graph_node {id => label, props}` and `graph_edge {src, rel, dst => props}` already defined by `CozoGraphShuttle::seed_schema` |
| `crate-graph.json` | Same graph as `{nodes:[…], edges:[…]}` for jq / other tooling |
| `../../scripts/crate-graph.sh` | Generator (`cargo metadata` + jq/awk/grep — no Python, per AGENTS.md). Re-run after any Cargo/module change |
| `../../crates/op-cozo-store/examples/load_crate_graph.rs` | Loads the script into an in-memory or RocksDB `CozoGraphShuttle` and runs the structural queries in §1 |

```sh
scripts/crate-graph.sh                                   # regenerate docs/crate-graph/*
CXXFLAGS="-include cstdint" cargo run -p op-cozo-store --example load_crate_graph -- docs/crate-graph/crate-graph.cozo
#   … or persist it next to the other Cozo stores:
CXXFLAGS="-include cstdint" cargo run -p op-cozo-store --example load_crate_graph -- docs/crate-graph/crate-graph.cozo /var/lib/opdbus/crate-graph.cozo
```

Both commands were run for this review; the load succeeds and every query below
returned rows from the loaded graph.

---

## 0. Graph model

Node ids are prefixed by kind so they can share one `graph_node` relation with the
identity graph already stored there.

| label | id form | props | count |
|---|---|---|---|
| `crate` | `crate:op-core` | path, description, bins[], has_lib, rs_files, loc, tests, unsafe_refs, todo, unwrap_expect | 38 (37 members + root `op-dbus`) |
| `ext_crate` | `ext:tonic` | req | 123 |
| `module` | `mod:crates/op-core/src/lib.rs` (one per `.rs` file) | crate, kind (src/test/example/bench/bin/build), loc | 962 |
| `bin` | `bin:opblob` | crate, src | 32 |
| `plugin` | `plugin:proxy_server` | file, line, declared, dbus_path | 69 (67 declared + 2 phantom, see §2.1) |
| `proto_service` | `proto:operation.v1.PluginService` | file, package | 11 |
| `runit_service` | `runit:op-web` | run, enabled, retired | 5 |
| `meta` | `meta:crate-graph` | generated_at, commit, generator | 1 |

| rel | src → dst | meaning |
|---|---|---|
| `depends_on` / `dev_depends_on` / `build_depends_on` | crate → crate | Cargo path dependency (kind preserved) |
| `uses_ext` | crate → ext_crate | Cargo registry dependency (props: kind, optional) |
| `contains` | crate → module | file belongs to crate |
| `imports` | module → crate | file references `op_<x>::` outside comments (props: refs) |
| `provides_bin` / `bin_entry` | crate → bin, bin → module | binary target and its entry file |
| `registers_plugin` / `defines_plugin` | crate → plugin, module → plugin | `inventory::submit! { PluginReg::new(…) }` site |
| `declares_proto_service` | crate → proto_service | hand-written `.proto` under `op-grpc-bridge/proto/` |
| `runs_bin` | runit_service → bin | `deploy/runit/<svc>/run` execs a workspace binary |

Not modelled (deliberately, would need a compiler pass): function-level call
graphs, the *generated* per-plugin gRPC services (`plugin_methods.proto` is a build
artifact), and D-Bus method inventories (those live in the sealed blob catalog,
which is the authority for them anyway).

### Query cookbook

```
# Who would be rebuilt if op-state-store changed? (reverse transitive closure)
up[c] := c = "crate:op-state-store"
up[c] := up[d], *graph_edge[c, "depends_on", d, _]
?[c] := up[c]

# Which files define plugins, and how big are they?
?[plugin, file, loc] := *graph_edge[file, "defines_plugin", plugin, _], *graph_node[file, "module", p], loc = get(parse_json(p), "loc")

# Modules that reference a crate their owner does not declare in Cargo.toml
dep[a, b] := *graph_edge[a, "depends_on", b, _]
dep[a, b] := *graph_edge[a, "dev_depends_on", b, _]
?[m, t] := *graph_edge[m, "imports", t, _], *graph_edge[o, "contains", m, _], not dep[o, t]

# runit service → binary → crate
?[svc, bin, crate] := *graph_edge[svc, "runs_bin", bin, _], *graph_edge[crate, "provides_bin", bin, _]
```

---

## 1. Workspace shape (what the graph says)

**Dependency layers** (longest path from leaves; a crate's layer = 1 + max of its deps):

```
L0  op-cozo-store  op-deployment  op-execution-tracker  op-gateway  op-http  op-state-store  op-xray-daemon
L1  op-blob  op-cache  op-core
L2  op-agents  op-compliance  op-dbus-model  op-gemma  op-jsonrpc  op-network  op-runit-systemctl  op-snowball
L3  op-identity  op-introspection  op-state
L4  op-dbus-cli  op-inspector
L5  op-plugin-lint  op-tools
L6  op-dynamic-loader
L7  op-plugins
L8  op-llm  op-mcp  op-mcp-aggregator  op-services  op-workflows
L9  op-cognitive-mcp
L10 op-assistant-grpc  op-grpc-bridge
L11 op-chat
L12 op-web
L13 op-dbus (root; one example target)
```

No dependency cycles (normal or dev). The chain is 14 deep; `op-web` transitively
compiles essentially the entire workspace.

**Fan-in** (`depends_on`, direct): op-core 23 · op-state-store 12 · op-plugins 8 ·
op-execution-tracker 7 · op-identity 7 · op-introspection 7 · op-network 7 · op-tools 7 ·
op-blob 6 · op-snowball 6 · op-state 6.
**Fan-out**: op-grpc-bridge 14 · op-web 14 · op-cognitive-mcp 11 · op-plugins 11 · op-mcp 9.

**Actual reference weight** (`imports` edges, file-level): op-grpc-bridge → op-plugins
283 refs, op-plugins → op-state-store 463 refs, op-grpc-bridge → op-state-store 145,
op-grpc-bridge → op-blob 137. The real core of the system is the triangle
**op-state-store ⇐ op-plugins ⇐ op-grpc-bridge**; everything else is periphery.

**Transports by crate** (from `uses_ext`):

| transport | crates |
|---|---|
| D-Bus (`zbus`) | op-agents, op-assistant-grpc, op-chat, op-cognitive-mcp, op-core, op-dbus-cli, op-grpc-bridge, op-identity, op-introspection, op-mcp, op-network, op-plugins, op-runit-systemctl, op-state, op-state-store, op-tools, op-web, op-xray-daemon |
| gRPC (`tonic`) | op-assistant-grpc, op-cache, op-chat, op-cognitive-mcp, op-grpc-bridge, op-mcp, op-services, op-snowball, op-xray-daemon |
| HTTP server (`axum`) | op-agents, op-grpc-bridge, op-http, op-mcp, op-tools, op-web |
| HTTP client (`reqwest`) | 13 crates |
| SQLite (`rusqlite`) | op-cache, **op-introspection** |
| CozoDB | op-cozo-store, op-cognitive-mcp |

**Deployed surface** (`runit_service → bin → crate`, from `deploy/runit/*/run`):

| runit service | binary | crate | enabled-services |
|---|---|---|---|
| op-grpc-bridge | op-grpc-bridge | op-grpc-bridge | yes |
| op-web | op-web-server | op-web | yes |
| op-runit-systemctl | op-runit-systemctl | op-runit-systemctl | yes |
| opdbus-grants | op-grants-materializer | op-grpc-bridge | no (dependency of the bridge) |
| op-of-controller | op-of-controller | op-network | no |

So of 32 workspace binaries, 5 are referenced by a runit definition and 3 are in
`enabled-services`. The other 27 are tooling (`opblob`, `dump-protos`, `gen_*`,
`op-plugin-lint`, …) or unwired daemons (see §2.7).

**69 plugin registrations** exist in `op-plugins/src/state_plugins/` (one
`inventory::submit!` per file, ids in `crate-graph.json`), but only **67 compile**:
`host_runtime` and `privacy_router` live in files `state_plugins/mod.rs` never
declares, so their `inventory::submit!` is never built and they do not exist on the
bus (`props.declared == false` on the plugin node). **11 hand-written proto
services** in `op-grpc-bridge/proto/`: PrivacyNetworkService, StateSync, PluginService,
EventChainService, OvsdbMirror, RuntimeMirror, DbusPassthrough, MailService,
HookProvider (EMQX exhook), ComponentRegistry, RegistrationService.

---

## 2. Cross-cutting findings (ranked by impact)

### 2.1 ≈34 k LOC of source is not compiled — orphaned files under `src/`

Files that no `lib.rs`/`main.rs`/`bin` reaches through `mod` / `#[path]` /
`include!`. They are invisible to `cargo build`, `clippy` and `/verify`, so they rot
silently, and several of them are exactly the "retired design" code AGENTS.md says
must not come back (`systemd.rs`, `netmaker` helpers, the `op-mcp` HTTP server, the
old `op-web` orchestrator).

| crate | orphaned files | LOC | notable |
|---|---|---|---|
| op-tools | 22 | 8,260 | entire second `builtin/` tool set (`dbus_tool.rs`, `shell_tool.rs`, `incus_tools.rs`, `lxc_tools.rs`, `self_tools.rs`…), `builtin_old.rs`, `executor.rs` |
| op-chat | 13 | 7,829 | `tool_loader.rs` (2,222), `grpc_client.rs`, `router.rs`, `chat_loop.rs`, the whole `orchestration/{coordinator,executor,workflows,workstacks,dbus_orchestrator}.rs` set |
| op-mcp | 20 | 4,378 | `http_server.rs`, `router.rs`, `sse.rs`, `request_handler.rs`, `tool_adapter*.rs`, `tools/{shell,systemd,filesystem,…}.rs` — the pre-bridge standalone MCP listener |
| op-plugins | 6 | 3,759 | `privacy_router.rs` (1,558), `host_runtime.rs`, `systemd.rs`, `systemd_networkd.rs`, `privacy.rs`, `common/selector.rs` |
| op-introspection | 3 | 2,395 | `src/mod.rs` (1,028 — a stray module root), `hierarchical.rs`, `cpu_features.rs` |
| op-inspector | 2 | 1,360 | `cli.rs`, `datadump.rs` |
| op-agents | 4 | 1,172 | `generator/{template,md_parser,mod}.rs`, `agents/system/mod.rs` |
| op-web | 6 | 1,098 | `handlers/auth_bridge.rs`, `handlers/websocket.rs`, `orchestrator/anti_hallucination.rs`, `router.rs`, `server.rs`, `system_prompt_loader.rs` |
| op-http | 3 | 701 | `health.rs`, `metrics.rs`, `request_filters.rs` |
| op-mcp-aggregator | 1 | 631 | `unused/context.rs` (already self-labelled) |
| op-jsonrpc | 3 | 555 | `ovsdb_jsonrpc.rs`, `ovsdb_rpc_call.rs`, `nonnet_staging.rs` |
| op-llm | 1 | 490 | `antigravity_replay.rs` |
| op-plugin-lint | 1 | 463 | `rust_emit.rs` |
| op-core | 2 | 370 | `connection.rs`, `message.rs` |
| op-state-store | 1 | 303 | `subid_ui.rs` |
| op-grpc-bridge | 2 | 204 | `callable_reflection.rs`, `schema_passthrough.rs` |
| op-state | 2 | 180 | `manager.rs` (the excised StateManager lib.rs says is gone), `mod.rs` |
| op-cognitive-mcp | 1 | 132 | `live_schema.rs` (references the retired `live-schema.json` design) |

Detection: `scratch/orphans.py`-style walk (resolver honours `#[path]`, `include!`,
`src/bin/*`, `[[bin]] path`). The 10 "undeclared import" edges in the graph
(`op-mcp/src/*.rs → op-agents/op-http/op-chat`, `op-chat/src/router.rs → op-http`,
`op-plugin-lint/src/rust_emit.rs → op-state-store`) are *all* inside these orphaned
files — they literally cannot compile in their current crate, which is independent
confirmation. Two of the orphans carry `inventory::submit!` plugin registrations
(`host_runtime`, `privacy_router`): those plugins are phantoms — present in source,
absent from the built registry and therefore from the sealed catalog.

**Recommendation.** Decide per file: wire it back with a `mod` line (and fix the
missing Cargo deps), or delete it. Add a CI/`/verify` step that fails on unreachable
`.rs` files so this cannot recur. Candidates that look like intentional retirements
and should simply go: `op-plugins/systemd*.rs`, `op-mcp/tools/systemd.rs`,
`op-mcp/http_server.rs` + `router.rs` + `sse.rs`, `op-cognitive-mcp/live_schema.rs`,
`op-state/manager.rs`, `op-tools/builtin_old.rs`, `op-mcp-aggregator/unused/`.

### 2.2 24 declared internal dependencies are never referenced

Each pair below is in the dependent's `Cargo.toml` but no `.rs` file in that crate
mentions the crate (checked with a raw identifier grep, not just the graph):

```
op-core            → op-execution-tracker     (foundation crate pulling prometheus/sha2 for nothing; 23 dependents pay for it)
op-state           → op-jsonrpc, op-network, op-snowball   (op-network brings netlink/rtnetlink/reqwest into 76-ref op-plugins' build graph)
op-mcp             → op-plugins, op-state, op-state-store
op-cognitive-mcp   → op-cache, op-core, op-dynamic-loader
op-plugins         → op-dynamic-loader, op-execution-tracker
op-grpc-bridge     → op-jsonrpc
op-identity        → op-compliance
op-web             → op-state
op-tools           → op-execution-tracker
op-compliance, op-dbus-model, op-dynamic-loader, op-inspector, op-jsonrpc, op-snowball → op-core
op-dbus-cli        → op-introspection
op-mcp-aggregator  → op-plugins
```

**Recommendation.** Remove them; this both shortens the 14-deep build chain and makes
the graph honest. `cargo machete` / `cargo udeps` would keep it that way.

### 2.3 Stale editor/patch artifacts committed inside `src/`

30 non-source files live next to code: `*.bak-plane-safe-20260806T…`, `*.patch`,
`*.backup`, `*.copied`, `*.stub`, `*.fix`, `lib.rs.grpc-additions`, an
`op-dbus-v2.code-workspace`, four `*.sql` schema files in `op-state-store/src`
(`ad_full_schema.sql`, `cms_drupal_schema.sql`, `cms_wordpress_schema.sql`,
`namespace_schema.sql` — never `include_str!`'d, and SQL in a no-SQLite project).
None affect the build; all of them make grep/review noisier and contradict "verified
source baseline".

**Recommendation.** Delete; add `*.bak*`, `*.patch`, `*.backup`, `*.stub`, `*.fix`
to `.gitignore`.

### 2.4 Control-plane bypasses in *compiled* plugin/service code

AGENTS.md: "No `Command::new(...)` subprocesses … in plugin/service code";
runit control goes through `op-runit-systemctl` at
`/org/opdbus/v1/plugins/runit/systemctl`. Compiled violations (orphaned files excluded):

| where | what | note |
|---|---|---|
| `op-plugins/src/service_def.rs:447,503` | `tokio::process::Command::new("sv")` for status/ctl | used by `state_plugins/service.rs` and re-exported to `op-services` — the `service` plugin bypasses the D-Bus runit door its sibling `service/methods.rs` already targets |
| `op-state/src/authority.rs:25,46` | `Command::new(SV_BIN)` to stop competing network managers | library code in L3; documented as deliberate, still a bypass |
| `op-grpc-bridge/src/grpc_server.rs:3012` | `runit_service_info` shells `sv status` | the bridge itself answering gRPC with a subprocess |
| `op-grpc-bridge/src/dbus_identity.rs:814` | spawns `dbus-daemon` | test-fixture bus; acceptable if it stays test-only (it is behind fixture code) |

(`plugin:netmaker` is *not* a bypass: `default_registry.rs:37 RETIRED_PLUGIN_IDS =
["compact_mcp", "factory", "netmaker"]` fail-closes it and `mutation_engine.rs:2377`
bails on invocation. It is dead weight — 906 lines that compile for nothing — not a
live surface. Corrected after the triage swarm, §2.11.)

Legitimate/out-of-scope subprocess users (host tooling, not plugins): `op-runit-systemctl`
(*is* the sv adapter), `op-network/bin/op-ovsbr0-setup`, `op-blob/btrfs.rs`
(`mkfs.btrfs`), `op-snowball`, `op-cache`, `op-gateway`, `op-deployment` (btrfs),
`op-identity` (`wg`, `gcloud`), `op-http/tls.rs` (`openssl` self-signed), `op-llm`,
`op-tools` builtin shell/git/rg tools (that *is* their job), `op-agents` analysis agents.

### 2.5 SQLite beyond the documented debt

AGENTS.md lists four `op-cache` files as the only `rusqlite` migration debt.
`op-introspection/src/indexer.rs` is a fifth — an FTS5 indexer with its own
`rusqlite::Connection`, compiled and consumed by `op-tools/builtin/dbus_search_tool.rs`
via `IndexerManager`. Either add it to the debt list or migrate it to Cozo (Cozo has
FTS indices since 0.7).

### 2.6 Retired designs still registered / documented

* `plugin:netmaker` still compiles and self-registers (`state_plugins/netmaker.rs:906`)
  although Netmaker is retired; it is fail-closed by `RETIRED_PLUGIN_IDS` so it cannot
  be called, but 906 lines of retired plugin ship in the verified source, and its
  `"s6-systemctl … unavailable"` error string is doubly stale. Delete the file.
* `op-http/src/lib.rs` describes a "unified-server binary" that composes
  `op_mcp::create_router()`, `op_chat::create_router()`, `op_web::create_router()`.
  No such binary exists, `op-web` does not depend on `op-http`, and the `op_chat`/
  `op_mcp` routers it names are orphaned files (§2.1). `op-http` is consumed only by
  `op-tools` and `op-agents` for `RouterBuilder` types.
* `op-cognitive-mcp/src/live_schema.rs`, `op-plugins/src/state_plugins/systemd*.rs`,
  `op-mcp/src/tools/systemd.rs` — retired, and orphaned (§2.1).
* `op-mcp/src/tools/shell.rs` allow-lists `systemctl`, `journalctl` (orphaned).

### 2.7 Crates with no consumer and no deployment

| crate | status | evidence |
|---|---|---|
| op-deployment | dead | no bin, no dependents, only `image_manager.rs` |
| op-gateway | dead | no bin, no dependents; WireGuard-auth MCP gateway superseded by SID1/op-grpc-bridge |
| op-workflows | dead-ish | only the root `op-dbus` package (which has no binary) depends on it |
| op-jsonrpc | dead-ish | two dependents, both unused (§2.2); 555 of 1,767 LOC orphaned |
| op-compliance | dead-ish | one dependent (op-identity), unused |
| op-dbus (root) | odd | 19 internal + 36 external deps carried to build one `examples/ovs_native_rust.rs` |
| op-assistant-grpc, op-services, op-gemma, op-xray-daemon | daemons without a runit definition | binaries exist; nothing in `deploy/runit` runs them. AGENTS.md says a binary target is inventory, not deployment — but then they should be listed in `retired-binaries` or get a `run` |

### 2.8 Test distribution

`#[test]` counts per crate (src + tests). Zero: op-gemma (2,254 LOC), op-gateway,
op-http, op-dbus-cli, op-execution-tracker, op-runit-systemctl, op-dynamic-loader,
op-dbus-model. Thin: op-introspection 2, op-workflows 3, op-services 1, op-deployment 1,
op-xray-daemon 2. Heavy: op-grpc-bridge 614 (+21 integration files), op-plugins 510,
op-state-store 74, op-identity 63.

`op-runit-systemctl` is an *enabled production service* with no tests at all.

### 2.9 Production `panic!` paths (outside `#[cfg(test)]`)

Only 11, and most are fail-closed schema invariants evaluated at startup
(`op-plugins/schemars_adapter.rs:51`, `large_language_model.rs:114/122`,
`web_ui_methods.rs:70`, `op-grpc-bridge/human_principal_dispatch.rs:957`,
`op-tools` double-init guards). Acceptable. `op-mcp-aggregator/aggregator.rs:756
unimplemented!("Use Arc<Aggregator> directly")` is a live trait impl that will abort
if reached.

### 2.10 `.unwrap()` / `.expect()` density (non-test lines, approximate)

op-grpc-bridge 1,351 · op-plugins 1,089 · op-cache 169 · op-state-store 122 ·
op-cozo-store 104 · op-network 97. For the two hot crates this is ~1 per 60 LOC; the
bridge is the authenticated public edge, so a sampling review of the `unwrap`s in
`grpc_server.rs`, `mutation_engine.rs`, `mcp_frontend.rs` is worth a dedicated pass.

### 2.11 Verification gate and retired-infrastructure sweep (2026-09-19)

The fresh export passes the full `/verify` gate on its own, with none of the ~7,000
excluded files:

| gate | result |
|---|---|
| `cargo build --workspace` | ✅ 37 libs, 32 bins |
| `cargo fmt --all -- --check` | ✅ |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | ✅ |
| `cargo test --workspace --all-targets --all-features --no-fail-fast` | ✅ **110 suites · 1,862 passed · 0 failed · 9 ignored** |

Identical to the gate122 numbers recorded in SIGNALS.md. The 9 ignored tests are all
environment-gated (multi-node NUMA ×3, OVS binaries/root ×4, live bridge ×2). No
`panicked at` / `WARN` / `ERROR` noise inside passing tests in any suite; 19 tests in
`op-grpc-bridge` exceed cargo's 60 s slow-test notice.

Twelve read-only triage workers then swept every crate in the run for tokens the owner
has retired: **TCP `50051` in any form (only `:8090` exists)**, `s6`/`service6`/
`systemctl` (outside the runit adapter), `plugin_schema.dat`/`live-schema.json`, OIB1
identity blobs, waypipe/netmaker/netclient/compact_mcp/op-grpc-adapters/op-projection,
and the retired UIs. Doc-comment mentions, `#[cfg(test)]` fixtures, absence-guard tests
and text that *forbids* the token were discarded. What remains is **live**:

**Port 50051 still a runtime default** (all should become the `:8090` door or be deleted):

| file:line | what |
|---|---|
| `op-plugins/src/state_plugins/tched_router.rs:856` | `TCHED_ROUTER_GRPC_TARGET` fallback `http://10.200.0.2:50051` — the router plugin's default upstream |
| `op-chat/src/orchestration/grpc_pool.rs:118-128` | `AGENT_PORTS` 50051–50060, read at `:326` |
| `op-chat/src/system_prompt.rs:36,266,271,310` | `FIXED_BASE_PROMPT` tells the LLM `10.200.0.x:50051` is the current gRPC topology |
| `op-assistant-grpc/src/server.rs:33`, `incus.rs:24`, `transport.rs:22` | `DEFAULT_GRPC_PORT`/`DEFAULT_GRPC_ENDPOINT`/`DEFAULT_RPC_ENDPOINT` = 50051; module docs assert it is current |
| `op-web/src/bin/opdbus.rs:27` | `DEFAULT_GRPC_PORT = 50051` — the `opdbus` binary *is* the retired separate listener |
| `op-mcp/src/grpc/client.rs:30`, `grpc/server.rs:39` | `[::1]:50051` in `Default` impls |
| `op-cache/src/grpc/server.rs:34` | `[::1]:50051` (no caller in workspace) |
| `op-gateway/src/mcp_gateway.rs:95` | `grpc://localhost:50051` routing endpoint (dead crate) |
| `op-grpc-bridge/tests/live_reflection_probe.rs:19` | `#[ignore = "requires a running bridge on 127.0.0.1:50051"]` — the ignore reason itself is stale; the sibling test at `:106` correctly uses `:8090` |
| `op-grpc-bridge/src/grpc_server.rs:2680,4816` | fallback `10.200.0.1` controller/management IPs |

**Chatbot told to use tools/infra that do not exist:**

| file:line | what |
|---|---|
| `op-web/src/orchestrator/tools.rs:134,171` | compact-mode prompt instructs `s6_start_service`/`s6_stop_service`; only `sv_*` tools exist (`op-tools/src/builtin/sv.rs`) |
| `op-chat/src/system_prompt.rs:229-312` | Netmaker/netclient mesh and `op-projection` described as live; s6 lines; `/usr/local/bin/systemctl` shim documented as usable |
| `op-agents/src/unified/prompts.rs:342-343` | `SYSTEMD_EXPERT` persona teaches `systemctl` |
| `op-llm/src/headless_oauth.rs:254` | runtime error tells the operator to `sudo systemctl start antigravity-display` |

**`systemctl` allowed rather than blocked:** `op-tools/src/validation.rs:88`
(`command_whitelist`), `op-tools/src/security.rs:246` (`NATIVE_ALTERNATIVES`, "don't
BLOCK"), `op-agents/src/agents/operations/devops_troubleshooter.rs:23` (profile
allow-list). Contrast: `op-chat/chat_loop.rs:186`, `nl_admin.rs:239`, `op-web
parsing.rs:158`, `routes/admin.rs:220`, `anti_hallucination.rs` all *forbid* it — the
policy is enforced in some layers and permitted in others.

**Netmaker still load-bearing in names/defaults:**

| file:line | what |
|---|---|
| `op-identity/src/oracle_assertion.rs:79,100,151,283` + `op-grpc-bridge/src/oracle_assertion{,_client}.rs` | `netmaker_inner_ip` is a wire-format field of `OracleIdentityAssertion` used for peer-IP validation — renaming changes the OIA1 format, so this is a deliberate decision, not a cleanup |
| `op-core/src/security.rs:205,238,271` | `NetworkConfig.auto_netmaker` / `trust_netmaker()` |
| `op-state-store/src/disaster_recovery.rs:281`, `op-state/src/schema_validator.rs:227,233`, `op-tools/src/discovery/sources/plugin.rs:35`, `op-tools/src/builtin/openflow_tools.rs:313`, `op-plugins/src/state_plugins/net.rs:400-465` | netmaker in default plugin lists / dependency maps / port-role tags |
| `op-plugins/src/state_plugins/netmaker.rs` (906 LOC) | compiled, self-registers, fail-closed by `RETIRED_PLUGIN_IDS` — delete |

**Other:** `op-state-store/src/plugin_schema.rs:1435` `canonical_name()` maps
`"systemd" | "dinit"` → `"s6"` (pre-runit leftover, reachable);
`op-network/src/bin/op-ovsbr0-setup.rs:27-29,244,780,794` documents `service6` as "the
sanctioned wrapper" in help text; `op-web/src/privacy_network.rs:3,6` header claims
"Artix + s6" is the current architecture; `op-grpc-bridge/src/bin/dump-protos.rs:35`
defaults its output to the retired `operation-dashboard-ui-07/proto`;
`op-gemma/src/ui_gallery.rs` (17 hits) bakes 50051/netmaker/s6 into generated demo
dashboard content; `op-tools/src/builtin/sv.rs:22` cites a crate named
`op-s6-systemctl` that does not exist.

Hits inside orphaned files (`op-chat/grpc_client.rs`, `op-chat/tool_loader.rs`,
`op-mcp/tools/shell.rs`, `op-web/system_prompt_loader.rs`, `op-plugins/privacy_router.rs`,
`op-mcp-aggregator/unused/`, the `.bak` files) are already covered by §2.1 — deleting
those files removes them.

Per-slice worker reports are preserved in the session scratchpad; the consolidated
list above is the deliverable.

**Applied 2026-09-19 (owner decisions: op-gemma retired, s6 is runit/sv, Netmaker
gone, 50051 is `:8090`):** `crates/op-gemma` deleted and listed in
`retired-binaries` (36 member crates now); `state_plugins/netmaker.rs` deleted with
its re-exports, `auto_netmaker`/`trust_netmaker` removed from `op-core`, netmaker
dropped from default plugin lists / dependency map / `container_mesh` template /
`net.rs` port roles, `netmaker_inner_ip` → `tunnel_inner_ip` (positional wire
format, unchanged on the wire); op-web compact prompt now names `sv_*` tools,
`canonical_name()` maps `systemd|dinit|s6|runit` → `service`, remaining s6 wording
rewritten; `tched_router` default target, `op-assistant-grpc`
`DEFAULT_GRPC_ENDPOINT`, `live_reflection_probe` and the op-chat `FIXED_BASE_PROMPT`
topology now say `:8090`. Gate after the change: 109 suites · 1,862 passed · 0
failed · 9 ignored, clippy and fmt clean. Left as-is on purpose: each component's
*own* listener port (`op-assistant-grpc` `DEFAULT_GRPC_PORT`, `op-mcp`/`op-cache`
gRPC defaults, `op-web/bin/opdbus.rs`, `op-chat` `AGENT_PORTS`) — retiring those
means retiring those listeners.

---

## 3. Crate-by-crate

Format: **purpose** · layer / node id · deps → (internal) · ← used by (file refs) ·
binaries · size (rs files / LOC / tests) · findings.

### Layer 0 — leaves

#### op-state-store  `crate:op-state-store`
Schema, in-memory state, event chain, canonical hashing, public-state projection.
The single most referenced type source in the workspace (`PluginSchema`,
`MethodDecl`, `FieldType`, `SideEffect`).
→ none internal · ← op-plugins (463 refs), op-grpc-bridge (145), op-blob (18),
op-web, op-state, op-plugin-lint, op-cognitive-mcp, op-llm, op-gemma, op-dbus-model
· 14 files / 7,904 LOC / 74 tests.
Findings: `subid_ui.rs` orphaned; four `.sql` files committed in `src/`;
`plugin_schema.rs` is 2,472 LOC and `event_chain.rs` 1,405 — both are the contract
everything else derives from, so splitting `plugin_schema.rs` into decl/validation/
canonical-json modules would ease review. Depends on `reqwest` and `jsonschema`
(`schema_validator`) — a leaf "types" crate pulling an HTTP client is a smell.

#### op-cozo-store  `crate:op-cozo-store`
Embedded CozoDB (RocksDB engine) owner: identity sleds/genesis, human principals,
runtime documents, control-plane chatbot episodes, generic `graph_node`/`graph_edge`,
audit log. Enforces private 0700 directory boundaries and distinct owners per domain.
→ none · ← op-grpc-bridge (70), op-plugins (26), op-cognitive-mcp, op-web, op-cache
· bin `identity-sled-bind` · 8 files / 5,854 LOC / 43 tests.
Findings: `lib.rs` is 2,546 LOC with ~25 domain APIs on one struct; the
`identity_transactions` / `runtime_document_transactions` / `episodes` splits are the
right direction — continue it for the wg/privacy/consumer-account sections. Imports
`op_identity::` in `lib.rs` **without** a Cargo dep (works only because it is inside a
doc string/comment — verify; if it is code it is the 11th undeclared import).
`WgSessionRecord`/`wg_sessions` relations remain although WireGuard is decoy-only —
classify per AGENTS.md before removing. The `op_identity::` mention in `lib.rs:139`
is a doc comment, not an undeclared dependency. This crate is where the crate graph now
lives; `load_crate_graph` example added.

#### op-execution-tracker  `crate:op-execution-tracker`
Execution ack/tracking/metrics (prometheus). → none · ← op-dynamic-loader, op-chat,
op-workflows, op-mcp · 6 files / 947 LOC / **0 tests**.
Findings: declared by op-core, op-plugins, op-tools and never used there (§2.2).
Real consumers are all L6–L11 orchestration code that is itself partly orphaned.

#### op-http  `crate:op-http`
axum/rustls server builder + middleware. → none · ← op-tools, op-agents (router types)
· 8 files / 1,719 LOC / **0 tests**.
Findings: lib doc describes a non-existent unified-server binary (§2.6); `health.rs`,
`metrics.rs`, `request_filters.rs` orphaned; `tls.rs` shells `openssl` to mint
self-signed certs (dev path; ensure not reachable in prod). Only `TlsConfig`/
`RouterBuilder` are actually used by others.

#### op-xray-daemon  `crate:op-xray-daemon`
D-Bus daemon for Xray lifecycle (`/org/opdbus/v1/plugins/xray`) + a commander client
lib. → none · ← op-grpc-bridge (2) · bin `op-xray-daemon` · 5 files / 683 LOC / 2
tests. Findings: no runit definition; OD-05 says it must run as a service for the
dynamic xray cutover. Good: no subprocess use, honours the `/etc/xray/xray_config.json`
invariant.

#### op-gateway  `crate:op-gateway`
WireGuard-pubkey MCP gateway + encrypted btrfs storage. → none · ← **nothing** ·
no bin · 5 files / 1,832 LOC / 0 tests. Findings: dead (§2.7); shells `btrfs`,
`dd`, `mount`, `df`. Predates SID1/SealedId. Retire or fold into op-identity.

#### op-deployment  `crate:op-deployment`
Container image manager. → none · ← **nothing** · 2 files / 474 LOC / 1 test.
Findings: dead (§2.7); AGENTS.md says deployment is btrfs send/receive with no script —
this crate is the script. Retire.

### Layer 1

#### op-core  `crate:op-core`
Config, errors, security levels, self-identity, runit constants, **projection SHM**
(the canonical `/dev/shm` state tree, transactions, private document publication).
→ op-execution-tracker (unused) · ← 23 crates (op-grpc-bridge 123 refs, op-plugins,
op-tools, op-web …) · 21 files / 5,365 LOC / 53 tests.
Findings: `connection.rs`, `message.rs` orphaned; `lib.rs.patch` committed; drop the
op-execution-tracker dep. `self_identity.rs` shells `git` (tooling, fine).
The projection/transaction code (`projection_transactions.rs` 719 LOC, tests 444)
is the MutationEngine's commit substrate and is well covered.

#### op-blob  `crate:op-blob`
schemars → `PluginSchema` → sealed zero-copy blob (`OPBLOB01`) → SHM catalog → btrfs
image; hand-encoded protobuf descriptors; the JSON-Schema→proto mapper the bridge
shares. Sole blob writer per AGENTS.md. → op-state-store · ← op-grpc-bridge (137),
op-web (22), op-plugins (19), op-cognitive-mcp, op-mcp, op-gemma · 15 files / 4,296
LOC / 32 tests (+4 integration files).
Findings: clean; `Cargo.toml` documents the `serde_json` BTreeMap-order dependency for
canonical JSON — good. `demo.rs` (251 LOC) panics on invalid subids — example code
kept in `src/`; move to `examples/`.

#### op-cache  `crate:op-cache`
btrfs subvolume cache, NUMA, agent registry, workstack/workflow orchestration, gRPC
services. → op-cozo-store · ← op-snowball (2) · 20 files / 9,581 LOC / 57 tests.
Findings: the four `rusqlite` files are the acknowledged debt; `btrfs_cache.rs:1201,
1237` run `bash -c`; `workflow_executor.rs:457` calls `taskset`. Only consumer is
op-snowball's `op_cache::` (2 refs) — most of this crate (agent registry, workstacks,
four gRPC services) has no live caller; verify against `enabled-services` before
investing in the SQLite migration.

### Layer 2

#### op-agents  `crate:op-agents`
Agent catalog/registry/schemas, D-Bus service, HTTP router, security profiles/sandbox,
unified persona registry. → op-core, op-http · ← op-web, op-plugins, op-mcp, op-tools,
op-cognitive-mcp, op-chat · bins `dbus-agent`, `gen-agent-defs`, `op-agent-manager` ·
144 files / 20,633 LOC / 36 tests.
Findings: 93 `Command::new` — the analysis/ops agents (`code_reviewer.rs`,
`security_auditor.rs`, …) shell `rg`, `tokei`, `semgrep`, `bandit`, `npm`, `cargo`;
that is their function but they run with the service's ambient privileges — see OD-39.
`generator/` (template + md parser, 1,150 LOC) orphaned. Prompts still teach
`systemctl` (`unified/prompts.rs:342`) on a runit host. 144 files for 36 tests.

#### op-network  `crate:op-network`
Pure-Rust OpenFlow, OVSDB JSON-RPC, OVS generic-netlink, rtnetlink, WireGuard kernel,
Proxmox client, `datapath_safe`. → op-core · ← op-grpc-bridge (40), op-plugins (66),
op-tools (27), op-web, op-mcp, op-chat · bins `op-of-controller` (runit: yes, not
enabled), `op-ovsbr0-setup`, `op-rtnetlink-init` · 24 files / 10,889 LOC / 56 tests.
Findings: six `.bak-*` snapshots committed in `src/` (§2.3); `op-ovsbr0-setup` shells
`ip`, `ovs-dpctl`, `sv` (boot tooling — acceptable, but it is the only place `ip link
del` on the bridge exists, so keep it out of any plugin path). `proxmox.rs` (645 LOC)
+ `reqwest`: no Proxmox on this host per SIGNALS — candidate for retirement.
Vendored `rovs-openflow` fork is the only `[patch]`.

#### op-snowball  `crate:op-snowball`
Streaming audit snowball, btrfs delta/replication, retention. → op-core (unused),
op-cache · ← op-plugins (6), op-cognitive-mcp (4), op-introspection, op-grpc-bridge ·
8 files / 3,416 LOC / 40 tests.
Findings: `replication.rs:248` spawns `ssh` for send/receive — AGENTS.md says
send/receive is an operator step, not service code; confirm this path is not reachable
from a plugin. `snowball.rs` 1,697 LOC.

#### op-identity  `crate:op-identity`
SealedId/SID1, session genesis, oracle assertion (OIA1), recovery, schema bridge, gcloud
auth, WireGuard identity helpers. → op-core, op-compliance (unused) · ← op-grpc-bridge
(87), op-web (10), op-plugins, op-cognitive-mcp, op-assistant-grpc, op-mcp, op-llm,
op-dbus-cli, op-cozo-store · bins `op-decoy-issuer`, `op-identity-headers` · 17 files /
3,976 LOC / 63 tests.
Findings: `wg.rs` is `include!("depreciated-wg.rs")` — a file whose name says it is
deprecated is still the implementation of `get_local_pubkey`/`get_peer_pubkey`; four
`wg` subprocess sites in `wireguard.rs`. Per the topology correction these belong to
the decoy (`op-decoy-issuer`), not the server — split the crate along that line.
`argon2`, `bip39`, `md5` deps: OD-38 removed the PSK path; check `argon2`/`md5` are
still needed (md5 is the EventChain head digest per SIGNALS, so probably yes).

#### op-runit-systemctl  `crate:op-runit-systemctl`
The D-Bus systemctl→sv adapter (`/org/opdbus/v1/plugins/runit/systemctl`). Enabled
production service. → op-core · ← nothing (D-Bus consumers) · bins
`op-runit-systemctl`, `svd` · 3 files / 925 LOC / **0 tests**.
Findings: correct place for `sv` subprocesses. Zero tests on an enabled service.
`dbus.rs:654` tails a log path chosen from fixed `/var/log/op-dbus/<svc>/current` /
`/var/log/<svc>/current` candidates — constrained, fine.

#### op-jsonrpc, op-compliance, op-dbus-model, op-gemma
* **op-jsonrpc** — JSON-RPC over UDS + OVSDB client. 7 files / 1,767 LOC / 7 tests;
  3 files orphaned; both dependents unused → dead-ish (§2.7).
* **op-compliance** — "The Law Firm" schema validators, 1 file / 327 LOC / 6 tests; sole
  dependent (op-identity) unused → dead-ish.
* **op-dbus-model** — 2 files / 113 LOC; only `PluginCatalog`/`CatalogDocument` used by
  `op-plugins/registry.rs`; drop unused op-core dep.
* **op-gemma** — routing brain binary (subid → xray/openflow routes), 2 files / 2,254
  LOC / **0 tests**, no runit definition, `ui_gallery.rs` 1,994 LOC. AGENTS.md says the
  name is historical and model-agnostic; the crate is not consumed by anything and not
  deployed.

### Layer 3

#### op-introspection  `crate:op-introspection`
D-Bus service discovery, XML→JSON, cache, FTS indexer, projection. → op-core,
op-snowball · ← op-tools (4), op-web, op-mcp, op-inspector, op-chat, op-dbus-cli(unused)
· 10 files / 4,121 LOC / **2 tests**.
Findings: undocumented `rusqlite` (§2.5); 2,395 LOC orphaned including a stray
`src/mod.rs` and `cpu_features.rs` (shells `modprobe`, `rdmsr`, `dmesg`); 99
`println!` in library code — the projection prints to stdout instead of `tracing`.

#### op-state  `crate:op-state`
`StatePlugin` trait, D-Bus plugin base/server, crypto, plugtree, schema validator,
authority enforcement. → op-core, op-state-store, op-jsonrpc/op-network/op-snowball
(all unused) · ← op-plugins (133), op-grpc-bridge (47), op-tools · 11 files / 2,143
LOC / 9 tests.
Findings: three unused heavyweight deps (§2.2) — removing `op-network` here cuts
netlink/reqwest out of the build path of every plugin; `manager.rs` orphaned (the
excised StateManager lib.rs says is gone — delete it); `authority.rs` shells `sv`
(§2.4); features `mcp = []` declared, never referenced.

### Layer 4–6

#### op-dbus-cli  `crate:op-dbus-cli`
`dbus-plugin-cli`: introspects the live plugin tree and generates subcommands.
→ op-core, op-identity, op-introspection (unused) · 4 files / 1,406 LOC / 0 tests.
Findings: forwards SID1 via `identity.rs` — good; no tests.

#### op-inspector  `crate:op-inspector`
Universal object inspector (Docker, gcloud, runit, XML/JSON). → op-core (unused),
op-introspection · ← op-tools, op-plugin-lint (optional feature) · 6 files / 3,567
LOC / 27 tests. Findings: `cli.rs` (1,002) and `datadump.rs` orphaned; shells
`docker`, `gcloud` (introspection tooling, fine).

#### op-tools  `crate:op-tools`
Tool registry, built-ins (D-Bus introspection, OVS, rtnetlink, shell, agent, gcloud,
anydesk…), validation, security levels, orchestration hooks. → op-core, op-network,
op-state, op-introspection, op-http, op-agents, op-inspector, op-execution-tracker
(unused) · ← op-chat (18), op-mcp (21), op-web (8), op-dynamic-loader, op-mcp-
aggregator, op-workflows · bin `op-packagekit-install` · 50 files / 20,606 LOC / 50
tests.
Findings: **8,260 LOC orphaned** — a whole parallel `builtin/` generation
(`dbus_tool.rs`, `shell_tool.rs`, `incus_tools.rs`, `lxc_tools.rs`, `procfs.rs`,
`self_tools.rs`, `openflow_tools.rs`, `ovsdb.rs`…) plus `builtin_old.rs`; two `mod.rs`
patch files. `builtin/anydesk.rs` (802 LOC) shells `anydesk` — remote-desktop control
as a chatbot tool deserves an explicit grant review (G-3/OD-40 scope). Security model
is "access level, not command level" (`lib.rs` doc) — consistent with OD-39's concern.

#### op-plugin-lint  `crate:op-plugin-lint`
Static audit of plugin sources against `PLUGIN-RENDER-CONTRACT.md`; repomix/CLI
introspection. → op-inspector (optional) · bin `op-plugin-lint` · 15 files / 5,008
LOC / 17 tests (+2 integration). Findings: `rust_emit.rs` orphaned and references
`op_state_store` which is not a dep — the Rust emitter was never finished; `binary.rs`
shells `ssh` to probe remote binaries (tooling). CI "enforces subid uniqueness" per the
grpc-expert skill — this crate is where that lives; make sure `/verify` runs it.

#### op-dynamic-loader  `crate:op-dynamic-loader`
LRU/execution-aware tool loading. → op-core (unused), op-tools, op-execution-tracker ·
← op-mcp (only from orphaned `tool_adapter.rs`) · 5 files / 388 LOC / 0 tests.
Findings: effectively dead — its only importer is an uncompiled file; op-plugins and
op-cognitive-mcp declare it without use.

### Layer 7 — the plugin crate

#### op-plugins  `crate:op-plugins`
The 69 D-Bus plugins (`state_plugins/`), self-registration via `inventory`, canonical
plugin-document persistence, `opblob` sealer CLI, tched_router contract + generated
config surface. → op-state-store (463 refs), op-state (133), op-network (66),
op-cozo-store (26), op-blob (19), op-cognitive-mcp, op-core, op-snowball, op-agents,
op-identity, op-dbus-model, op-dynamic-loader/op-execution-tracker (unused) ·
← op-grpc-bridge (283), op-cognitive-mcp, op-llm, op-workflows, op-services · bins
`opblob`, `op-mail-admin`, `gen_tched_router_config_surface` · 148 files / 114,922
LOC / 510 tests.
Findings:
* `tched_router_contract.rs` is 26,134 LOC (the imported upstream schema document —
  by design, regenerated through `--import-json`); `incus.rs` 3,748; `tched_router.rs`
  2,868; `openflow.rs` 1,872. Fine for generated/contract files; `incus.rs` and
  `net.rs` (1,789) are hand-written and would benefit from the `mail_server/` style
  directory split already used elsewhere.
* Control-plane bypass: `service_def.rs` shells `sv` (§2.4) while
  `state_plugins/service/methods.rs` correctly targets the D-Bus runit door — two
  implementations of the same thing, one of them forbidden.
* `plugin:netmaker` still compiled, fail-closed by `RETIRED_PLUGIN_IDS` (§2.6) — delete.
  `wireguard`, `wg_opdbus`, `wgcf` plugins present — classify decoy vs server before
  touching (owner instruction).
* 3,759 LOC orphaned incl. `privacy_router.rs` (1,558) and `host_runtime.rs` (828) —
  both contain `inventory::submit!` registrations, so the `privacy_router` and
  `host_runtime` plugins are **phantoms**: they appear in a source grep (and in older
  method inventories) but are not in the compiled registry. `privacy_routes` (declared)
  is the live one.
* `plugin_scaffold.rs.template` and `openflow.rs.bak-*` in `state_plugins/` (§2.3).
* `unsafe`: 14 references (memmap/SHM zero-copy) — expected for this crate.
* Panics are startup schema invariants (fail-closed) — good.

### Layer 8

#### op-llm  `crate:op-llm`
Multi-provider LLM clients (Antigravity, Gemini, Anthropic, Perplexity, HF, gemini-cli,
pty bridge, headless OAuth, salad, openclaw). → op-plugins, op-state-store, op-identity
· ← op-web (15), op-chat (10), op-grpc-bridge (4) · 18 files / 8,845 LOC / 25 tests.
Findings: 19 `unsafe` references (`pty_bridge.rs`) — review; `antigravity_replay.rs`
orphaned; lib doc still instructs `sudo systemctl start antigravity-*` on a runit
host; `pty_bridge.rs:199` spawns arbitrary `command` — ensure it is only reachable
through an authenticated grant. AGENTS.md says the LLM schema is owned by the
`tched_router` plugin and embedded here (`schema.rs`) — consistent.

#### op-mcp  `crate:op-mcp`
MCP protocol/tool-runtime library (compact/agents/full/blob-schema modes, transports).
→ op-tools (21), op-core, op-blob, op-network, op-agents, op-identity, op-http,
op-introspection; op-plugins/op-state/op-state-store (unused) · ← op-cognitive-mcp
(11), op-grpc-bridge, op-http · 39 files / 11,003 LOC / 40 tests.
Findings: **4,378 LOC orphaned** — the whole standalone listener (`http_server.rs`,
`router.rs`, `sse.rs`, `request_*`, `tools/*`) that the lib doc says no longer exists
("the crate owns no production listener binary"). Delete it; it is the retired second
MCP door AGENTS.md forbids. `external_client.rs:255` / `http_server.rs:376` spawn
configured MCP commands (upstream stdio servers — legitimate, but only the compiled one
matters). 12 `unsafe` references (`transport/`) — review. The `grpc` feature gate on
`pub mod grpc` is real (`#[cfg(feature = "grpc")]`).

#### op-mcp-aggregator  `crate:op-mcp-aggregator`
Upstream MCP aggregator with compact mode/profiles. → op-core, op-tools, op-plugins
(unused) · ← op-web (1) · 9 files / 5,417 LOC / 33 tests. Findings: `unused/context.rs`
orphaned (self-labelled); `aggregator.rs:756` live `unimplemented!`; `groups.rs.patch`
committed. AGENTS.md: compact *mode* is in-process in op-web and op-grpc-bridge owns the
public MCP door — this crate's aggregator listener should not be started anywhere;
op-web's single reference should be checked to be the compact meta-tool set only.

#### op-services  `crate:op-services`
runit service manager with gRPC server; schema from `op_plugins::service_def`. →
op-plugins · ← nothing · bins `op-services`, `op-svctl` · 10 files / 1,029 LOC / 1 test.
Findings: no runit definition, not enabled, `sv` subprocess via `service_manager.rs:24`
(fine for a service manager, but it duplicates op-runit-systemctl's role). Two runit
managers (`op-runit-systemctl` on D-Bus, `op-services` on gRPC) is a second control
plane — retire this one or make it a client of the D-Bus door.

#### op-workflows  `crate:op-workflows`
PocketFlow-style engine with plugin/tool/D-Bus nodes. → op-core, op-plugins, op-tools,
op-execution-tracker · ← root only · 13 files / 2,651 LOC / 3 tests. Dead-ish (§2.7).

### Layer 9–10

#### op-cognitive-mcp  `crate:op-cognitive-mcp`
Embedded cognitive MCP runtime for the bridge: memory (Cozo + Qdrant), RAG pipeline,
chain vectors, NotebookLM tools, typed namespace tools, activity filter, quota. →
op-state-store (14), op-mcp (11), op-plugins, op-blob, op-snowball, op-identity,
op-cozo-store, op-agents; op-cache/op-core/op-dynamic-loader (unused) · ← op-assistant-
grpc (9), op-grpc-bridge, op-chat, op-plugins · bins `op-chain-vectors`, `op-cog-admin`,
`rag-ingest` · 32 files / 11,579 LOC / 50 tests.
Findings: `live_schema.rs` orphaned (retired design); `cozo_shuttle.rs` re-exports
`CozoGraphShuttle` — per SIGNALS 2026-09-14 the chatbot episodes needed a *separate*
Cozo owner from generic query tools; the graph shows both `op-cozo-store` (2 refs) and
`cozo` (direct) used here — confirm the generic Datalog/query tools cannot reach the
episode store. Retired `op-cognitive-mcp` runit service is correctly absent.

#### op-assistant-grpc  `crate:op-assistant-grpc`
gRPC gateway to the Assistant (agents/sessions/tasks/models/cron/soul/namespace/memory)
over D-Bus or HTTP-RPC. → op-cognitive-mcp (9), op-identity, op-core · ← nothing · bin
`op-assistant-grpc` · 21 files / 3,214 LOC / 21 tests (+1 integration).
Findings: AGENTS.md: "Never … point external clients at op-assistant-grpc"; no runit
definition; `auth.rs` still exports `wireguard_auth_interceptor` / `WireGuardIdentity`
(pre-SID1 naming). Candidate for retirement or explicit "internal only" marking.

#### op-grpc-bridge  `crate:op-grpc-bridge`
The control-plane edge: D-Bus↔gRPC bridge, MutationEngine, MCP frontend (`:8090`),
schema router, dynamic reflection from the SHM blob catalog, generated per-plugin
services (`build.rs` → `plugin_methods.proto`), identity dispatch, tched_router runtime,
generation jobs, cron, decoy network, EMQX hook provider. → op-plugins (283),
op-state-store (145), op-blob (137), op-core (123), op-identity (87), op-cozo-store
(70), op-state (47), op-network (40), op-llm, op-cognitive-mcp, op-mcp, op-xray-daemon,
op-snowball; op-jsonrpc (unused) · ← op-web (7), op-chat · bins `op-grpc-bridge`
(runit: enabled), `op-grants-materializer` (runit `opdbus-grants`), `op-decoy-network`,
`dump-protos` · 132 files / 76,487 LOC / 614 tests (+21 integration files) · 50
external deps.
Findings:
* Size: `grpc_server.rs` 5,960, `mutation_engine.rs` 5,474, `mcp_frontend.rs` 3,044,
  `schema_router.rs` 2,781, `tched_router_runtime.rs` 2,682 — seven files over 2,000
  LOC. `grpc_server.rs` mixes service registration, TLS, reflection, runit info and
  the generated `include!` of `plugin_method_routes.rs`; the generated include is the
  natural seam to split on.
* `grpc_server.rs:3012` `runit_service_info` shells `sv status` (§2.4) — should call
  the D-Bus runit door like the plugins do.
* Two `grpc_server.rs.bak-datapath-*` files committed (§2.3);
  `callable_reflection.rs`, `schema_passthrough.rs` orphaned.
* 1,351 unwrap/expect lines — the public authenticated edge; sample-review the
  dispatch paths (`*_dispatch.rs`) for `unwrap` on request-derived data.
* `proto/operation.proto.bak` next to `operation.proto`.
* Good: both reflection layers (static descriptor set + dynamic SHM hydration) are
  present as the skill describes; TLS wired; 21 integration test files.

### Layer 11–12 — user-facing

#### op-chat  `crate:op-chat`
Chat actor, forced-execution/hallucination pipeline, NL admin, session, system prompt,
orchestration gRPC pool + workstack executor, generated `op_chat.*` protos. → op-core
(21), op-tools (18), op-llm (10), op-network, op-cognitive-mcp, op-grpc-bridge,
op-execution-tracker, op-introspection, op-agents · ← op-web (12), op-mcp, op-http · bin
`list_tools_client` · 44 files / 29,056 LOC / 59 tests.
Findings: **7,829 LOC orphaned** (13 files: `tool_loader.rs` 2,222, `grpc_client.rs`,
`router.rs`, `chat_loop.rs`, `*_executor.rs`, `orchestration/{coordinator,executor,
workflows,workstacks,dbus_orchestrator}.rs`) plus `tool_loader.rs.copied`/`.stub`; 109
`#[allow(dead_code)]`/`unused` attributes — the highest in the workspace, i.e. the
compiled half is also carrying dead code. `orchestration/services/rust_pro.rs` shells
`cargo`/`rustc`/`rustfmt` (a code-agent service — grant-gate it). `chat_loop.rs:186`
and `nl_admin.rs:239` block-list `systemctl` text — consistent with runit policy.
Retired `op-chat` runit service correctly absent; the crate is now a library for
op-web.

#### op-web  `crate:op-web`
Unified web server (`:8080`): REST, WebSocket, SSE, static Vite UI, privacy routes,
tched_router routes, users/groups admin, control-plane chatbot orchestrator. →
op-blob (22), op-state-store (21), op-core (18), op-llm (15), op-chat (12), op-identity
(10), op-tools, op-grpc-bridge, op-network, op-agents, op-mcp-aggregator, op-cozo-store,
op-introspection; op-state (unused) · ← op-http (comment only) · bins `op-web-server`
(runit: enabled), `opdbus` · 82 files / 17,833 LOC / 55 tests.
Findings: 6 orphaned files incl. `handlers/auth_bridge.rs` (366 — an auth path that is
not compiled: confirm it is superseded, not accidentally dropped), `handlers/
websocket.rs`, `orchestrator/anti_hallucination.rs`; `chat_handler.rs.fix`,
`main.rs.patch` committed; `crates/op-web/ui/` still in-tree (AGENTS.md: deprecated
RustEmbed UI; it is outside the module tree but still ships in the source export). OD-39
(legacy `/api/chat` runs tools as nobody) is tracked on the board; `orchestrator/` is
where it lives.

### Root

#### op-dbus  `crate:op-dbus`
Root package: 19 internal + 36 external deps, no lib, no bin, one example
(`examples/ovs_native_rust.rs`). Findings: every `cargo build --workspace` resolves and
links this dependency set for an example; move the example into op-network and drop the
root `[package]`, or give the root package a purpose.

---

## 4. Suggested order of work

1. **Orphan sweep** (§2.1) — decide keep/delete per file, then add an unreachable-file
   check to `/verify`. Largest single reduction in review surface (~34 k LOC).
2. **Dependency hygiene** (§2.2, §2.7) — remove the 24 unused deps and retire
   op-deployment, op-gateway, op-workflows, op-jsonrpc, op-compliance, op-dynamic-loader
   unless someone claims them; shrink the root package.
3. **Control-plane bypasses** (§2.4) — route `service_def.rs`, `authority.rs`,
   `grpc_server.rs:3012` through `/org/opdbus/v1/plugins/runit/systemctl`; delete
   `netmaker.rs`.
3a. **Retired-infrastructure defaults** (§2.11) — replace every live `50051` default
   with the `:8090` door, fix the `s6_*` tool names in the op-web compact-mode prompt,
   rewrite `op-chat` `FIXED_BASE_PROMPT` topology, drop `systemctl` from tool
   allow-lists.
4. **Artifacts** (§2.3) — delete the 30 stray files, ignore the patterns.
5. **SQLite** (§2.5) — add `op-introspection/indexer.rs` to the debt list or migrate.
6. **Tests** (§2.8) — `op-runit-systemctl` first (enabled service, 0 tests).
7. Re-run `scripts/crate-graph.sh` after each step; the `meta:crate-graph` node records
   the commit so successive graphs can be diffed in Cozo.
