# dc3 — Scope 3 dead-code audit, 2026-10-05

Worker: Codex (GPT), task `dead-code-20261005/dc3`. Shared, uncommitted branch `feat/od-108-native-lookup`, HEAD `3cf5c47d1dd3baec2522c5ddac03de5bba1c5523`. Scope: `op-plugins`, `op-cache`, `op-state-store`. Mandatory four skills and identity-storage lifecycle correction loaded. Existing `workflows_plugin.rs` changes belong to the orchestration worker and were preserved.

## Result

Deleted five private, explicitly suppressed OpenFlow text helpers: **197 lines**, in `crates/op-plugins/src/state_plugins/openflow.rs` only. No schema declaration, public type, trait implementation, registration, test fixture, manifest, proto, inventory pin or deployment input changed. `op-cache` and `op-state-store` source unchanged.

## Proof and graph cross-check

Proof **b**, not compiler-warning proof: each removed method had its own `#[allow(dead_code)]`. Searches used `rg -n '\b(parse_flows|parse_flow_line|parse_actions|flow_to_string|action_to_string)\b' crates docs`; the negative-gates file is included under `crates` and was inspected separately. Initially the two roots had only their declarations. Delete roots, search again; then delete newly unreferenced callees, search again; then delete the final newly unreferenced callee. No references remain. Evidence is `/tmp/opencode/dc3/openflow-{before,after-roots,after-middle,final}-refs.txt`; original source `/tmp/opencode/dc3/openflow-before.rs`.

| File (original lines) | Decision | Proof |
|---|---|---|
| `op-plugins/src/state_plugins/openflow.rs:373–393` `parse_flows` | deleted | b: explicit suppression; zero references outside declaration before any removal |
| `op-plugins/src/state_plugins/openflow.rs:485–527` `flow_to_string` | deleted | b: explicit suppression; zero references outside declaration before any removal |
| `op-plugins/src/state_plugins/openflow.rs:394–456` `parse_flow_line` | deleted | b: explicit suppression; its only caller was removed `parse_flows`; after root deletion only declaration remained |
| `op-plugins/src/state_plugins/openflow.rs:528–568` `action_to_string` | deleted | b: explicit suppression; its only caller was removed `flow_to_string`; after root deletion only declaration remained |
| `op-plugins/src/state_plugins/openflow.rs:457–484` `parse_actions` | deleted | b: explicit suppression; its only two calls were in removed `parse_flow_line`; after middle deletion only declaration remained |

Live Cozo was unavailable. Began at mounted `/run/mount/3tchedFS/plugins/cozo`, read declared `list_relations` operation (`cap.data.cozo.relations.list@v1`), then the user-authorized `zcall cozo list_relations -a '{}'` returned **`Failed to connect to system scope bus via local transport: Operation not permitted`**. Caller: existing local worker identity, no alternate identity or transport used. This is a transport denial; a missing grant was not established. No live query success is claimed.

Fallback authorities, read before deleting:

- `docs/crate-graph/crate-graph.cozo`: OpenFlow module at line 982; contains edge at 2735; registered plugin edge at 2838; defines/imports at 4061–4064. SHA256 `672ddf7f6eecfd777ceaee3bcfd334246fd950619a8000379c0a02c7c994af5b`.
- `docs/crate-graph/crate-graph.json`: 1267 nodes / 2940 edges. SHA256 `3de03935fe8ce697c269f52d1269861394ff52e7a1c6ac50b98bfe172c1c9b50`.
- `/tmp/opencode/system-graph-snapshot.json`: fresh dirty-source projection, namespace `source:op-dbus`, generated `2026-10-05T02:17:01Z`, 1246 nodes / 2939 edges. SHA256 `23c782cf5d3ae2e554f4db210b2a0d452b3a00641152c72d69b650abefa64bfd`.

Both JSON sources retain `crate:op-plugins contains mod:crates/op-plugins/src/state_plugins/openflow.rs` and plugin registration/definition. The graphs are source inventory, not deployed proof and not function-level reachability. `op-plugins` reverse crate consumers: root `op-dbus`, `op-cognitive-mcp`, `op-grpc-bridge`, `op-llm`, `op-services`, `op-workflows`. Inspected import edges, including bridge OpenFlow methods/tests, and generation inputs `op-plugins/build.rs` / `op-grpc-bridge/build.rs`: they consume public types/schema/registry, not private parser/formatter methods. Rust method privacy and zero references rule out these reverse consumers for the deleted helpers only. Module, public types, schema and registration remain. Graph extraction receipts: `/tmp/opencode/dc3/{committed,fresh}-graph-evidence.json`.

