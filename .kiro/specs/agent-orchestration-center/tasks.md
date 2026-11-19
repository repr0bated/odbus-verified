# Agent Orchestration Center — Implementation Tasks

**Owner-authorized orchestration implementation, 2026-11-19; in progress.** All builds/checks are on explicit hold pending owner coordination of four streams: no Cargo/rustc/npm/Vite/typecheck/test/lint/fmt/smoke/bench/API probe. No live credential/grant/container/storage/seal/service changes. Serial selective task commit/push is authorized only through the single parent delivery coordinator; workers do not mutate Git.

**Goal:** preserve user-designed object-node Workflows and behind-the-scenes intent/utility Workstacks as separate native systems, covering every specialist and sharing zero-exec chatbot delegation, Runtime admission, receipts and tracker.

**Architecture:** evolve WorkflowController for authored graph runs; add the missing native WorkstackController for task intent/utility and direct agent coordination, optionally selecting workflows. Reuse bounded mechanics, registry/pieces, one BridgeCore writer and unified tracker; do not make either domain a universal wrapper or restore raw legacy transports.

**Technology/owners:** Rust, existing `op-agents`, `op-plugins` PluginSchema scaffold helpers, retained `op-grpc-bridge::Runtime`/BridgeCore, `op-execution-tracker` and existing domain/index owners. No new daemon, database engine, plugin-schema directory, per-persona proto/service, MCP or model dispatcher.

**Mandatory owner correction for EVERY task:** all caller operations enter the identity-bound FUSE operation namespace on separate `3tchedFS`/Our or `threetched`/Binary mounts, including controllers/agents, inference/Jev, graph authoring and checkpoint/resume. Runtime references below mean private post-kernel-callback backing evaluation, never an alternate caller API. Bridge-hosted library mounts are current owner-confirmed behavior (`runtime_bootstrap.rs:87`, `runtime_fuse.rs:261-302,348-426`); do not rerun checks to establish that observation.

- [ ] Across T2–T9U, route each newly initiated operation through the correct identity-bound FUSE surface into mandatory tracker preflight/Runtime authorization/core receipts/output release; migrate any retained direct controller/agent Runtime caller path. No raw socket/RPC/registry fallback or same-process exception. Keep private post-callback helpers private without recursively serializing each helper through FUSE.
- [ ] Future manual cases: `all_controller_and_agent_calls_enter_identity_fuse`, `judgment_and_resume_use_fuse_ingress`, `same_process_runtime_bypass_unreachable`, `our_binary_mounts_remain_separate`, `missing_fuse_operation_has_no_transport_fallback`. Do not execute checks now.
- [ ] Future acceptance: `new_child_fuse_call_waits_for_parent_root_release`, `judgment_fuse_call_has_fresh_authorization`, `synchronous_parent_becomes_pending_continuation`, `correlation_does_not_inherit_authority`, `durable_attempt_precedes_effect`, `ui_door_translates_to_container_fuse`. These cases replace any inherited/reentrant new-call interpretation below; no checks run now.
- [ ] Future manual capacity case: `cold_core_full_tracker_empty_refuses_without_unknown_pollution`; verify durable core capacity is authoritative before a detached tracker hydrates. NOT RUN; four-stream build/check hold remains.

**Spec:** read [requirements.md](requirements.md) and [design.md](design.md), including both coverage appendices, before implementing any task. Current source references identify owners, not immutable line numbers for future edits.

**Active ownership (2026-11-19, owner-narrowed):** orchestration worker owns `op-agents/**`, `op-workflows/**`, bridge workspace/workflow/workstack/cognitive/chat orchestration and new `runtime_agent*`/`runtime_workflow*`/`runtime_workstacks*` modules. Control-foundation peer owns orchestration Runtime/tracker/core hooks only. Another GPT owns ALL canonical PluginSchema/registration/capture/schema and generic FUSE/door delivery. The user owns the KiroWeen UI; our scope ends at actual backend docking (design §4.7), with no frontend edit/restoration or missing-source blocker. Deleted root handoff/boards are not recreated.

**Observed pushed source receipts — all UNBUILT / UNTESTED / UNDEPLOYED:** ownership docs `75d244bbacd99f0992eaa0d1b5bd05264c430d0a`; recipe/context foundation `a87473c`; tracker `2a41126`; typed contracts A `7a06c52855add277b86d3d74b48caba41be33b38`; private checkpoint B `518e3d6`; capacity `25c4d54d391b2e3a486422369c5d5ed2333adebd`; provider primitive C `1f053f66`; private capture/reconcile `908ee684`. Receipt prefixes are parent-observed, not acceptance/deployment claims. T4/T5/T6/T9 remain incomplete: actual domain/lifecycle/delegation/final-reader integration and canonical schema/FUSE/resource receiver delivery are pending. No build/check/test/API probe was performed.

**Current source-only orchestration work:** typed provider primitives/private checkpoint infrastructure are delivered; concrete workstack/workflow lifecycle, privacy-correct bounded frames, delayed post-Jev normal flow, scoped final getter and chatbot caller cutover remain in progress. Deterministic graphs use no unused model/Jev group. Another owner must deliver canonical methods/effects/capabilities, real same-FD mounted checkpoint calls, actual container-owned inference/config/key backing and enforceable operation resource/input receivers (design §4.7); no acknowledgement or positive execution proof is invented. Pure graph DTO docking is split from unfinished lifecycle contracts. The user owns UI; no source restoration or frontend work is pending here. Four-stream build hold remains.

