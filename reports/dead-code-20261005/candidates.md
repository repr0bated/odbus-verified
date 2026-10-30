# Scope 5 candidate inventory — 2026-10-05

Worker: Codex dc5 (`/root`); task `dead-code-20261005/dc5`. Source baseline: `3cf5c47d1dd3baec2522c5ddac03de5bba1c5523`, branch `feat/od-108-native-lookup`, shared uncommitted tree. **Report only; zero source deletions.** Locations below were checked against current source; concurrent changes can move lines.

## Compiler candidates (maximum 30)

**Zero compiler-dead/unused candidates.** The single delegated native scoped `cargo check` selecting all 29 remaining packages passed exit 0 in 2m 00s, with zero compiler diagnostics. JSON receipt has 891 messages and `build-finished.success=true`; stderr ends with the successful dev-profile completion. Receipt: `/tmp/opencode/dc5-check.jsonl`, stderr `/tmp/opencode/dc5-check.stderr`. The capped compiler list is empty; suppressed-source candidates below are not represented as emitted compiler warnings.

| Crate | Item | File:line | Proof type | Action |
|---|---|---|---|---|
| All 29 selected scope packages | No compiler dead/unused diagnostic | — | a absent | No warning-backed deletion |

## Evidence rules and graph cross-check

- `a`: a compiler `dead_code`/`unused` diagnostic in the scoped check. Warnings from dependencies are reported separately from this scope.
- `b`: explicit `#[allow(dead_code)]` plus zero references outside the declaration across `crates/`, `docs/`, and the negative-topology gate list. A declaration-only symbol match is counted as zero references. This is eligibility evidence, not permission to remove a reachable public API.
- `b failed`: suppression exists, but callers, construction, exports, implementations, tests, or unresolved same-name references prevent zero-reference proof.
- `b partial`: the whole module/type is suppressed; no claim that every contained item is dead.

Started at mounted `/run/mount/3tchedFS` (catalog generation 1083). The projected Cozo object exists. `zcall cozo list_relations` failed before plugin admission: `Failed to connect to system scope bus via local transport: Operation not permitted`. The caller is this dc5 workspace process; no plugin grant name was returned because transport failed first. No alternate identity or transport was used. Used authorized file fallback: `docs/crate-graph/crate-graph.cozo`, `.json` (1267 nodes / 2940 edges), and `/tmp/opencode/system-graph-snapshot.json` (1246 / 2939, source commit above, generated 2026-10-05T02:17:01Z, dirty source).

Reviewed `depends_on`, `imports`, `contains`, and `provides_bin`; module membership is present in both graph formats. The graphs are crate/module inventories, not symbol call graphs or proof of deployment. Relevant reverse edges agree in committed and fresh projections:

| Crate | Reverse `depends_on` consumers |
|---|---|
| op-inspector | op-dbus, op-plugin-lint, op-tools |
| op-llm | op-chat, op-grpc-bridge, op-web |
| op-mcp | op-cognitive-mcp, op-dbus, op-grpc-bridge |
| op-network | op-dbus, op-grpc-bridge, op-plugins, op-tools, op-web |
| op-state | op-dbus, op-grpc-bridge, op-plugins |
| op-workflows | op-dbus |
| op-gateway | No crate reverse edge found; absence alone does not prove an item dead |

Public APIs with unresolved reverse consumers are **kept**. Private declaration-only candidates are recorded for human review because this is the report-only worker. No whole crate, module, binary, schema, registration, fixture, identity lifecycle operation, or service inventory is proposed for removal.

## Retained suppressed-item audit (not compiler warnings)

This table records every explicitly suppressed candidate investigated, including failed proof. It is separate from the capped compiler list. `candidate` means needs human review; `kept` means reachability/proof blocks removal. Nothing in this table was deleted.

