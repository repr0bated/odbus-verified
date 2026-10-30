# dc2 — op-web / op-chat dead-code audit, 2026-10-05

Worker: Codex dc2. Task: dead-code-20261005 scope 2. Branch `feat/od-108-native-lookup`, HEAD `3cf5c47d1dd3baec2522c5ddac03de5bba1c5523`; shared tree already uncommitted. AGENTS.md and all four mandatory skills preloaded, including the new identity-storage lifecycle invariant. No identity/storage behavior changed.

## Deleted

| File:line before edit | Deleted | Proof |
|---|---|---|
| crates/op-chat/src/orchestration/grpc_pool.rs:241–244 | private `AgentConnection::full_address` (attribute and function, five lines including separator) | **b**: explicit `#[allow(dead_code)]`; `rg -n '\bfull_address\b' crates/ docs/` returns only its definition. Negative-topology-gates list contains no symbol/path reference. Method and receiver type are private within grpc_pool.rs, so incoming crate-level consumers cannot call it; all crate sources including tests were searched. Both graphs contain the module with its crate `contains` edge and no other edge to that module. No exposed pool/type API removed. |

Only this deletion belongs to dc2. Existing `execute_scoped`/`execute_streaming_scoped` additions in this file belong to the orchestration port and were preserved. `rustfmt --edition 2024 crates/op-chat/src/orchestration/grpc_pool.rs` succeeded; no other Rust files formatted.

## Graph and live-surface evidence

Started with mounted `/run/mount/3tchedFS/plugins/cozo/schema.json`. Declared `list_relations` requires `cap.data.cozo.relations.list@v1`. Current worker identity's `zcall cozo list_relations -a '{}'` failed with `Failed to connect to system scope bus via local transport: Operation not permitted (consider using --machine=<user>@.host --user to connect to bus of other user)`. Transport denial does not establish a missing grant. No alternate identity/transport or direct database access attempted.

Fallback: read `docs/crate-graph/crate-graph.cozo` plus `.json` (committed 1267 nodes/2940 edges) and `/tmp/opencode/system-graph-snapshot.json` (1246 nodes/2939 edges, source:op-dbus, 2026-10-05T02:17:01Z, dirty source snapshot). Both show `op-dbus depends_on op-web`, `op-web depends_on op-chat`, imports into op-chat from web handlers/chat.rs, orchestrator/process.rs and routes/admin.rs, and `provides_bin` edges for list_tools_client, op-web-server and opdbus. Fresher snapshot additionally shows bridge's op-chat dependency and imports from grpc_server.rs and workstack_service.rs, supporting retention of public orchestration APIs. Committed .cozo lines453/1640 identify grpc_pool.rs and its `contains` edge. Graph evidence supports module/crate reachability; it is not symbol liveness or runtime proof.

## Retained candidates, per file

Locations below are pre-edit locations; grpc_pool entries after removed helper move up five lines. Every suppression candidate considered is recorded. Definition/initializer references prevent strict proof b for stored fields; generic names and unresolved public reverse consumers also prevent deletion.