`op-cache` reverse consumers root `op-dbus` and `op-snowball` remain unresolved for unused public APIs. `op-state-store` reverse consumers include op-blob, op-cognitive-mcp, op-compliance, root, op-dbus-model, bridge, op-llm, op-plugins, op-state, op-web and threetched-fs. No crate/module deletions justified.

## Retained candidates — original file:line and proof status

Paths below are relative to `crates/`. Definition/initialization references count as references; an unused read is not sufficient proof b. Public or macro-reachable symbols were kept.

| File:line / candidate | Decision and proof status |
|---|---|
| `op-plugins/src/state_plugins/net.rs:223` `snowball_sender` | kept: explicit suppression, but constructor writes at 231/240; b fails |
| `op-plugins/src/state_plugins/net.rs:235` `with_snowball_sender` | kept: public API; same symbol also in web_ui; graph reverse consumers cannot be ruled out |
| `op-plugins/src/state_plugins/net.rs:245` `validate_interface_config` | candidate (needs human): no symbol callers; suppression inherited from impl at 227; public API with unresolved graph reverse consumers |
| `op-plugins/src/state_plugins/net.rs:251` `check_ovs_available` | candidate (needs human): no callers; inherited impl suppression; public API with unresolved reverse consumers |
| `op-plugins/src/state_plugins/net.rs:263` `query_current_state_dbus` | candidate (needs human): no callers; inherited impl suppression; public API with unresolved reverse consumers |
| `op-plugins/src/state_plugins/net.rs:276` `parse_ipv4_config` | candidate (needs human): private and no symbol callers, but inherited impl suppression only, no item-level suppression or compiler warning; retained under literal evidence bar |
| `op-plugins/src/state_plugins/net.rs:315` `parse_cidr` | kept: referenced; other impl methods also have source callers/trait/dispatch reachability |
| `op-plugins/src/state_plugins/web_ui.rs:318` `snowball_sender` | kept: explicit suppression but constructor writes at 328/339; b fails |
| `op-plugins/src/state_plugins/full_system.rs:240` `snowball_sender` | kept: explicit suppression but construction assigns field; b fails |
| `op-plugins/src/state_plugins/schemars_adapter.rs:703,708,713` Leaf/Job/Workspace | kept: protected cfg(test) fixtures; schema_for!/derive reachability |
| `op-plugins/src/state_plugins/schemars_adapter.rs:755,761` Record/Workspace | kept: protected cfg(test) fixtures; schemars derivation reachability |
| `op-plugins/src/state_plugins/keyring.rs:91` `get_collection_info` | candidate (needs human): private wrapper with no symbol callers, but module-level inherited suppression only; no item-level suppression or compiler warning |
| `op-plugins/src/state_plugins/keyring.rs:2,29,38,47,64,80,96,102,134` module, types and remaining helpers | kept: serde/schemars/trait/registry or source-call reachability; module suppression is not whole-file deletion proof |
| `op-plugins/src/state_plugins/keyring.rs:57,323` fixture with_connection/read_alias | kept: protected cfg(test) fixtures, including D-Bus macro method |
| `op-plugins/src/state_plugins/xray_config_types.rs:14` generated typed config family | kept: serde/schemars macro/schema reachability and unresolved plugin reverse consumers; no shape deletion based on field-read absence. Type locations listed below |
| `op-cache/src/btrfs_cache.rs:382` `numa_stats` | kept: explicit suppression but initializer at 606; b fails |
| `op-cache/src/btrfs_cache.rs:1625,1633` hot_ratio/avg_accesses | candidate (needs human): definition-only matches, impl-level suppression at 387; public cache APIs with unresolved op-snowball/root reverse consumers |
| `op-cache/src/btrfs_cache.rs:387` other impl methods | kept: source/test callers; cache/snapshot/transport functionality and protected tests preserved |
| `op-cache/src/snapshot_manager.rs:423,428` oldest_snapshot/newest_snapshot | candidate (needs human): explicit suppression and zero callers; public cache APIs with unresolved root/op-snowball graph reverse consumers |
| `op-cache/src/snapshot_manager.rs:587` SnapshotInfo.created | kept: explicit suppression but snapshot construction assigns field; public metadata, b fails |
| `op-cache/src/workflow_tracker.rs:90,92,94` AgentCall input_hash/timestamp/latency_ms | kept: explicit suppression but buffered AgentCall initialization at 190–192 and SQL names match; b fails; documented SQLite migration debt preserved |
| `op-cache/src/workflow_cache.rs`, `workstack_cache.rs` | kept: documented SQLite migration debt; no explicit dead-helper proof found |
| `op-cache/src/orchestrator.rs:74` numa_topology | kept: explicit suppression but construction/logging references at 97/102/111; b fails |
| `op-cache/src/grpc/agent_service.rs:28` endpoint | kept: explicit suppression but registration writes at 62/127; b fails |
| `op-cache/src/grpc/cache_service.rs:35` compressed | kept: explicit suppression but writes at 191/328; b fails |
| `op-cache/src/grpc/orchestrator_service.rs:31` first_seen | kept: explicit suppression but pattern construction at 262; b fails |
| `op-cache/src/grpc/mcp_service.rs:25` orchestrator_service | kept: explicit suppression but constructor parameter/storage at 33 onward; b fails |
| `op-cache/src/numa.rs:162,171,598,979` memory_utilization/distance_to/remote_penalty/set_cpu_affinity | candidate (needs human): definition-only matches under module suppression at 2; public APIs with unresolved root/op-snowball consumers |
| `op-cache/src/numa.rs:2` other functions/types and cfg(test) section | kept: source/test consumers and protected fixtures; generic names/macro reachability are not dead proof |
| `op-state-store` | kept: no explicit dead_code/unused suppressions found and scoped check emitted no warnings; heavily consumed public schema/state surface |