## Global constraints and review focus

- Preserve `dev` and shared dirty work. Coding workers must not reset/stash/switch/create branches or worktrees. Only the delivery coordinator may use the explicitly approved isolated `dev` delivery repository for selective task commits/pushes, without changing the original branch/index/worktree; never stage another workstream's changes.
- Container IS identity. Recipe/profile != worker != container. Reuse existing approved executor roles; do not create identities per persona.
- Every accepted user request, including reasoning-only conversation, enters an attributed agent run. Chatbot submits/observes/relays only, with narrow control permissions and no direct answers, native execution or internet rights. Inference/retrieval infrastructure uses independently approved roles, never ambient root/service credentials.
- Every actual tool attempt, including recursive child/reflection/default/fallback paths, must enter unchanged Runtime admission with exact grants, pinned schema and durable receipt correlation. Model text never grants permission.
- TWO SEPARATE FUSE mounts everywhere: `3tchedFS`/Our and `threetched`/Binary. Helpers get no identity secrets.
- Tool effects remain serialized by the existing global Runtime gate. Reasoning concurrency does not authorize relaxing effect locking or bypassing inference admission.
- One core effect ledger. Tracker/index/workflow materialization is not another general state authority. Session sled/Qdrant ownership stays separate and unchanged.
- Initial shared bounds: 64 total expanded tasks/nodes per ROOT request across all child runs and both controllers/domains, 64 aggregate resident active runs, 2 reasoning workers and 32 reasoning transitions; policy supplies concrete token/cost/deadline/tool ceilings. No fresh child/domain allowance or effect replay.
- Tests/checks only on explicit owner request. No live TypeSafe request without explicit approval, spend budget and data disclosure. No secret/private code disclosure. Existing JEV routing selects models.

Highest-impact acceptance focus: (1) zero-grant chatbot can submit but cannot execute; (2) timeout/disconnect/restart cannot repeat effects; (3) active/unresolved tracking cannot be evicted or leak raw args; (4) all 99 stock IDs and 4 historical contributions are accounted for; (5) binary-private tools cannot evade mediation; (6) workstack phases remain a separate domain.

## Dependency graph

`T1 -> T2 -> T3 -> T4 -> T5 -> T6`; `T2 + T3 -> T7`; `T2 + T3 + T5 -> T8`; `T4 + T5 + T6 + T7 + T8 -> T6W`; `T6W -> T9`; `T2 + T6 + T6W + T9 -> T9U`; `T1..T9 + T6W + T9U -> T10`. T6W is workstack policy orchestration; T9U is user workflow editor. All remain pending; shared file writes require one owner.

## T1 — Establish exhaustive recipe seed and provenance coverage

**Status:** Pending. **Dependencies:** none. **Requirements:** R1, R8, R12, R14.

**Owners/files:** `crates/op-agents/src/lib.rs`, `agent_catalog.rs`, `unified/registry.rs`, `unified/legacy_catalog.rs`, `unified/{persona,execution,orchestration}/`, `dynamic/{pieces,assemble}.rs`, historical `agents/` contributions, `src/bin/gen-agent-defs.rs` only if generated output is retained. Do not restore old bridge modules.

**Contract:** `AgentRecipeSeed` carries stable intent ID, source refs, typed I/O contracts, immutable piece references and execution-role requirements. The seed catalog is derived from shared native metadata; no second handwritten list becomes authority. Every current specialist produces a runnable reasoning recipe, not a process or automatic shell grant.

- [x] Source implemented/pushed `a87473c`: derive recipe seeds from the exact shared registry/descriptor set with equality enforcement, preserving all 99 source-inventoried intent IDs. Runtime construction/acceptance has not been executed.
- [x] Source implemented/pushed `a87473c`: account for 86 historical declarations, with the four unmatched IDs mapped only as attributed expertise contributions, never restored backends/aliases.
- [x] Source implemented/pushed `a87473c`: preserve actual catalog knowledge/examples and extracted historical prose as provenance-pinned pieces; immutable bodies share interned content. Historical diagnostics/superseded policy fragments have explicit source/digest classifications, not active authority.
- [x] Source implemented/pushed `a87473c`: preserve exact native intent IDs and existing backend alias mappings without collapsing distinct expert/pro IDs or adding a second dispatch catalog.
- [ ] Treat generated markdown as derived projection. Record the currently absent `.agents/generated` rather than creating it merely to make a count pass; correct stale generator/module documentation only within the eventual authorized cutover.

**Manual acceptance / future named cases:** `native_stock_recipe_ids_equal_descriptor_ids`, `all_historical_specialists_classified`, `recipe_ids_unique_without_alias_collapse`, `expertise_and_examples_retained_with_provenance`, `absent_generated_catalog_is_not_empty_registry`. Inspect set equality and unmatched-ID classification; test execution only on request.

## T2 — Add plugin-owned recipe lifecycle and typed run contracts

**Status:** Pending. **Dependencies:** T1. **Requirements:** R1,R6,R8,R11,R13,R14,R15,R19.