| Crate | Item | File:line | Proof type | Action and evidence |
|---|---|---|---|---|
| op-llm | GOOGLE_USERINFO_URL | crates/op-llm/src/headless_oauth.rs:30 | b | candidate — private constant; declaration-only whole-tree symbol search; parent module has reverse consumers, but they cannot name this private constant |
| op-llm | CachedToken.loaded_at | crates/op-llm/src/headless_oauth.rs:93 | b failed | kept — initialized at headless_oauth.rs:223; no zero-reference proof |
| op-llm | GeminiCliProvider::setup_gcloud_env | crates/op-llm/src/gemini_cli.rs:62 | b | candidate — private method; declaration-only search; no deletion in report-only scope |
| op-inspector | IntrospectiveGadget::parse_docker_top | crates/op-inspector/src/introspective_gadget.rs:456 | b | candidate — private method; declaration-only search; reverse consumers use public exported inspector APIs |
| op-inspector | IntrospectiveGadget::extract_container_ports | crates/op-inspector/src/introspective_gadget.rs:488 | b | candidate — private method; declaration-only search |
| op-inspector | IntrospectiveGadget::extract_container_env | crates/op-inspector/src/introspective_gadget.rs:518 | b | candidate — private method; declaration-only search |
| op-inspector | IntrospectiveGadget::extract_container_labels | crates/op-inspector/src/introspective_gadget.rs:537 | b | candidate — private method; declaration-only search |
| op-workflows | Orchestrator.plugin_catalog | crates/op-workflows/src/orchestrator.rs:241 | b failed | kept — constructor parameter and stored field at :252/:261; catalog ownership comment is not deadness evidence |
| op-network | flush_addresses | crates/op-network/src/rtnetlink.rs:346 | b failed | kept — production callers op-tools/src/builtin/rtnetlink_tools.rs:561 and op-network/src/plugin.rs:385 |
| op-network | link_down | crates/op-network/src/rtnetlink.rs:404 | b failed | kept — callers in op-tools, op-network plugin/binary, and op-plugins rtnetlink; reverse graph confirms consumers |
| op-network | build_flow_mod_delete_all | crates/op-network/src/controller.rs:253 | b failed | kept — protected unit-test caller controller.rs:1243; suppression explicitly preserves wire-encoding coverage |
| op-gateway | WireGuardAuthManager::load_or_generate_master_key | crates/op-gateway/src/wireguard_auth.rs:589 | b failed / uncertain | kept — same-name live method/call in encrypted_storage.rs:319/:137; broad rg is not zero and receiver-level disambiguation has not established proof |
| op-mcp | McpServer | crates/op-mcp/src/server.rs:141 | b failed | kept — exported at lib.rs:47; handler impl transport/mod.rs:37; constructed by grpc/service.rs:156/:289 |
| op-mcp | ClientInfo | crates/op-mcp/src/server.rs:153 | b failed | kept — field type at :146, initialized at :239; similarly named pinned proto type is not a removal target |
| op-mcp | EnvAllowListValidator::new | crates/op-mcp/src/transport/http.rs:74 | b failed | kept — protected test callers :685/:732/:743; authorization remains deny-all |
| op-mcp | is_wireguard_pubkey_shape | crates/op-mcp/src/transport/http.rs:102 | b failed | kept — referenced by is_wireguard_auth_token_shape at :119 |
| op-mcp | is_wireguard_session_id_shape | crates/op-mcp/src/transport/http.rs:111 | b failed | kept — referenced by is_wireguard_auth_token_shape at :119 |
| op-mcp | is_wireguard_auth_token_shape | crates/op-mcp/src/transport/http.rs:118 | b failed | kept — protected test callers :720/:723/:726/:727; not used as an authorization decision |
| op-mcp | Session | crates/op-mcp/src/grpc/service.rs:35 | b failed | kept — sessions map :84 and construction :131; type-level suppression does not justify deleting used fields |
| op-mcp | McpGrpcService | crates/op-mcp/src/grpc/service.rs:82 | b failed | kept — generated trait impl :266, grpc/server.rs constructors :140/:150/:160/:279 and public re-export grpc/mod.rs:28 |
| op-mcp | McpGrpcService::start_session_agents | crates/op-mcp/src/grpc/service.rs:129 | b | candidate — private method; declaration-only search; feature-gated compatibility service retained |
| op-mcp | McpGrpcService::emit_event | crates/op-mcp/src/grpc/service.rs:169 | b | candidate — private method; declaration-only search; service and broadcast subscription path retained |
| op-mcp | run_grpc_server | crates/op-mcp/src/grpc/server.rs:265 | b failed / uncertain | kept — public feature-gated helper; many same-name functions/callers in other crates; unresolved reverse consumers; no zero-reference proof |
| op-mcp | run_grpc_server_lightweight | crates/op-mcp/src/grpc/server.rs:272 | b; graph blocks deletion | kept — declaration-only symbol search, but public grpc module and op-mcp reverse consumers cannot be ruled out; compatibility retirement needs human decision |
| op-state | PlugTree | crates/op-state/src/plugtree.rs:26 | b failed | kept — public trait re-exported at lib.rs:31/:45; async_trait expansion; unresolved public consumers |
| op-state | extract_pluglets | crates/op-state/src/plugtree.rs:48 | b failed | kept — helper caller :64 and protected test :92 |
| op-state | find_pluglet_by_id | crates/op-state/src/plugtree.rs:58 | b failed | kept — protected test callers :105/:109 and public module |
| op-state | StatePlugin::version / verify_state / rollback / capabilities | crates/op-state/src/plugin.rs:146,166,173,177 | b failed | kept — public trait implemented throughout op-plugins, including openflow.rs:933, tched_router.rs:1725, incus.rs:976; trait and macro reachability preserved |
| op-state | DbusStatePluginBase | crates/op-state/src/dbus_plugin_base.rs:24 | b partial | kept — public async trait; blanket suppression is not per-method zero-reference proof; unresolved op-state reverse consumers |
| op-state | json_to_zvariant | crates/op-state/src/dbus_plugin_base.rs:243 | b; graph blocks deletion | kept — declaration-only symbol search; public conversion API in exported module, reverse consumers not ruled out |
| op-state | zvariant_to_json | crates/op-state/src/dbus_plugin_base.rs:263 | b failed | kept — production callers :63/:109, recursive conversion, tests :325 onward; docs/architecture-flow.md:432 |
| op-state | crypto module | crates/op-state/src/crypto.rs:1 | b partial | kept — module-wide allow; StateEncryption used by serialization functions :226/:243/:259; public module lib.rs:18; no item-by-item deadness claim |
| op-state | plugin_workflow module | crates/op-state/src/plugin_workflow.rs:2 | b partial | kept — module-wide allow; PluginWorkflowState participates in ProcessState impl :35 and node/flow code :120/:211/:306; public module |
| op-plugin-lint | emitted dead_code attribute | crates/op-plugin-lint/src/rust_emit.rs:37 | not a/b | kept — generated Rust text inside emitter, not a suppressed Rust declaration; do not misclassify string match as compiler evidence |