Reference counts include declarations and are supporting evidence only (generic names can overcount). Full inherited-suppression method inventory: `/tmp/opencode/dc3/all-method-reference-counts.txt`, net inventory `/tmp/opencode/dc3/net-method-reference-counts.txt`. Test functions in those inventories remain protected even with a single declaration match.

## Validation

- `rustfmt --edition 2024 crates/op-plugins/src/state_plugins/openflow.rs`: passed; only owned Rust file formatted.
- `git diff --check -- crates/op-plugins/src/state_plugins/openflow.rs`: passed.
- Final removed-symbol `rg` across crates/docs: zero matches (exit 1, expected).
- Scoped native Cargo check **passed, exit 0, 75 seconds**, no compiler warnings or errors: delegated to `gpt-6-luna` execution worker, exact `env CARGO_CALLER_FAMILY=gpt CARGO_DISPATCHED=1 cargo check -p op-plugins -p op-cache -p op-state-store`, from repo root. Same target lock; no bypass or background-build interruption. Receipt `/tmp/opencode/dc3-check.log`, independently read by primary. No proof-a candidates were reported.
- Coordinator owns final `/verify`; no workspace fmt/clippy/test claim made by this worker.

No commits/pushes, seal/golden, service restart, deployment, runtime/identity/storage/grant modifications. Worker audit and scoped validation complete; coordinator's next action is review the shared diff/report and perform final workspace verification.

## Retained generated Xray type locations

All paths `crates/op-plugins/src/state_plugins/xray_config_types.rs`; kept for typed schema/serde reachability, not deletion candidates based on unused runtime fields.
20:pub struct APIConfig {
31:pub struct NameServerConfig {
77:pub struct DNSConfig {
115:pub struct PolicyConfig {
126:pub struct DokodemoConfig {
155:pub struct HTTPAccount {
166:pub struct HTTPServerConfig {
181:pub struct HTTPRemoteConfig {
192:pub struct HTTPClientConfig {
215:pub struct SocksAccount {
226:pub struct SocksServerConfig {
246:pub struct SocksRemoteConfig {
257:pub struct SocksClientConfig {
278:pub struct VMessAccount {
289:pub struct VMessDefaultConfig {
296:pub struct VMessInboundConfig {
308:pub struct VMessOutboundTarget {
319:pub struct VMessOutboundConfig {
341:pub struct VLessInboundFallback {
359:pub struct VLessInboundConfig {
376:pub struct VLessReverseConfig {
385:pub struct VLessOutboundVnext {
396:pub struct VLessOutboundConfig {
425:pub struct TrojanServerTarget {
442:pub struct TrojanClientConfig {
461:pub struct TrojanInboundFallback {
479:pub struct TrojanUserConfig {
492:pub struct TrojanServerConfig {
503:pub struct ShadowsocksUserConfig {
521:pub struct ShadowsocksServerConfig {
542:pub struct ShadowsocksServerTarget {
560:pub struct ShadowsocksClientConfig {
580:pub struct FreedomConfig {
611:pub struct Fragment {
625:pub struct Noise {
640:pub struct FreedomFinalRuleConfig {
656:pub struct BlackholeConfig {
663:pub struct Policy {
691:pub struct SystemPolicy {
710:pub struct XrayInboundProtocolCatalog {
781:pub struct XrayOutboundProtocolCatalog {