| File:line | Decision | Proof status / reason |
|---|---|---|
| crates/op-chat/src/nl_admin.rs:682 `format_value` | kept | Suppressed but recursive reference at :703 and other same-name formatting functions across crates. Strict zero-reference b not met; no compiler warning established. |
| crates/op-chat/src/orchestration/grpc_pool.rs:202 `address`, :204 `port` | kept | Suppressed fields initialized by live AgentConnection::new :228–229 and generic name references across crates. Removing storage/constructor arguments would expand scope. |
| crates/op-chat/src/orchestration/grpc_pool.rs:209 `last_health_check` | kept | Suppressed, initialized :233; op-chat SPEC.md:595 and DESIGN.md:810 explicitly retain the field; other crate consumers of spelling. No b. |
| crates/op-chat/src/orchestration/grpc_pool.rs:254 `session_id`, :256 `started_agents`, :260 `metadata` | kept | Suppressed fields initialized in live start_session :398–402; names referenced across source/proto. No b; preserve session lifecycle/state. |
| crates/op-chat/src/orchestration/skills.rs:282 `SkillRegistry::list` | candidate (needs human) | Suppressed public API, generic list spelling has many consumers; incoming op-web/bridge crate edges cannot be ruled out by lexical search. No b. |
| crates/op-chat/src/orchestration/skills.rs:315 `SkillRegistry::combined_context` | candidate (needs human) | Suppressed and unique spelling has no call found, but public registry API has unresolved incoming crate consumers. Graph reverse-edge bar requires keep. |
| crates/op-chat/src/orchestration/workstack_executor.rs:320 `get_variable` | candidate (needs human) | Suppressed private helper, same-name op-workflows/context.rs:56 API exists; strict zero-reference search fails. No compiler warning established. |
| crates/op-chat/src/orchestration/workstack_executor.rs:459 `execution_id`, :461 `workstack_id`, :464 `current_phase`, :466 `started_at` | kept | Suppressed state fields populated at :597–601 and generic name references throughout crates. Preserve active scoped executor work and live state construction. |
| crates/op-web/src/routes/mod.rs:22 `chat` module | candidate (needs human) | Module-level suppression covers public route/types; legacy chat decisions reserved to owner, no compiler a proof. Incoming root crate edge unresolved. |
| crates/op-web/src/routes/mod.rs:24 `llm` module | kept | Module-level suppression is not evidence every item is dead; get_models is wired into router :216. Public types/handler APIs and macros need preservation. |
| crates/op-web/src/orchestrator/tools.rs:186 `build_system_prompt` | candidate (needs human) | Suppressed; UnifiedOrchestrator stack owner-active-decision, compiler-dead only. No a proof. |
| crates/op-web/src/orchestrator/parsing.rs:14 `extract_tool_calls_from_text`, :71 `parse_function_calls` | candidate (needs human) | Owner-active UnifiedOrchestrator stack; parse_function_calls called within retained extractor at :50,:65. Suppression insufficient under compiler-only restriction. |
| crates/op-web/src/orchestrator/formatting.rs:8 `format_results`, :64 `format_tool_result`, :107 `format_array`, :135 `summarize_object`, :181 `format_value`, :224 `clean_llm_text` | candidate (needs human) | Owner-active UnifiedOrchestrator stack; formatter methods reference each other (:41,:54,:74,:76,:83,:103,:118,:123,:206,:215). No a proof, no whole-stack deletion. |
| crates/op-web/src/handlers/chat.rs:66 `chat_handler`, :266 `chat_stream_handler`, remaining legacy handlers | kept | Explicit owner-active decision; public Axum handler signatures/macros and sessions/history routes retained. No compiler a evidence. Fixtures at :687+ protected. |
| crates/op-chat/src/orchestration/proto/op_chat.chat.rs:150,:316 and op_chat.orchestration.rs:1736,1973,2176,2475,2750,3047,3393,3632,3862,4268,4621,5174,5653,6158,6829,7234 module allowances | kept | Generated proto code explicitly protected; tonic trait/server/client macro reachability. Not deletion candidates. |

No pinned inventory, retired list, Cargo manifest, test fixture, PluginSchema declaration, proto/OpenAPI, inventory registration, deploy/runit file, mutation_engine.rs or identity_sled_dispatch.rs edited.

## Validation

Scoped native check delegated to gpt-6-luna (low effort), exactly `env CARGO_CALLER_FAMILY=gpt CARGO_DISPATCHED=1 cargo check -p op-web -p op-chat`, repo root. Log `/tmp/opencode/dc2-check.log`. **Incomplete: command session exited 143 (SIGTERM)** after Checking op-chat and compiling op-web/op-grpc-bridge, without final Cargo diagnostics. Cause of termination unestablished; worker did not kill any process. This is not a compile pass. Narrow retry of the only edited crate, `env CARGO_CALLER_FAMILY=gpt CARGO_DISPATCHED=1 cargo check -p op-chat`, delegated to the same worker, log `/tmp/opencode/dc2-op-chat-check.log`: **passed, exit 0, 7.68s, no warnings**. Existing target lock remains respected; no process kill, alternate target or lock workaround. op-web has no dc2 edits and its compile validation remains incomplete; no new proof-a candidates were established. Fleet coordinator owns final /verify after shared changes settle; no full-workspace verification claimed.

No commits, pushes, seal, golden, restart or deployment performed. Next action: coordinator consolidates reports and runs final /verify, including incomplete op-web verification.
