# op-agents routing audit status

Task: `tched-routing-audit-20261024/op-agents`.
Started: 2026-10-24. Owner: Codex crate worker.
Scope: audit/report only; no crate mutations.
Inventory: `/var/tmp/tched-routing-audit-20261024/crates/op-agents/inventory.tsv`.
State: complete static audit, 2026-10-24. All 149 inventory files, 21,956 lines, read sequentially through EOF; 27 directory listings reconciled. One ordinary-word occurrence (`matched`, gen-agent-defs.rs:10:21), leave_as_is. Zero tched-associated dead ends, reroutes, renames or deletions.
Authority: per-crate instructions retire `tched_router`; older root guidance describing its active integration is conflicting historical context, not live proof.
Shared bookkeeping: required HANDOFF.md claim is the only exception to exclusive-report writes, required by higher-priority session rules.
No searches, Cargo, tests, lint, formatting, runtime, deployment, sealing, grants or credential operations.

## Completion evidence

- Five required reports written; persisted coverage has 149 inventory-ordered rows, all EOF markers, no range gaps; persisted result counts reconcile.
- Exact occurrence Jev: ordinary_word 0.97, not_applicable 1, leave_as_is 1. Full verbatim context/questions/answers in judgments.md.
- Supplemental full-file Jev batch jdgb-15b3299009447e45: 149/149 complete, zero failures; 123 low-confidence answers and 92 literal-accounting disagreements retained as non-authoritative. These are not counted as discovered routes.
- Inventory drift D001: ios_developer.rs observed 3,311 bytes versus initial 3,303; observed SHA-256 0014ff2f33f1caeec4e4a8f536344443de7755873dba90bcf855847ae7fc4a35. Preserved. Other 148 reconstructed source hashes match initial inventory.
- Raw-prefix selectors resolved suffix-selector truncation; all 28 mandatory skill/reference files read completely. No additional nested guidance encountered.
- Commit/HEAD not queried. No source files changed, built, tested, installed or deployed. No runtime availability claim.

## Self-evaluation

| Axis | Score / 5 | Evidence / improvement |
| --- | --- | --- |
| Accuracy | 4 | Exact line/column and targeted judgments agree; rejected contradictory full-file labels rather than treating them as findings. Broad judgments were poor literal extractors. |
| Completeness | 4 | 149 coverage rows, one occurrence, all five deliverables; supplied ios baseline has drift and is explicitly not certified. |
| Clarity | 4 | Static result and uncertainty separated; large supplemental receipt section requires navigation. |
| Actionability | 4 | Exact leave-as-is disposition and D001 reconciliation next action; no authorized source repair is needed for this occurrence. |
| Conciseness | 3 | judgments.md retains 149 full source states to preserve exact evidence; most are not needed to understand the sole occurrence. |

Overall: 3.8/5. Improve future audit delivery with occurrence-focused contextual Jev questions and literal accounting as the extraction authority; retain complete-source evidence separately when required. User agreement is unverified.
