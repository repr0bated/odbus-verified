# Contextual declarative JEV record — op-llm

Fresh source contexts and judgments, task `tched-context-audit-20261024`. No prior tched coverage was reused. JEV is semantic evidence, not runtime/deployment proof.

## Successful requests

Two near-identical `judge_batch` requests were created because the first `eval` attempted `await judge_batch(...)` even though the documented API is synchronous; the failed await did not cancel the first request. Both completed successfully with the same answers for the three occurrence-specific states. They are retained transparently:

- `jdgb-15b33567c3185918`, model `typesafe/jev-1.14.0`, 4 states, 4/4 done, 0 failed.
- `jdgb-15b3356b7fd85919`, model `typesafe/jev-latest`, 4 states, 4/4 done, 0 failed.

The `docs` state was a broad inventory summary, not an occurrence context. Its answers had confidence at most .33 and are not used as an occurrence-level or complete-coverage judgment. Zero-occurrence coverage rests on the sequential file reads recorded in `coverage.tsv`.

## Questions supplied to both calls

1. **meaning** — “For the occurrences and contextual source evidence in this state, choose the best semantic classification. Distinguish the current threetched product/captured binary contract from legacy tched_router API references or the identity-bound filesystem namespace; include ordinary incidental text if relevant. Do not assume deployment.” Criteria: current_contract = current threetched binary/captured-schema contract; legacy_api = explicitly legacy tched_router API/compatibility surface; filesystem = 3tchedFS identity-bound filesystem; mixed_contexts = more than one distinct meaning that must be separated; insufficient = source cannot establish meaning.
2. **accuracy** — “Is the wording/identifier accurate in its exact local source context, considering current owner architecture and source-defined contracts? Do not equate source with deployment. Choose the best answer.” Criteria: accurate = accurate documented legacy/current contract; partly_misleading = some occurrences/claims conflict with current source-defined contract; inaccurate = mislabels a current source contract; insufficient = cannot determine without caller/runtime authority.
3. **change** — “What is the evidence-based scope-limited human disposition for the tched-specific references in this state? Options must account for current schema authority, compatibility risk, unknown external callers, and explicit insufficient evidence. No generic cleanup or speculative replacement.” Criteria: retain = exact references meaningful as current/legacy distinction; correct = small evidence-backed in-scope correction; propose_migration = preserve API and report required migration because callers unknown; insufficient = cannot change safely, record blocker/context.

## State inputs

### cargo_manifest
Fresh full read, `crates/op-llm/Cargo.toml:12-14`: `# Single schema source: the tched_router plugin owns the LLM schema; op-llm embeds` / `# it (see src/schema.rs) instead of defining a divergent copy.` The crate depends on `op-plugins` and `op-state-store`. Current owner architecture identifies threetched's verified binary/config capture as current schema authority. Fresh current source `crates/op-plugins/src/default_registry.rs:21-39` identifies `tched_router` as a retired competing binary application schema and says only the captured threetched contract may publish operations. Current `state_plugins/threetched.rs:1-6,142-150` describes the captured plugin source. These source declarations do not establish deployment. Alternatives: retain because the comment could mean legacy-only API; correct to distinguish current threetched authority from legacy DTO API; insufficient because downstream callers are unknown.

### reminder
Fresh full read, `crates/op-llm/src/graph_turn_reminder.rs:1-11`: occurrence line 6 is `3tchedFS` in the sentence “System graph: discover cozo.list_system_graphs through identity-bound 3tchedFS; check provenance and coverage; grants apply.” The preceding comment says the common caller lacks a verified OP-DBUS principal and the wording directs discovery without claiming graph access. Owner policy requires identity-bound 3tchedFS and operation/grant admission. Alternatives: retain; correct the namespace claim; insufficient because the mount's runtime availability is unverified.

### schema_legacy
Fresh full read, `crates/op-llm/src/schema.rs:1-18`: line 4 names verified threetched CLI/config capture and canonical sealed blob. Lines 1 and 15 explicitly label the module/projection legacy and for unmigrated callers/adapters; line 3 says it publishes no schema and calls captured threetched the only binary-application authority. The module re-exports legacy `TchedRouterError`, `LlmTransport`, `TchedRouterPlugin`, `TchedRouterState`, and a legacy `embedded_projection()` backed by `TchedRouterPlugin::current_state().catalog`. Current registry source retires `tched_router` as competing binary application schema; current threetched plugin source defines captured schema. Alternatives: retain legacy data API without treating it as schema authority; remove/repoint exports (potential compatibility break); insufficient because callers and full contract migration are outside crate inventory.

