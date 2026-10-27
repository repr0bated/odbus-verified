# op-cognitive native contextual audit

Task `native-line-audit-20261024/op-cognitive`; Codex crate worker; 2026-10-24.

## Outcome

Complete clarified occurrence review, retaining the prior full sequential coverage of all35 inventory files (12,595 supplied newline-counted lines). Five pre-edit case-insensitive substring occurrences are individually accounted for: four native/natively semantic uses and one embedded substring in alternative. J4 evaluated every actual context for meaning and wording accuracy. Two documentation corrections finished; no executable behavior changed and no needed native correction remains unresolved. Directory listings still agree with inventory. Supplied hashes remain pre-audit receipts.

## Every-occurrence disposition

`occurrences.tsv` records exact pre/post-edit positions, context, meaning,
JEV reference, outcome/confidence, decision and status. Only assigned crate
inventory content is counted; generated audit reports are not crate source.

| ID / original position | Meaning and contextual accuracy | Final decision |
| --- | --- | --- |
| O1 `src/cognitive_tools.rs:780` | native inside `qdrant_payload_serializes_as_native_values`; assertion compares ordinary JSON payload values | Preserve; J4/O1 .70 confidence. |
| O2 `src/voyage.rs:30` | Native dimensions vocabulary is legitimate, but “differing” per supported model conflicts with the shared-width contract; comment also precedes truncation instead of output_dimension | Corrected original28-32: accurate shared-width explanation now documents output_dimension. No model/config override added; J4/O2 .97. |
| O3 `src/rag_pipeline.rs:1228` | Xray native domain-socket vocabulary is legitimate, but whole comment claims always-localhost and automatic proxy behavior not implemented by this environment-selected client | Corrected original1223-1231: loopback is default, forwarding requires separate configuration. native remains at current1225; J4/O3 .98. |
| O4 `tests/e2e_voyage_qdrant.rs:113` | natively describes direct-protocol OVS-topic fixture prose for a similarity query, not live OVS verification | Preserve; J4/O4 .98. |
| O5 `src/gemini_fallback.rs:280` | native substring in alternative (characters6-11) means other possible research approaches, not native-system semantics | Preserve ordinary English; J4/O5 confidence1. Conservatively counted to avoid silent substring omission. |

After correction, four substrings remain: O1/O3/O4 and embedded O5.
All30 other inventory files have no native/case/compound substring identified
in their retained complete sequential reads; `coverage.tsv` lists every path
and actual read ranges. Focused current rereads supplement, not replace or
inflate, that coverage.

### Changed source and BEFORE evidence

- `src/voyage.rs`: comment placement/wording only; snapshot C635→686E.
- `src/rag_pipeline.rs`: function documentation only; snapshot2653→59D5.
- Exact BEFORE excerpts are retained in the assignment directory:
  `voyage.rs.before-hunk.txt` (original28-32) and
  `rag_pipeline.rs.before-hunk.txt` (original1223-1231).
  These are affected-hunk excerpts, not whole-file copies;
  `before-excerpts.json` links their full-file read receipts and snapshot tags.
  Source stayed intact during capture. No concurrent divergence was reported
  by the edit tool; no other hunks or tched implementation changed.

This supersedes the earlier blanket preservation/no-source-change conclusion:
J1/J2 established routing relevance, not full contextual wording correctness.
J4's exact evidence/questions/results are in `occurrence-judgment-evidence.md`
and indexed by `judgments.md`. No tched/cross-crate overlap remains.

## Contextual defaults and boundaries

