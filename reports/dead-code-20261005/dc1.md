# dc1 — op-grpc-bridge dead-code audit, 2026-10-05

Worker: Codex dc1 resumed. Task: dead-code-20261005 scope 1.
Branch: feat/od-108-native-lookup. Baseline HEAD: 3cf5c47d1dd3baec2522c5ddac03de5bba1c5523.
Shared tree was already uncommitted; no existing source diff belongs to this resumed pass.

## Outcome and boundaries

Deleted: none. No Rust source changes. All four mandatory skills and AGENTS.md were loaded, including the 2026-10-05 identity-storage lifecycle correction. `/tmp/opencode/dc1.log` was consulted for context only; earlier CognitiveCaller work is not claimed here. mutation_engine.rs and identity_sled_dispatch.rs belong to another worker. tched_catalog_binding.rs and tched_router_runtime.rs are excluded. Tests/fixtures, pinned inventory, manifests, registrations, proto/OpenAPI and deploy/runit inputs remain untouched.

## Graph evidence and supported surface

Started with `/run/mount/3tchedFS/plugins/cozo/schema.json`; its declared read methods expose the Cozo relations and system graph. `zcall cozo list_relations -a '{}'` failed: `Failed to connect to system scope bus via local transport: Operation not permitted (consider using --machine=<user>@.host --user to connect to bus of other user)`. This is a transport permission denial, not proof that a specific grant is absent. Caller: current dc1 process identity; required declared capability: `cap.data.cozo.relations.list@v1`. No alternate identity/transport attempted.

Fallback read: docs/crate-graph/crate-graph.cozo and .json (1267 nodes / 2940 edges), plus /tmp/opencode/system-graph-snapshot.json (source:op-dbus, generated 2026-10-05T02:17:01Z, dirty source at baseline HEAD). All audited module nodes have `contains` edges from the bridge crate. Both graphs show incoming `depends_on` from op-dbus and op-web. The committed graph shows six op-web module `imports`; the fresher projection adds handlers/agents.rs and updates state.rs imports. The bridge supplies four binary targets (`provides_bin`). This is source reachability support, not symbol liveness or deployment proof. Public APIs must remain where a reverse edge is unresolved. Current source confirms RemoteOperationClient imports from op-web/state.rs and state_manager_client.rs.

## Per-file candidates retained

Locations are the source positions observed during this pass. Proof b requires suppression plus zero references; definition/attribute occurrences are not references, but source/test/doc consumers are.

| File:line | Decision | Proof status / reason |
|---|---|---|
| src/proto_gen.rs:345 `to_snake_case` | kept | Suppressed, locally uncalled, but explicitly `#[cfg(test)]`: user protects fixtures. Same spelling is used by a distinct op-agents helper; lexical absence alone cannot establish proof b. |
| src/chain_vector_projector.rs:245 `stats` | candidate (needs human) | Suppressed; no qualified projector stats call found. Generic `stats` references exist across crates/docs. Projector/type itself is live in mutation_engine.rs:272,2152 and documented in identity-boundaries.md:188. Do not remove its loop, state or type; exact method proof remains insufficient under strict zero-reference rule. |
| src/dbus_identity.rs:42 `DbusBinding` | kept | Struct-level suppression supports audit timestamp; type is used at :52,54,86,266,391,812,813. `registered_at` is initialized at :272,397,819. Not proof b. |
| src/dynamic_codec.rs:14 `DynamicEncoder.output_descriptor` | kept | Suppressed but explicitly populated by encoder() at :71; DynamicCodec's descriptor participates in constructor/state. Removing it would require changing live plumbing, beyond zero-reference proof. |
| src/mcp_frontend.rs:2449 `filter_authorized_tools` | kept | Suppressed but called by test at :3779; fixtures protected, proof b fails. |
| src/chat_service.rs:6 module suppression | kept | Functions/types form an active generated ChatService implementation and schema dispatch path; async_trait and generated service calls require macro/trait reachability, not lexical call counts. |
| src/chat_service.rs:93 `ResolvedExecutionRoute` | kept | Suppressed but consumed in signatures :193,269 and constructed at :254. All five fields are read in execute_chat/dispatch_schema_chat. |
| src/chat_response_state.rs:211 `validate_scope` | kept | Conditional suppression; called at :551 from read(). Not proof b. |
| src/chat_response_state.rs:379 `commit` | candidate (needs human) | Suppressed convenience wrapper; generic commit references and retained transactional publication/test paths prevent strict zero-reference proof. Active commit path uses commit_reporting; do not alter publication infrastructure. |
| src/chat_response_state.rs:529 `subscribe` | kept | Conditional suppression; chat_response_tests.rs:271 uses publisher.subscribe(). Protected test consumer. |
| src/chat_response_state.rs:535 `read` | kept | Conditional suppression; chat_response_tests.rs:419,445 consume publisher.read(). Protected test consumers. |
| src/request_registry.rs:127 `open` | kept | Suppressed but test calls at :376,435,444,455,467,478,490,502,505,508,535. Protected fixtures; proof b fails. |
| src/request_registry.rs:233 `claim` | kept | Suppressed but test calls at :384–404,427,518. Protected fixtures; proof b fails. |
| src/grpc_client.rs:235 `RemoteOperationClient` | kept | Suppressed struct, lib.rs:142 re-export; proven op-web reverse consumers above. Public reachability rules out deletion. |
| tests/support/mod.rs:2 module suppression | kept | Test support excluded by fixture rule; no fixture edits. |
| src/tched_router_runtime.rs:186 `backup_path`, :1412 `project_state` | kept | Explicit file exclusion; no liveness decision or edit. |
| src/mutation_engine.rs:262 `dbus_call_limiter`, :282 `chat_manager`, earlier CognitiveCaller path | kept | Explicit exclusion / other worker ownership. Prior log is historical context only. |
| src/identity_sled_dispatch.rs | kept | Entire file excluded / other worker ownership; no new candidate inventory in this pass. |
| src/tched_catalog_binding.rs | kept | Entire file explicitly excluded. |
| build.rs:240 generated unused_qualifications allowance | kept | Writes generated code; not a dead-code candidate and generated/proto inventory must remain. |
| src/wake_doorbell.rs:230 `owner_key_for_sled` | kept | Proof a: scoped compiler check warns dead_code. However the test at :345 calls it; deletion would break a protected fixture. Both graph representations and .cozo identify this private module under bridge `contains`; no external module API consumer is indicated. Warning applies to production compilation, not test liveness. |

No pinned entry from tests/negative_topology_gates.rs was changed. No inventory::submit or PluginSchema declaration was changed.

## Validation and handoff

Scoped native check delegated to gpt-6-luna (low effort), executing exactly `env CARGO_CALLER_FAMILY=gpt CARGO_DISPATCHED=1 cargo check -p op-grpc-bridge` from repo root. Log: /tmp/opencode/dc1-check.log. **Passed, exit 0**, dev profile finished in 1m52s; sole warning is owner_key_for_sled above. No diagnostic for projector stats or chat_response_state.commit. Target lock preserved. No target-directory workaround, process kill, restart, seal, golden, commit or push. No rustfmt run because this pass owns no Rust edits. Fleet coordinator owns final /verify after concurrent changes settle; this report does not claim it passed.

Changed files owned by this resumed pass: this report, its dc1 Active work row and dated HANDOFF entry, and one appended SIGNALS observation. No deployment performed. Exact next action: coordinator consolidates fleet reports and runs its final /verify; retained candidates need a separate decision if test-only API narrowing is desired.