### docs
Full sequential read of all 26 inventory paths. This summary was only a cross-check, not a proper occurrence question; answers were weak and excluded from disposition. No source search or reranking was used.

## Answers — first successful request `jdgb-15b33567c3185918`

| State | Question | JEV choice | Confidence | Probabilities |
|---|---|---|---:|---|
| cargo_manifest | meaning | mixed_contexts | .40 | current .04; legacy .37; filesystem 0; mixed .52; insufficient .07 |
| cargo_manifest | accuracy | inaccurate | .29 | accurate .03; partly misleading .46; inaccurate .47; insufficient .04 |
| cargo_manifest | change | insufficient | .65 | retain .03; correct .20; propose migration .03; insufficient .74 |
| reminder | meaning | filesystem | .59 | current .04; legacy 0; filesystem .67; mixed .27; insufficient .02 |
| reminder | accuracy | accurate | .51 | accurate .63; partly misleading .15; inaccurate .03; insufficient .19 |
| reminder | change | insufficient | .88 | retain .06; correct .02; propose migration .01; insufficient .91 |
| schema_legacy | meaning | mixed_contexts | .85 | current .01; legacy .11; filesystem 0; mixed .88; insufficient 0 |
| schema_legacy | accuracy | accurate | .57 | accurate .68; partly misleading .28; inaccurate .03; insufficient .01 |
| schema_legacy | change | retain | .28 | retain .46; correct .06; propose migration .20; insufficient .28 |

## Answers — second successful request `jdgb-15b3356b7fd85919`

The same three state answer sets were returned: `cargo_manifest` meaning `mixed_contexts` (.40), accuracy `inaccurate` (.29), change `insufficient` (.65); `reminder` meaning `filesystem` (.59), accuracy `accurate` (.51), change `insufficient` (.88); `schema_legacy` meaning `mixed_contexts` (.85), accuracy `accurate` (.57), change `retain` (.28). Probabilities were identical to the first request. The fourth `docs` state was excluded as described above.

## Human disposition and uncertainty

- `Cargo.toml:12`: correct. The code explicitly claims the retired plugin is the single current schema owner; current repo authority says otherwise. Small comment correction scopes the new authority to current binary-application schema while preserving the expressly legacy crate API.
- `graph_turn_reminder.rs:6`: retain. Local wording and comment frame the mount as identity-bound discovery and explicitly preserve grant boundaries. JEV meaning/accuracy confidence is modest; runtime availability is not claimed or verified.
- `schema.rs` occurrences: retain with bounded legacy meaning. Current file documentation explicitly says no schema authority and labels it for unmigrated callers. Current registry retirement makes migration debt real, but deleting/repointing public legacy types without caller inventory would be an unsupported compatibility cutover. JEV accuracy/change answers are low confidence; retain is a human decision from exact local source and authority, not a model conclusion.
- No runtime deployment, mounted catalog, credential, operation or live admission was checked by this source audit.

## Post-edit occurrence judgment — corrected `Cargo.toml:12`

After editing and re-reading the manifest, ran a separate synchronous declarative `judge` on the exact current comment and current source-authority context. State: `Cargo.toml:12-14` now says “The captured threetched plugin owns the current binary-application schema” and “src/schema.rs retains legacy router payloads for unmigrated callers”; current `default_registry.rs` retires `tched_router` as a competing binary application schema, and `state_plugins/threetched.rs` describes the captured authority. Source does not prove deployment. The current line's `threetched` occurrence is distinct from the original `tched_router` occurrence it replaced; baseline and current occurrence counts each remain eight.

Questions: meaning (choose among current captured contract, legacy API, filesystem, incidental, insufficient); accuracy (accurate, partly misleading, inaccurate, insufficient); change (retain, correct, insufficient), all explicitly anchored to local source and not runtime deployment.

Answers: meaning=`current_contract` (.87; current .90, legacy 0, filesystem 0, incidental .05, insufficient .05); accuracy=`accurate` (.49; accurate .62, partly misleading .16, inaccurate .08, insufficient .14); change=`insufficient` (.11; retain .34, correct .25, insufficient .41). Accuracy and change are low-confidence. Human disposition: retain the wording because it names the current source-level captured schema authority; do not interpret this as live deployment proof. The synchronous `judge` response did not return a batch receipt id.
