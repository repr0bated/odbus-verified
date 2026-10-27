# Router retirement and identity/dataflow — contextual JEV decisions

Date: 2026-10-24. Decision-only continuation; root owns source, builds, deployment, and other workers' migrations. No source edits, Cargo, tests, runtime calls, or delegation. JEV was invoked directly with declarative `judge()`; outputs below preserve its exact typed choices, probability maps, and confidence values.

## Evidence inspected

- `crates/op-plugins/src/default_registry.rs:21-39`: `tched_router` is explicitly retired and included in `RETIRED_PLUGIN_IDS`.
- `crates/op-plugins/src/state_plugins/large_language_model.rs:98-136`: `large_language_model` schema depends on `tched_router`; its `stream_generate` input/output types are `tched_router::generation::{StartGenerationInput,GenerationMutationOutput}`.
- `crates/op-plugins/src/state_plugins/large_language_model_methods.rs:1-70`: `stream_generate` plans `tched_router.start_generation` and reads that plugin's sealed schema for the target capability.
- `crates/op-grpc-bridge/src/large_language_model_dispatch.rs:1-130`: nested target admission requires the exact target capability and dispatches with the caller's actor/session/genesis retained.
- `crates/op-grpc-bridge/src/mutation_engine.rs:5719-5928`: the `Chat` path forms `EpisodeScope` from caller actor, verified session, and genesis; assembles bounded context, prepends recalled context, and publishes completed episodes. Root's exact-call search identified this as the sole production `context_bundle::assemble_for_context` caller.
- `crates/op-chat-responder/src/main.rs:195-224`: `our_stack_generate` is an explicit bail stub; its comment says the combiner must wire guest-mounted `op_llm::chat::ChatManager` and explicitly forbids threetched generation/chat CLI use. Other workers own this migration; this report does not prescribe overwriting their WIP.
- Root's exact Rust search for `op_llm::schema`, `op_llm::*embedded_projection`, and `embedded_projection()` found only the definition in `crates/op-llm/src/schema.rs`, with no internal callers. The read `schema.rs:1-28` shows legacy reexports of router types and `embedded_projection()` constructing `TchedRouterPlugin::current_state().catalog`; `lib.rs:62-72` publicly exports `pub mod schema`, but the prelude does not. This supports an internal dead-code cleanup candidate while leaving external API compatibility as a risk. No Cargo dependency cleanup is supported.
- Important evidence discrepancy: the current `crates/op-grpc-bridge/src/method_receipt_audit.rs:470-495` serializes `call.change.new_value` under top-level key `args`; a targeted whole-file scan found no `input_payload` string in that file. This contradicts the supplied claim that these exact lines wrap args at `input_payload.args`. `chain_vector_projector.rs:82-140` reads `data.payload.input_payload` and passes that value directly to schema-described method fields. The conversion from the current audit event to the projector's `ChainBlockRef` payload is not established here. Do not claim the specific argument-shape mismatch without tracing that conversion.
- `crates/op-cognitive/src/chain_vectors.rs:495-650`: receive processing gathers pending vectors, upserts Qdrant, then replaces Cozo chain links, and writes the watermark after the loop. Older-block gap repair probes Qdrant point existence only. A successful Qdrant upsert followed by failed Cozo replacement can therefore leave a source-level repair gap for an older block; no live loss was reproduced.
- Owner checkpoint `/var/tmp/dataflow-e2e-20261024.rcLaWm/checkpoint.md`: same-session mounted operations observed catalog generation 216/hash, 7,084 readable/vectorized/latest blocks, valid chain verification for range 7040–7083 (44 events), Qdrant `list_collections` denied for `cap.data.qdrant.collection.list@v1`, and a separate chatbot episode listing with five entries/one returned. These do not join a specific vector/Qdrant point/Cozo link to a consumed generation turn; full chain proof, receive-triggered indexing, and chatbot retrieval/reingestion remain unverified. No permission bypass was attempted.

## JEV decisions

One `judge()` call used the evidence above. Questions explicitly included contrary or insufficient outcomes.