**Owners/files:** existing `state_plugins/workflows_plugin.rs` for authored graph contracts; proposed `state_plugins/workstacks_plugin.rs` for missing task/intent/utility and recipe owner, using existing scaffold/inventory conventions; `op-workflows/src/{node,flow}.rs` and builtin node adapters; shared `op-agents` metadata. No second schema catalog, schema directory or per-persona proto/service.

**Consumes:** T1 recipe seeds and canonical catalog/PluginSchema/actual object graph. **Produces:** immutable `AgentRecipe`, `ObjectNodeRef`, object-backed `WorkflowGraph`/typed ports/NodeConnection, RunSpec/TaskSpec/ChildRunRef, and scoped schema-owned management/preview/submit/status/control contracts.

- [ ] Define recipe create/update(expected revision)/disable/retire/list/get contracts; distinguish owner-management from chatbot submit/status/cancel-request capabilities. Proposed names in design §4.1 are schema-owned future methods, not calls to assume already exist.
- [ ] Define separate workstacks task/intent/utility/recipe metadata and workflows graph CRUD/run contracts. Workflow create/save/edit(expected revision)/disable/list/get/discover/preview/validate/run never imply execution rights; versions pin active runs. Workstacks expose typed submit/status/control and approved policy references without requiring graph wrappers.
- [ ] Pin exact recipe version, piece digests, execution-role reference, schema/catalog/policy revisions and typed I/O at run acceptance. Updates cannot mutate running definitions; disable blocks new runs/future dispatch; retain in-flight reconciliation and referenced versions.
- [ ] Make **each eligible discovered object a workflow node**, preserving plugin ID/actual object path/schema hash/version/read-or-operation binding and policy pin. Derive typed ports from that object's canonical schema, connect explicit node/port edges and keep graph definitions distinct from runtime tasks/attempts. Support child objects only when real schema/introspection exposes them; agent recipes bind actual plugin-owned recipe records, not per-persona services.
- [ ] Add schema-owned object-node discovery/preview/graph validation to the existing workflows contract, with plan validation capacity for 64 total expanded tasks/nodes per ROOT request shared across all child runs and both controllers/domains, not fresh allowance per child/domain. Read declared properties only from authorized canonical committed SHM, with observation revision; state fields are not implicit writable ports. Mutations require declared admitted operations and receipts.
- [ ] Validate node existence, named ports, required/default semantics, exact I/O compatibility, bounds, cycles, stale node/port policy/schema pins and grants. Replace incomplete raw builtin execution wiring with VerifiedCaller/Runtime interfaces; do not enable the blocked original engine or create per-object schemas/protos.
- [ ] Keep workstack task/intent/phase IDs/state distinct from authored workflow graph/version/run IDs. Add the missing native workstacks contract rather than restoring an old service or putting its state into workflow tables; allow direct agent coordination and optional workflow child refs.
- [ ] Expose complete authorized paged recipe discovery and exact-ID lookup even when a semantic selector shortlists only three candidates.

**Manual acceptance / future named cases:** `recipe_update_pins_existing_run`, `disabled_recipe_stops_future_dispatch_not_receipts`, `retired_version_retains_references`, `every_eligible_object_has_schema_derived_node`, `object_port_edges_preserve_schema_and_version`, `node_mutation_requires_declared_operation`, `readonly_property_is_not_mutation_port`, `missing_child_object_not_invented`, `stale_or_unauthorized_node_refused`, `dag_rejects_cycles_and_bad_bindings`, `typed_child_receipt_is_not_child_completion`, `workstack_reference_does_not_alias_workflow`. Inspect one canonical schema-derived graph surface, not a second node catalog.

## T3 — Implement approved delegation with real executor attribution

**Status:** Pending. **Dependencies:** T2. **Requirements:** R2,R3,R4,R11,R18,R19.

**Owners/files:** retained `crates/op-grpc-bridge/src/runtime.rs`, `runtime_identity.rs`, `runtime_identity_owner.rs`, `admission.rs`, `grant_document.rs`, `runtime_workspace_project.rs`, existing identity/project authorization owners. Core audit extensions are completed in T4. This task does not approve or install any live policy/grants.

**Consumes:** RunSpec and execution-role references. **Produces:** verified `DelegationBinding`/executor authority with requestor scope, approving owner/policy revision, exact operation/resource bounds, expiry/budget/disclosure rules and credential-free references.

- [ ] Resolve a registered existing execution role to its actual verified principal/container using current identity ownership. Reject model-supplied principal overrides and ambient root/service fallbacks; responder author selection is not executor selection.
- [ ] Authorize narrow submission independently from execution. Enforce executor current exact tool grant AND approved delegation policy AND requestor target/project/read-only scope, not impossible intersection with chatbot zero tool grants.
- [ ] Revalidate binding, executor authority, policy, recipe status and exact capability at each effect/disclosure boundary; stop dispatch on revocation or expiry without erasing prior effects.
- [ ] Provide job-only helpers constrained inputs/resources/results without SID1/IID1/OIA1, identity storage, grant documents, provider secrets or ambient executor credentials.
- [ ] Preserve Our/Binary mount separation across bridge-hosted and other library instances; missing/denied operation is a refusal, never a raw-tool or alternate-transport fallback.

**Manual acceptance / future named cases:** `chatbot_zero_tool_grants_can_submit_approved_scope`, `chatbot_direct_tool_attempt_denied`, `delegate_cannot_expand_requestor_resource_scope`, `revoked_executor_stops_next_task`, `same_executor_supports_multiple_personas`, `helper_has_no_identity_material`, `response_author_is_not_executor`. Approved policy fixtures must be disposable and not live mutations.

