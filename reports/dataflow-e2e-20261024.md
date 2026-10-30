# Identity and chatbot dataflow — current evidence

Agent: Codex `/root`. Task: `dataflow-e2e-20261024`, joined to the owner's router-removal fallout review. Updated 2026-10-24 22:28 UTC. HEAD `86f03d2dede738e2d780ab789dd5800b58678479`; substantial prior working-tree changes are not attributed to this review. Full end-to-end delivery is **not established**.

The owner clarified that 3tchedFS mirrors sealed blobs while enforcing identity. The requested work follows actual data and callers, crate by crate; it is not a FUSE rewrite or an exhaustive wording audit.

## Source path

| Link | Current owner and evidence | Finding |
| --- | --- | --- |
| Identity admission | `op-grpc-bridge/src/interceptor.rs` (`anchor_verified_human_session`, `insert_identity_metadata`, `revalidate_bound_oracle`); `project_dispatch::verified_session`; mounted guest/host identity enforcement | Principal/session/genesis are verified at operation boundaries. An operation receipt establishes only that operation, not identity lifecycle completeness. |
| Audited operation -> hash chain | `op-grpc-bridge/src/method_receipt_audit.rs:474`; `mutation_engine.rs:6800` | Attempt and terminal events carry caller scope and hashes. The lossless event becomes a Snowball footprint; the projector is notified after persistence. |
| Timing block -> vector | `op-grpc-bridge/src/chain_vector_projector.rs:82`; `op-cognitive/src/chain_vectors.rs:337` | Automatic origin path attaches vectors to the chain. It does not automatically upsert the origin Qdrant index. Method input projection currently reads the wrapper rather than its `args`, so semantic argument text can be omitted. This predates this review; no live data-loss claim. |
| Vector stream -> Qdrant -> Cozo | `op-cognitive/src/bin/op-chain-vector-ingest.rs:1`; `chain_vectors.rs:495`; `op-cozo-store/src/lib.rs:897` | Receive hook opens received timing/vectors; ingest writes a deterministic block-number point, replaces Cozo `chain_vector_link`, then saves the watermark. Running automatic send/receive and receive-hook invocation remain unverified. Below-watermark repair checks Qdrant point existence only: a successful upsert followed by a failed Cozo write can leave an old block's links unrepaired. |
| Cozo/semantic recall -> chatbot | `op-grpc-bridge/src/context_bundle.rs:136`; `mutation_engine.rs:5719,5810`; `ctl_plane_chatbot_dispatch.rs:570` | Scoped episode and namespace recall exists. The only production caller injecting the bundle into generation remains inside `tched_router.Chat`. |
| Generation after router retirement | `op-plugins/src/default_registry.rs:39`; `state_plugins/large_language_model_methods.rs:4`; `op-grpc-bridge/src/large_language_model_dispatch.rs:1`; `op-chat-responder/src/main.rs:195` | Registry retires router, but generation facade still resolves its schema and invokes its `start_generation`. The new responder's our-stack hook is explicitly unfinished. This is concrete source migration fallout, not proof that currently deployed old-router calls fail. A blanket `threetched` rename cannot preserve different method/argument/result contracts. |

## Bounded runtime evidence

All calls used the existing installed mounted invoker and assigned host session at `/run/mounts/3tchedFS`, UID1000. Full receipts remain in `/var/tmp/dataflow-e2e-20261024.rcLaWm/`; no credentials are copied here.

- 21:11–21:12 UTC: `snowball.verify_chain` verified exactly44 events7040–7083, `valid=true`, errors empty. `get_stats` reported7084 readable/vectorized blocks. This does not prove those vectors arrived at Qdrant.
- `get_projector_stats`:481 passes,239 embedded,1 failure, last_error null. Counters are not delivery receipts.
- `cozo.list_relations` listed40 relations, including `chain_vector_link`, `audit_event` and memory relations. Row contents and link agreement were not established.
- `qdrant.list_collections` refused the authenticated caller for missing `cap.data.qdrant.collection.list@v1`, before allocating an operation ID.
- 22:11:56 UTC: `ctl_plane_chatbot.list_episodes`, limit1, completed operation `4154e121-7021-4639-9910-0621dfd03ab2`, event7089/terminal7090; reported5 scoped episodes and returned1. This is scoped retrieval, not evidence of recall in a generated reply.
- 22:13:00 UTC: a bounded read-only Cozo query for the known event7081 hash's chain links was refused for missing `cap.data.cozo.query.run@v1`, before allocating an operation ID. The general query method is declared mutation-capable; the submitted Datalog contained no mutation. No retry through another identity/transport or direct database access.

## Build and deployment boundary

The filesystem cleanup's GNU release builds passed, including regenerated typed example compilation. No tests or custom fixtures were executed. No deployment has run. Current installed bridge advertises the old three-input operation call; current filesystem sends five inputs, matching current bridge source. Activation must use compatible components. Resealing current source also removes router while the callers above still depend on it; source retirement is not yet a verified live migration.

