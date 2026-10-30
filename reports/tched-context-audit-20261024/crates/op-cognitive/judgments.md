# JEV contextual judgments — op-cognitive

Task: `tched-context-audit-20261024/op-cognitive`. Fresh source-context review; no prior tched coverage reused. JEV instructions were read from `xd://eval/judge`. Call reference `occurrence-context-batch-01` is the successful declarative `judge(state, questions)` call made after source contexts were read. No source edits were made.

## Evidence submitted

Repository naming authority: 3tchedFS is the projected filesystem; external executable/surface is `threetched`; plugin name is `tched_router`; `op-cognitive` is an embedded runtime. Product names must not be blindly renamed.

- `src/chain_vectors.rs:603`: generic retrieval request “batched” per chunk.
- `src/qdrant_shuttle.rs:149,196`: generic Qdrant logging says semantic trace episodes/matches were “fetched.”
- `src/qdrant_shuttle.rs:575-582`: unit test passes `threetched` to generic deterministic UUID mapping assertions; test intent is stable mapping and distinct inputs, not an actual catalog identity.
- `tests/e2e_voyage_qdrant.rs:115`: E2E retrieval fixture says “mail-3tched container”; adjacent OVS fixture and test behavior concern vector search and container scoping. No evidence establishes this as a runtime product/container name or typo.

The first JEV input also included nearby `touched` comments (`activity_filter.rs:49,174` and `qdrant_shuttle.rs:640`) as possible lexical candidates. Final literal review confirms **touched does not contain contiguous tched**; those three strings are not occurrences and are excluded from the seven-row occurrence ledger and coverage count. Their inclusion in the question did not change disposition of any actual substring.

## Questions and JEV results

1. `need_fix` — “Which occurrence groups need a tched-specific source fix, based only on supplied evidence?” Alternatives: none (no justified tched-specific edit; ordinary fragments or uncertain fixture); precise groups (demonstrably misname product); insufficient evidence.
   - Answer: `none`; confidence 0.95; probabilities none 0.97, precise groups 0.02, insufficient evidence 0.01.
2. `fixture` — “Disposition of the threetched fixture string used solely for deterministic UUID test.” Alternatives: retain (harmless test-only fixture), replace (change to unrelated fixture), uncertain.
   - Answer: `retain`; confidence 0.97; probabilities retain 0.98, replace 0.01, uncertain 0.01.
3. `e2e` — “Disposition of the mail-3tched fixture name in E2E document. Does it establish an inaccurate product reference requiring correction, or does intent lack evidence to edit?” Alternatives: retain (no edit justified), edit (demonstrably inaccurate), uncertain.
   - Answer: `retain`; confidence 0.88; probabilities retain 0.92, edit 0.01, uncertain 0.07.

## Supplemental coordinator evidence

- `/var/tmp/tched-context-audit-20261024/crates/op-cognitive/coordinator-followup.md` identifies `src/blob_vectors_tool.rs` as the sole source file missing actual read metadata. Its full sequential 1–188 read was completed in the supplemental turn. No `tched` substring occurs there; therefore no occurrence-level JEV question is applicable and the prior seven occurrence judgments are unaffected.
- All 28 files in `/var/tmp/tched-context-audit-20261024/mandatory-preload.txt` were read fully in this supplemental turn. This is recorded as supplemental, not backdated as an initial preload.
- `coordinator-read-gaps.json`: 34 files have read metadata; no missing-line or column-limited files. Follow-up specifically identifies the additional source read.

## Human disposition

Retain all seven actual occurrences. `batched` and `fetched` are ordinary technical prose. Product-looking strings occur only in a UUID test fixture and E2E retrieval fixture; neither supplied context proves an inaccurate product claim, so changing either would be speculation. `touched` strings are lexical near-misses, not occurrences. The supplemental source gap contains no occurrences. No runtime/deployment inference is made. No concrete tched-specific correction is supported by the reviewed crate context.