## T4 — Bind orchestration events and receipts to the existing core ledger

**Status:** Pending. **Dependencies:** T3. **Requirements:** R4,R7,R11,R13,R17,R18,R19.

**Owners/files:** `crates/op-grpc-bridge/src/core.rs`, `core_operations.rs`, `core_persistence.rs`, `core_projection.rs`, `runtime.rs`, existing workflow materialization and episode owners. No new general database.

**Consumes:** T2 run/task identities, T3 verified delegation. **Produces:** versioned attributed run/task/lease/operation-link events committed by the core owner, safe recovery references and rebuildable scoped workflow/run projections.

- [ ] Extend the existing ledger attribution/binding contract with requestor, response author where relevant, verified executor and approved delegation reference without forging the core operation actor or weakening exact principal/session recovery.
- [ ] Bind canonical operation UUID to run/task/attempt before dispatch; preserve the distinction between reserved ID, denied/pre-admission request, durable admitted attempt and dispatched effect.
- [ ] Record accepted run and transitions through the one core writer before returning/announcing acceptance. Migrate current workflow CAS records as proven scoped materialization with provenance, not a second concurrent authoritative scheduler store.
- [ ] Link child accepted-submission receipts and child terminal aggregate to parent task explicitly; preserve every committed tool receipt and partial result on later failure.
- [ ] Keep requestor status disclosure policy-scoped; executor private operation recovery remains executor-scoped. Never return private raw results or input digests merely because the requester owns the parent run.
- [ ] Recover lost responses by original operation receipt. Missing terminal, uncertain append or publication failure remains unknown; do not generate a new operation ID to replay it. Preserve session sled/Qdrant ownership and separate workstack domain state.

**Manual acceptance / future named cases:** `lost_terminal_never_replays_effect`, `duplicate_operation_id_never_dispatches_again`, `accepted_run_has_durable_core_proof`, `child_receipts_survive_parent_failure`, `requestor_cannot_read_executor_private_recovery`, `ledger_failure_closes_dispatch`, `workflow_materialization_rebuilds_from_core`. No successful agent boolean substitutes for a receipt.

## T5 — Implement mandatory tracker checkpoint and unified records

**Status:** Pending. **Dependencies:** T4. **Requirements:** R4,R5,R7,R18,R19.

**Owners/files:** `crates/op-execution-tracker/src/{execution_context,execution_tracker,record,lib,metrics,telemetry}.rs`, retained `crates/op-grpc-bridge/src/runtime.rs` and tracker consumers discovered from its public types.

**Consumes:** T4 core proofs and T3 existing authority evaluator. **Produces:** mandatory preflight/admission/output checkpoint under Runtime, one correlated execution model, scoped redacted observations and counters. No third model or independent effect ledger.

- [ ] Make one Runtime-owned tracker coordinator mandatory for every native/agent/workflow/workstack/recursive/adapter/reflection/default/fallback tool. Preflight discovery/schema/intent appropriateness, invoke existing Runtime grants/delegation evaluator, core-attempt/dispatch-once, then receipt/output grounding/relevance/disclosure and result release. No raw bypass, duplicated grant policy, circular API, new effect DB or observation-only substitute.
- [ ] Persist preflight states separately from target admission. Every NEW child/judgment/provider call uses identity-bound FUSE after persisted/deferred continuation and ancestor root end/release. No inherited/reentrant cross-FUSE root or direct nested workaround. Migrate synchronous parents to accepted/pending receipts. Postflight runs after terminal/root release. Runtime-selected finite JudgmentInfrastructure remains tracked/auth/budgeted/receipted; IDs correlate but never inherit authorization.
- [ ] Ground tool and agent-final responses in schema, true domain status, receipt/canonical SHM version and task-intent evidence; unknown/contradiction/required-judge no-fit/uncertainty/error -> withhold or explicitly unverified/NeedsReview, never fabricated success. Preserve committed effects/no replay; no secret/private-code model state by default or hallucination-free guarantee.
- [ ] Reconcile ExecutionContext parent/trace data with the ExecutionRecord model actually stored by ExecutionTracker; migrate every retained consumer and remove duplicate ID/status conventions rather than adding another compatibility model.
- [ ] Use the admitted operation ID as the tool tracker key; carry run/task/attempt/parent and requestor/executor attribution. Track refusals as refusals, never completed operations.
- [ ] Move terminal tracker reconciliation inside the detached native owner, not the request waiter; cover normal/error/unknown, dropped transport, task panic and later recovery paths.
- [ ] Replace raw argument Started events and raw error logging with closed redacted metadata/receipt references. Default is omission; safe disclosure requires explicit schema/policy support.
- [ ] Retain active and unresolved references; reserve capacity atomically and reject new admission rather than evicting in-flight records. Page completed history separately and filter recent-completed correctly.
- [ ] Derive counters once from terminal event identity; use scoped durable cursor/snapshot recovery for lagged broadcasts. A cache/broadcast gap cannot change core effect truth.