| Question | Exact choice | Probabilities | Confidence |
|---|---|---|---|
| Is the retired-target facade and sole recall caller migration fallout or mere naming? | `source_migration_fallout` | `source_migration_fallout`: 1.00; `mere_naming`: 0.00; `insufficient`: 0.00 | 1.00 |
| Disposition of internal-unused `op-llm` schema shim | `remove_candidate` | `remove_candidate`: 0.93; `retain`: 0.05; `insufficient`: 0.02 | 0.89 |
| Do bounded receipts establish full identity-correlated hashchain → vectors/receive → Qdrant → Cozo → chatbot round trip? | `not_established` | `not_established`: 1.00; `end_to_end`: 0.00; `contradicted`: 0.00 | 1.00 |
| Would blanket target rename to `threetched` preserve the evidenced contract? | `not_safe` | `not_safe`: 1.00; `safe`: 0.00; `insufficient`: 0.00 | 1.00 |
| Is the specific claimed `input_payload.args` argument mismatch established? | `not_established` | `confirmed_mismatch`: 0.00; `not_established`: 1.00; `live_loss`: 0.00 | 1.00 |
| How to classify the old-block Qdrant-success/Cozo-link-failure window? | `source_risk_only` | `source_risk_only`: 1.00; `live_loss_proven`: 0.00; `no_risk`: 0.00; `insufficient`: 0.00 | 1.00 |

## Evidence-bound interpretation

- The `large_language_model` facade and router Chat recall path are active source dependencies on the retired ID, so migration fallout is real; the replacement contract is not established here. Do not rename to `threetched`; its evidenced interface is not interchangeable with `start_generation`.
- `op-llm/src/schema.rs` is an internal-unreferenced router shim and a reasonable removal candidate, not an instruction to remove caller-owned code blindly. Its `pub mod` is a public API; unknown external consumers are an explicit compatibility risk. Keep common types in their existing owner and do not remove dependencies without separate evidence.
- End-to-end identity/dataflow success is not established. The observed segments are bounded and do not prove a correlated Qdrant→Cozo→chat-generation path; Qdrant inspection was denied for the exact capability recorded above.
- The proposed argument projection mismatch is unresolved because current receipt-audit evidence differs from the supplied description and the event-to-projector shape mapping is missing.
- The vector receive sequence has a source-level repair risk for Cozo links, not demonstrated live data loss. No patch recommendation is made from this decision alone.

## Supplemental JEV reassessment — traced method argument envelope

Read the additional exact contexts in this continuation:

1. `crates/op-grpc-bridge/src/method_receipt_audit.rs:473-501` builds the envelope `{operation_id,args_digest,method,args}` and passes that full value to `ChainEvent::new`.
2. `crates/op-state-store/src/event_chain.rs:204-241` stores `canonicalize_json(input_patch)` as `ChainEvent.input_payload`.
3. `crates/op-grpc-bridge/src/mutation_engine.rs:6799-6865` serializes the complete `ChainEvent` into `PluginFootprint.payload`.
4. `crates/op-snowball/src/footprint.rs:98-109` places that footprint payload under `BlockEvent.data.payload`.
5. `crates/op-snowball/src/snowball.rs:369-384` serializes the `BlockEvent` as timing data; `ChainBlockRef::from_timing_bytes` at 1281-1312 removes only the outer `data` field and preserves the nested data object.
6. `crates/op-grpc-bridge/src/chain_vector_projector.rs:81-143` extracts `data.payload.input_payload`, then passes that whole value to `described_fields` for method argument schemas. The signal branch intentionally consumes the direct signal input payload.

This complete current-source mapping resolves the earlier gap: the actual method arguments are under `data.payload.input_payload.args`, while the method projector receives the enclosing envelope. This establishes a source-level projection mismatch. No live semantic loss was reproduced; no patch was requested or made.

One actual `judge()` question asked whether the traced mapping established the mismatch, with options `confirmed_source_mismatch`, `insufficient`, and `no_mismatch`:

| Question | Exact choice | Probabilities | Confidence |
|---|---|---|---|
| Does the complete serialization trace establish the specific method argument projection mismatch, separately from live loss? | `confirmed_source_mismatch` | `confirmed_source_mismatch`: 0.98; `insufficient`: 0.02; `no_mismatch`: 0.00 | 0.97 |

This supplemental judgment supersedes only the earlier `not_established` finding for the argument-shape issue. All other JEV decisions above remain unchanged. No source patch, tests, Cargo, or runtime action was performed.