The next crate's unused `op-llm::schema` router shim was removed after repository caller search and JEV assessment. It has no repository consumers; public API removal is recorded in that crate's router-fallout report. The targeted GNU release library build passed, exit0 (3m29s).

Actual JEV decisions are preserved at `reports/tched-context-audit-20261024/dataflow-jev.md`. The judge initially lacked the event-conversion link and rejected the argument-envelope conclusion as unestablished. A supplemental trace through `ChainEvent::new`, `event_to_footprint`, `PluginFootprint::to_block_event` and `ChainBlockRef::from_timing_bytes` established `data.payload.input_payload.args`; supplemental JEV confirmed the source mismatch at0.97. This supersedes only that earlier uncertainty. No live semantic loss or complete round-trip success is claimed.

## Implemented projector correction

`op-grpc-bridge/src/chain_vector_projector.rs` now recognizes canonical `method_call` events and reads only the matching method envelope's object-shaped `args`. Missing, mismatched or non-object arguments yield no argument text. Signals and historical direct payloads retain their existing handling. The existing schema-described field filter still controls declared/private/secret fields; identity metadata and hashes are not rewritten.

The same file's existing method fixture now uses active `snowball.search_own_events` instead of the retired router, models the real audit envelope, checks that declared query text survives, and checks envelope/scalar noise exclusion. Signal and unknown-plugin fixtures are retained. Tests were edited but not run, per owner policy. No existing vectors were regenerated or indexes changed.

The exact repair decision in `reports/tched-context-audit-20261024/projector-fix-jev.md` caught a scalar-argument gap in the initial proposal; the implementation applies its narrow object-shape guard. The targeted bridge GNU release build failed on the existing errors below, so this fix is implemented but not successfully built or deployed. Source hash and pre-edit file are in `/var/tmp/dataflow-e2e-20261024.rcLaWm/`.

## Current receiver build blocker

The initial bridge build exited101 because its default installed threetched artifact lacked `application_generation_cli_sha256` provenance. The existing prepared producer at `/var/tmp/fuse-application-fusion-20261023.aq5laq/u3-artifact-build-20261023-retry3/artifact/` was then verified against current source.pin: binary SHA256 `bbebdcc43c8a3984acb5463e27069cb881fafe47f23d787d62a9d1c5854934d1`, manifest SHA256 `72c43f5066b0d5c06deeb80aa92a8d4f341b1e78cdc59dceaaec7858fe7576d7`, with matching upstream and patch provenance. No binary or manifest was altered.

A second targeted build used that supported capture-path pair and passed schema capture. It reports two pre-existing compiler errors in the current JSON-render work:

- `mutation_engine.rs:3844` calls `json_render_generation_dispatch::dispatch`, but that module has only response prompt/argument helpers and no dispatch function. The plugin still declares `generate_spec`; deleting the branch would not supply its implementation.
- `mutation_engine.rs:3897` passes `AdmittedContract` to `json_render_methods_enriched`, which requires `PluginSchema`.

The pinned-input build finished with exit101 and exactly these two compiler errors; `bridge-build-pinned-exit-code` records the exit. These files were not changed by this task. Source cleanup is not evidence of a built bridge, successful deployment, or end-to-end dataflow. Existing caller-generation migration remains WIP; the recovered owner report describes old router functionality as nonfunctional, so preservation of that old design must not become an invented deployment gate. The concrete current gate is obtaining a compiling compatible receiver.

Logs: `bridge-build.log` and `bridge-build-pinned.log` in the dataflow scratch directory. No `./deploy.sh`, live seal, index mutation or grant change occurred.

Final read-only controller status remains `complete a release-9b52e195-0253-4616-8a98-2b2f16d9defb c5ca9e62d86b05259f8b38ebf887897c05379e7a164bfec48492cbcfdb7216e0`, with `transaction_in_progress=false`. Receipt: `release-after.json` in the same scratch directory. Cargo/compiler processes finished; suspended unrelated workers were not resumed.

## Self-review of this checkpoint

Accuracy4/5: source, actual JEV and bounded receipts are separated; no complete live dataflow proof. Completeness2/5: generation migration, receiver build, deployment and cross-store verification remain incomplete. Clarity4/5: report identifies the exact links and blockers; older historical findings still need their original context. Actionability3/5: next work is resolving the existing `generate_spec` dispatcher and schema-wrapper call in their owned migration, rebuilding the receiver with the verified capture pair, then preparing the authorized coherent release. Conciseness4/5: detailed evidence lives here; user-facing status can remain short. Overall3.4/5. Do not claim the original task finished or reinterpret missing checks as passes.

## Context authority

The owner's current instructions supersede historical byte-audit workflow. The local NotebookLM source `/home/jeremy/.notebooklm-sources/gpt-6-astra/codex_01a14efc-83a2-7ed0-82a8-c6eaceef4def.json`, user turn1143, records the earlier instruction to collapse handrolled binary-schema overlap into captured `threetched`. It does not establish today's deployed contract. Current source, mounted declarations and operation receipts supply the evidence above. No remote NotebookLM sync success is claimed.