**Manual acceptance / future named cases:** `drop_waiter_still_tracks_terminal_owner`, `active_tracker_records_never_evicted`, `unresolved_operation_retained_at_capacity`, `started_event_contains_no_raw_args`, `terminal_event_counted_once`, `lagged_subscriber_recovers_scoped_cursor`, `hallucinated_method_rejected`, `transport_ok_domain_failed_no_success`, `unsupported_claim_withheld`, `judgment_uncertain_no_release`, `contradiction_requires_review`, `judgment_infrastructure_tracked_without_self_recursion`, `no_raw_runtime_checkpoint_bypass`. Use fixtures, not real secrets.

## T6 — Evolve bounded workflow scheduling without relaxing Runtime admission

**Status:** Pending. **Dependencies:** T5. **Requirements:** R1,R4,R6,R7,R13,R15,R17,R19.

**Owners/files:** retained `runtime_workspace_workflows.rs` as WorkflowController and `runtime_workspace.rs`/Runtime linkage; proposed small shared native lease/budget helpers only where both domains need them. Workstack policy is T6W, not a second generic scheduler/effect authority.

**Consumes:** object-node workflow graphs, verified execution roles, core events/tracker. **Produces:** graph-run ready-node/port execution with shared bounded leases/budgets, controls and receipt recovery; not workstack intent/utility policy.

- [ ] Preserve submission-after-durable-commit and `for_background` gate release. Never await workers, model/human approval or child roots while retaining the submitting Runtime execution gate.
- [ ] Validate finite DAGs through schema-owned plan contracts supporting 64 total expanded tasks/nodes per ROOT request across all child runs and both controllers/domains, with no fresh child/domain allowance, and reserve 64 resident-run capacity/2 reasoning-worker permits atomically. Retain existing 128 KiB plan and 16 KiB step bounds unless explicitly changed by the schema owner.
- [ ] Route each new tool/provider/retrieval/child operation through verified Our/Binary FUSE, mandatory tracker and private Runtime admission. Persist/defer parent continuation and end/release its root first; no cross-FUSE inherited gate, direct nested Runtime call or serial-effect relaxation. Only private non-operation callback helpers may share the current gate.
- [ ] Persist lease owner/epoch/fence/deadline and task budgets before dispatch. Share total run token/cost/deadline/tool/transition limits across children; no auto retry/independent hidden child budget.
- [ ] Implement pause/cancel-request boundaries and truthful domain/method outcomes; continue reconciling dispatched operations even when no new work may start.
- [ ] On shutdown/restart fence old owners, mark interruption and reconcile original receipts. A timeout/expired lease/old PID is not no-effect proof and cannot cause automatic redispatch.
- [ ] Preserve current fixed workflow conditions, useful templates and child scope checks when converting synchronous cognitive orchestration into this owner. Unsupported invalid/nested/cyclic recipes fail closed rather than silently losing steps.
- [ ] Preserve original native verified-scope/control/event-sink and typed graph/port/dependency/context contracts from `op-chat::orchestration` and `op-workflows` when evolving the agent DAG owner. Keep workstack phases/domain state separate; adapt scoped ToolExecutor semantics to Runtime, fail closed on unknown conditions and do not enable blocked raw tool/sequence facades.
- [ ] Classify existing cache/pattern owners as scoped read-result reuse and reviewable recipe proposals only. Require matching input/schema/policy, scoped proof and read freshness; cached output never replaces effect dispatch, receipt or authorization. Patterns never auto-promote methods/grants; add no new cache database or throughput claim.

- [ ] Add core-owned approval/input continuation to both domains: scoped pending SHM preview bound exact plan/args/pins/approver; end call/release gate; exactly-one valid decision; invalid input remains pending; verified current authority and expected-revision CAS consume before new work. No bearer token/local store, prior-step replay, shell DSL or imported runtime.

- [ ] Add future cases `invalid_input_preserves_checkpoint`, `copied_resume_locator_grants_nothing`, `concurrent_resume_consumed_once`, `changed_plan_or_revoked_approver_cannot_resume`, `resume_never_replays_unknown_effect`, `checkpoint_yields_another_gate`. Source behavior comparison is design §2.7, not permission to install/run Lobster.
**Manual acceptance / future named cases:** `submit_releases_gate_before_child_root`, `reasoning_does_not_relax_serial_effects`, `concurrent_admission_cannot_exceed_capacity`, `lease_fences_stale_completion`, `timeout_keeps_original_operation_for_recovery`, `cancel_after_effect_reports_committed_child`, `restart_never_auto_replays_unknown`, `child_budget_cannot_exceed_parent`, `workstack_domain_unchanged`, `cached_result_cannot_replace_effect_or_authorization`, `learned_pattern_requires_recipe_review_without_grants`, `unknown_condition_fails_closed`. No throughput benchmark is implied.

## T7 — Integrate bounded composition and Jev semantic selection

**Status:** Pending. **Dependencies:** T2,T3. **Requirements:** R8,R9,R14,R19.

**Owners/files:** `crates/op-agents/src/dynamic/{pieces,assemble}.rs`, retained `crates/op-grpc-bridge/src/runtime_cognitive.rs`, existing JEV routing/provider owner and one focused question-policy module in that subsystem. No new model dispatcher or unapproved API client path.

**Consumes:** complete recipe registry, scoped piece access and approved operation/role roster. **Produces:** pinned bounded persona composition plus `Selected`, `NoAuthorizedCandidate`, `NoSemanticMatch`, `NeedsReview`, `JudgmentUnavailable` and `AuthorizationChanged` outcomes.