- `src/cognitive_tools.rs:64-150` exposes a named `gemini_query` operation with `cognitive.invoke`, query/depth validation and bounded execution. `src/gemini_fallback.rs:21-53,179-235` uses Gemini-specific environment configuration, falling back to `gemini-2.5-flash`. This is an explicit provider operation, not replacement of a separate ordinary-chat selection. J1 confidence 1. No rename or model-default removal warranted under native-only scope.
- `src/grpc_service.rs:698-772,911-979` retains fixed Gemini implementation for compatibility table extraction and explicit Gemini RPC. The quoted table body contains no authoritative caller native route or overwrite. J3 `not_established`, confidence 1. Its fixed internal provider choice is not evidence that active chat selection is replaced; the module identifies itself as embedded/internal compatibility code (`:1-4`). External registrations/callers not searched or re-audited.
- `src/code_tools.rs:388-409` prioritizes explicit collection, then explicit collections, configured retrieval collections, then fallback. `src/rag_pipeline.rs:30-43,678-715` describes existing shared Voyage-4 retrieval space. These are retrieval contracts, not native chatbot profile materialization.
- `src/server.rs:63-86` durable initialization refuses a missing semantic shuttle. No source evidence ties this to a native browser Apply prerequisite. Preserve durable-data policy.
- `src/chain_vectors.rs:159-184` requires nonempty admitted actor/session scope and filters both fields. `src/interceptor.rs:19-65` verifies a projected identity record. `src/grpc_service.rs:490-495,1083-1092` denies AddFolder before filesystem traversal. No identity/grant validation weakened. These observations are local source evidence, not proof of complete transport admission. J1 explicitly judged only local enforcement established (.95 confidence).
- `src/tool_profiles.rs` profiles select tool inventory, not a model. Fixed namespaces in `src/typed_tools.rs` select documentation memory contexts, not generation providers. No native-routing edit.

## Inherited generic risks — outside native edit scope

1. **UTF-8 preview can panic** — `src/doctor.rs:158-170` slices `&turn.answer[..200]` based only on byte length. A valid answer crossing byte 200 inside a multibyte character can panic. J3 `risk`, confidence .91. Static finding only; not reproduced. Separate repair would choose a character boundary without changing access/admission. No source change here.
2. **False indexing receipt and stale retrieval inventory** — `src/semantic_search.rs:60-69` returns `status: indexed` and `chunks_indexed: 1` without ingestion; J3 `false_receipt`, confidence 1. `:18-29,72-88` also contains a hard-coded repomix/retired-code collection inventory. The implemented registered path is `server.rs:151-159` → `register_code_tools` → `CodeIndexTool::execute` (`code_tools.rs:297-338`) → `ingest_source_text`; no call to `SemanticSearch` appears in the fully read crate. Repository-external reachability is unestablished, not declared dead. Coordinator may assign separate general cleanup; native-only audit does not replace or bless this inherited stub.

Historical SPEC/comparison dependency, session SQLite and fallback prose is not authoritative runtime evidence. No broad documentation cleanup performed.

## Verification and limitations

- Proof: retained sequential EOF reads for all35 files; focused current rereads; per-file and per-occurrence ledgers; actual successful J1/J2/J3 and supplemental J4 receipts; accepted exact comment-only edits.
- J4 reviewed all five actual contexts and explicit preserve/correct/insufficient alternatives; it did not merely ask whether routing changed.
- Earlier documented tool mismatch remains historical: JavaScript judge_batch was undefined; issue reported, supported fallback succeeded. Continuation's supported judge call succeeded without routing or source extraction.
- No searches, arbitrary eval execution, commands, Cargo/build/tests/lint/format/smoke fixtures, runtime/deployment/seal/grant/credential operations or Git mutation.
- Only comments changed; no significant behavioral change to exercise. Static patch receipts establish authored documentation correction, not built/tested/deployed or live protocol acceptance.
- Initial hashes and inherited HEAD are not current-file/commit attestations; no git/hash command run. Full source snapshots are retained read-tool receipts.
- Generic risks above remain outside native scope and untouched. No new runtime/transport/admission claim made.
- Prior HANDOFF bookkeeping belongs to the initial pass; the coordinator owns aggregate status during this continuation.

Next: coordinator reconcile `occurrences.tsv` and the two completed documentation hunks. No unresolved native change or worker-side execution remains.