## Scope coverage

Artifact coverage independently confirms all 28 selected member crates. The command included `-p op-dbus`, but the JSON contains no root-package artifact and this checkout has no root `src/` directory. Overall invocation succeeded; compilation of a root target is **unproven**, and no root target is fabricated. The 29-package selection must not be read as 29 compiled crate artifacts.

Every remaining declared workspace package is included in the single delegated scoped compiler command: op-services, op-gateway, op-core, op-introspection, op-http, op-state, op-blob, op-jsonrpc, op-llm, op-network, op-inspector, op-plugin-lint, op-workflows, op-snowball, op-deployment, op-mcp, op-mcp-aggregator, op-identity, op-execution-tracker, op-dynamic-loader, op-cognitive-mcp, op-cozo-store, op-dbus-model, op-compliance, op-assistant-grpc, op-runit-systemctl, op-xray-daemon, op-dbus-cli, and root op-dbus.

Scopes 1–4 retain ownership of op-grpc-bridge; op-web/op-chat; op-plugins/op-cache/op-state-store; op-tools/op-agents/op-chat-responder/threetched-fs. Dependency compilation may emit their diagnostics, but dc5 does not edit them.

`op-ml` is explicitly commented out of workspace membership (Cargo.toml:24) and its directory is absent in this checkout. `op-binaries` also has no crate directory or workspace declaration. These names are not evidence of buildable workspace packages, so neither is fabricated into a Cargo command. No manifest changed.

Source suppression scans found no explicit dead-code declaration in the other listed scope packages. This statement is not a claim of whole-program liveness. Default-feature `cargo check` does not prove all-feature/test/macro consumer coverage. Coordinator owns final `/verify`; protected fixtures and existing shared changes remain unchanged.