- [ ] Build one authority-filtered candidate/piece set from exact grants, delegation policy, source scope and pinned contracts; remove hardcoded executor advertisements and post-budget appended static persona path.
- [x] Source implemented/pushed `a87473c`: one conservative byte budget covers the complete rendered context, user question, framing and reserved output/tool overhead; oversized required minimum/first piece fails explicitly, with no fallback append. Existing contract fixtures were adapted but not run.
- [ ] Keep memory/markdown examples and imported profiles as scoped provenance-bearing data; source relevance weights cannot become trusted instructions or permissions.
- [ ] Implement two-stage authorized-description shortlist/full-description rerank with Choice plus companion Noul and per-candidate coverage Nouls. Keep exact wording, criteria, thresholds, state schema and versions together; deterministic truth table is design §6.2.
- [ ] Surface API/malformed/out-of-set/uncertain/no-match cases without keyword/random/default-agent fallback. Continue exact capability/pin revalidation after semantic selection. Keep all catalog entries exact-addressable.
- [ ] Record only approved minimal judgment state privately; no live API probe or private-code disclosure without explicit approval/budget. JEV routing remains model authority, including Cargo.

**Manual acceptance / future named cases:** `first_oversized_piece_rejected`, `all_prompt_sections_charged`, `source_weight_cannot_grant_tools`, `choice_without_companion_match_ignored`, `uncertain_boundary_needs_review`, `judgment_api_failure_has_no_heuristic_fallback`, `unauthorized_candidates_never_sent`, `shortlist_does_not_hide_catalog_entries`. Live probability calibration/order/adversarial probes remain separately permission-gated and must not assert exact model probabilities as deterministic tests.

## T8 — Register only enforceably safe external adapter modes/imports

**Status:** Pending. **Dependencies:** T2,T3,T5. **Requirements:** R3,R5,R9,R10,R14,R19.

**Owners/files:** existing binary-backed `crates/op-plugins/src/state_plugins/threetched.rs`/discovered surface owner, retained chat-router/provider adapter owners, recipe import contract. `vendor/threetched/source.pin`/builder are provenance inputs, not permission to patch/build/deploy the binary.

**Consumes:** discovered sealed methods/configs, schema pins, approved profiles, unified tracker. **Produces:** explicit adapter support binding plus scoped imported recipe/profile references; absence is a real Unsupported outcome.

- [ ] Represent every source-defined external preset/personality entry from design Appendix B in import/provenance metadata without miscounting risk/runtime/UI/context entries as specialist agents.
- [ ] Reconcile exact sets of every authorized-visible agent, risk-profile and runtime-profile map key through owned sealed discovery/config methods, including disabled/unreachable agents and unused profiles. Preserve states/assignments/revisions under source-plus-kind-plus-alias identity; represent each entry or classify explicitly unsupported. Unknown live counts stay unknown; no private-file/endpoint fallback.
- [ ] Register external inference only if tool-free operation is enforceable, or full tool use only if every recursive/delegate/reflection/default/fallback host attempt is exclusively mediated through discovered Our/Binary operations and Runtime.
- [ ] Reject stock autonomous raw-tool mode on current evidence. Empty upstream allowlists, `__` names, optional supervision, local HMAC receipts or prompt prohibitions cannot satisfy mediation.
- [ ] Pin adapter support to actual binary/capture/schema revision and explicit mode; revoke unsupported/drifted binding without broadening grants. Do not create capture endpoints, stubs, permanent shims or a second orchestrator.

**Manual acceptance / future named cases:** `outer_binary_receipt_does_not_prove_inner_tool_mediation`, `recursive_external_tool_path_rejected_unless_mediated`, `empty_allowlist_never_means_native_unrestricted`, `profile_import_cannot_grant_execution`, `missing_sealed_import_method_is_unsupported`, `all_external_preset_concepts_classified`. Actual binary probing/building requires a separate owner request; this task may complete with the unsafe mode truthfully unavailable, not an invented adapter.

## T6W — Implement behind-the-scenes workstack intent/utility orchestration

**Status:** Pending. **Dependencies:** T2,T3,T4,T5,T6,T7,T8. **Requirements:** R1,R2,R6,R7,R13,R16,R19.

**Owners/files:** proposed `crates/op-grpc-bridge/src/runtime_workstacks.rs` linked through current Runtime dispatch; proposed `crates/op-plugins/src/state_plugins/workstacks_plugin.rs`; shared `op-agents` recipe/intent data and narrowly reused lease/budget primitives. Existing `op-chat` phased models provide provenance, not raw execution transport.

**Contract:** IntentSpec + scoped versioned owner-approved UtilityPolicy -> auditable UtilityDecision -> direct agent run OR optional workflow run; distinct WorkstackTask state, shared core/Runtime/tracker.

- [ ] Persist typed task intent, desired success evidence, scope/resources, dependencies, constraints and budget before selection. Preserve useful native phases/context/dependency semantics without making workstack a graph synonym.
- [ ] Filter feasible recipes/workflow versions in code by grants/delegation/scope/budget/dependencies/adapter support, then obtain atomic semantic-fit Jev judgments through T7. No keywords or model permissions.
- [ ] Compute utility in code from approved versioned feature definitions, normalization, weights, constraints, tie/review rules and grounded estimates; missing policy/estimates/ambiguous intent -> NeedsReview. Do not invent production utility weights or claim recovered historical scorer.
- [ ] Record every alternative, rejection/feasibility reason, judgment/evidence version, exact utility input/result, chosen pins and budget; re-evaluate only undispatched work and preserve decision history.
- [ ] Coordinate agents directly when suitable, optionally choose a workflow child, and propagate accepted/terminal/receipt relationships without mandatory workflow wrappers or editing authored graphs.
- [ ] Expose scoped durable workstack status/control/intent/utility through its own PluginSchema, joined with workflow/agent views by tracker refs. Use shared bounds of 64 resident runs, 2 reasoning workers and 64 total expanded tasks/nodes per ROOT request across all child runs and both controllers/domains, no fresh child/domain allowance or duplicate DB/effect authority.

**Manual acceptance / future cases:** `workstack_direct_agent_needs_no_workflow`, `workstack_can_choose_pinned_workflow_optionally`, `utility_policy_missing_needs_review`, `utility_math_matches_approved_policy`, `utility_decision_records_rejected_alternatives`, `model_fit_cannot_grant_rights`, `workstack_and_workflow_lifecycles_separate`.

## T9 — Cut over chatbot and all retained callers to agent requests

**Status:** Pending. **Dependencies:** T4,T5,T6,T7,T8,T6W. **Requirements:** R1–R5,R8,R10,R11,R14,R16,R18,R19.

**Owners/files:** `crates/op-grpc-bridge/src/runtime_chat_conversations.rs`, `runtime_chat_ctl.rs`, `runtime_chat.rs`, `runtime_chat_generations.rs`, `runtime_cognitive.rs`, current persona/plugin callers and existing plugin schemas. Review actual module graph before removing paths; source existence alone is not runtime linkage.

**Consumes:** complete recipe/runs, independently authorized execution infrastructure, safe adapters, scoped tracking. **Produces:** end-to-end zero-execution conversation role with delegated agent work and receipt-based responses.

- [ ] Route every accepted chatbot request, including reasoning-only conversation, through WorkstackController IntentSpec to an attributed agent run; chatbot submits/observes/relays only. Workstack may coordinate agent directly or optionally choose a workflow. User-authored standalone workflow runs remain separately admitted.
- [ ] Route inference/retrieval/embedding infrastructure through approved execution roles so chatbot zero tool grants remains literal, not just a UI restriction. No direct chatbot internet/provider secret access.
- [ ] Migrate cognitive persona/specialist/fixed orchestration callers to WorkstackController and graph workflow callers to WorkflowController, sharing run/receipt primitives without collapsing ownership. Preserve all specialist intent IDs, real controlled execution backends and child receipt links.
- [ ] Remove superseded static persona direct execute/method/dispatch conventions and duplicate tracking models only once all retained callers migrate; no permanent re-exports, aliases or alternative raw execution route. Do not revive unlinked legacy files.
- [ ] Expose scoped recipe/run/task/budget/provenance/receipt views through owned plugin methods and canonical projections. Return truthful unavailable/approval-required/unknown outcomes, never fabricated answers or success after losing authority.

**Manual acceptance / future named cases:** `reasoning_only_chat_request_has_attributed_agent_run`, `chat_turn_delegates_without_human_tool_execution`, `requestor_responder_executor_distinct_in_audit`, `analysis_only_recipe_returns_typed_result_without_shell`, `all_specialists_addressable_after_cutover`, `denied_tool_has_no_alternate_raw_path`, `private_run_visibility_is_scoped`, `committed_effect_not_recast_as_disclosure_failure`. Include two personas sharing one executor and separate mount views.

## T9U — Backend docking for the owner-built KiroWeen UI

**Status:** In progress; frontend implementation is user-owned and out of scope. **Dependencies:** T2,T6,T6W,T9 and acknowledged canonical schema/FUSE delivery. **Requirements:** R13,R15,R17,R19.

**Owners/files:** orchestration DTOs/controllers and these three existing Kiro documents. Other GPT owns canonical schemas/door/FUSE delivery; the user authors the UI. No external frontend files/restoration.

- [ ] Implement actual object-node discovery/preview/typed-port validation, immutable graph CRUD/version/disable/run/pause/resume/cancel and scoped receipt-linked state.
- [ ] Implement separate workstack recipe/role/policy/intent/utility/status/control and durable approval/any-schema-valid-JSON input gates.
- [ ] Publish requested schema methods/effects/capabilities using existing Rust DTOs in design §4.7; acknowledge actual canonical delivery before end-to-end claims. No second schema/API or assumed sealed method.
- [ ] Preserve door translation into verified container-bound Our/Binary FUSE, tracker/core receipts and scoped state/events without browser host files/mounts/credentials.
- [ ] Record task delivery SHAs only from coordinator receipts; preserve the user-coordinated build hold and report unrun checks honestly.

**Future owner-requested acceptance:** backend graph save/edit/version/run, independent standalone workflows, canonical typed ports, scoped receipts and checkpoints. UI layout/catalog/registry/editor acceptance belongs to the user's KiroWeen work.

## T10 — Owner-requested acceptance and documentation cutover

**Status:** Pending; checks require explicit owner request. **Dependencies:** T1–T9,T6W,T9U. **Requirements:** R1–R19.

**Owners/files:** existing focused contract-test locations for the affected crates, the three Kiro documents and established project documentation/changelog owners. Do not add extra root boards or resurrect deleted handoff files.

- [ ] Review requirement-to-task coverage and the manual cases below against implemented source; report implemented, tested and deployed separately. Update stale design/inventory counts only with current evidence.
- [ ] If requested, add/run focused deterministic contract cases for identity/delegation, typed recipes/DAGs, tracker redaction/retention, lost terminals, lease ownership, concurrency limits and zero-execution conversation cutover. No workspace-wide command without an explicit workspace request.
- [ ] If separately approved, perform bounded disposable end-to-end acceptance of operation receipts, cancellation/restart recovery, both mount surfaces and safe external-mode rejection. No live grant/container/storage/service mutation is inferred from this plan.
- [ ] If separately approved with budget and disclosed data, calibrate Jev questions on representative/negative/uncertain/adversarial/order-variant cases through the existing authorized provider path; keep secrets/private code out.
- [ ] Update established docs/changelog at clean cutover; distinguish absent current workstack implementation from implemented target contracts, and unsupported external modes from working integrations. No missing evidence labeled pass.

## Manual end-to-end acceptance matrix (not executed)

| Case | Expected observable result | Owning task |
|---|---|---|
| Chatbot has submit/read/cancel-request but zero tool grants | Every accepted request, including reasoning-only, has an attributed agent run; chatbot only relays result; direct tool attempt denied and executor separate | T3, T9 |
| Requestor asks for another project's resource | Refusal despite executor having broad capability; no grant broadening | T3 |
| Two specialists share an approved role | Distinct recipe/version/provenance; same existing verified container executor; no identity mint | T1–T3, T9 |
| Analysis-only specialist | Typed reasoning result, no shell privilege or fabricated process | T1, T7, T9 |
| Every stock specialist and historical declaration | 99 source-native seed IDs exactly covered; 4 unmatched historical IDs attributed; examples/expertise accounted for | T1, T9 |
| Recipe updated/disabled during a run | Immutable existing pin; future dispatch obeys disable/current authority; dispatched receipts remain recoverable | T2, T6 |
| Transport drops after dispatch | Detached owner completes/reconciles same operation and tracker record; no replay | T4–T6 |
| Terminal append/response lost | Unknown/recovery locator retained; no duplicate effect or new operation attempt | T4–T6 |
| Tracker fills while active work exists | Active/unresolved references retained, new work bounded/refused, terminal history paged | T5, T6 |
| Secret-shaped/private arguments supplied | No raw Started/event/log payload or unauthorized status disclosure | T3, T5 |
| Parent cancelled/times out after one effect | Stop subsequent dispatch; preserve committed child receipt; unresolved effect remains unknown, not rolled back | T4, T6 |
| Worker crashes/lease expires/restarts | Fence stale owner, reconcile original operations; no auto-resume uncertain effects | T4, T6 |
| Controller waits for child root | Submission already released gate; normal serial admission; no nested root deadlock | T6 |
| Jev API fails or no candidate fits | JudgmentUnavailable/NoSemanticMatch/NeedsReview; no keyword/random/default selection | T7 |
| External binary requests a hidden shell/reflection/child/fallback tool | Unsupported mode/refusal unless exclusive per-attempt Runtime mediation is enforced | T8 |
| External inventory contains disabled/unreachable agents and unused profiles, plus duplicate aliases across kinds/native names | Exact visible sets reconcile into represented or explicitly unsupported records; states/assignments preserved; source/kind/alias namespace prevents collision | T8 |
| Upstream profile named yolo/unbounded imported | Metadata preserved, native grants/budgets unchanged; no unrestricted mode enabled | T8 |
| Workstack task intent/utility and user graph coexist | Separate state/IDs/controllers; direct agent task or optional workflow selection; approved utility decision audited; standalone authored workflow independent | T2,T6,T6W,T9U |
| Workflow composed from eligible plugin/root/declared child objects and a recipe record | Object-backed nodes retain identity/schema/version, typed port connections and separate task attempts; absent children not fabricated; property reads canonical SHM, mutation only admitted method | T2, T6 |
| Same operation viewed by another principal/session | No unauthorized private result/recovery access; permitted requestor summary only under delegation disclosure | T3–T5 |
| Approval/input checkpoint copied or resumed concurrently with invalid/stale input | No held gate; invalid answer stays pending; locator grants nothing; valid current authority+plan/version CAS required and no prior effects replay | T2,T6,T6W,T9U |
| Hallucinated method/object/port or unsupported final claim | Exact discovered binding check denies preflight; receipt/state grounding withholds or marks unsupported claim unverified | T3–T5,T9 |
| Transport OK/domain failed, contradictory evidence or required judge uncertain | No success/release fallback; NeedsReview/withheld; actual effect proof retained and never retried | T5,T7,T9 |

**Traceability:** T1 inventory; T2 separate schemas/object graph; T3 authority; T4 core proofs; T5 tracker; T6 graph execution/shared mechanics; T6W intent/utility workstacks; T7 TypeSafe/composition; T8 binary mediation; T9 chatbot/caller migration; T9U backend docking for the owner-built UI; T10 manual acceptance. Implemented source, owner-delivery dependencies and unrun acceptance remain separately reported.
