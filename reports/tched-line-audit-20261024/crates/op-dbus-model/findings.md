# op-dbus-model contextual naming findings

Task: `tched-line-audit-20261024/op-dbus-model`. Date: 2026-10-24. Agent: Codex crate worker.

## Outcome

Complete static audit: all five inventory files and all 390 supplied/observed lines fully read sequentially through EOF. Root and src directory listings agree with inventory. No nested agent file, binary, symlink, dependency entry, missing file or unreadable entry encountered in those listings/inventory.

Zero `tched` substrings observed, including inside longer tokens. No occurrence dispositions required: leave_as_is=0, replace_with_threetched=0, replace_with_3tched=0. Zero source edits, generated-source edits, proposed hunks, cross-crate migrations or native-audit overlaps. No BEFORE copy needed because no crate edit was made.

## Coverage

| File | Fully read lines | Substrings | Action |
| --- | --- | --- | --- |
| Cargo.toml | 1-15 EOF | 0 | unchanged |
| compare-op-dbus-model.md | 1-63 EOF | 0 | unchanged |
| SPEC.md | 1-199 EOF | 0 | unchanged |
| src/lib.rs | 1-66 EOF | 0 | unchanged |
| src/models.rs | 1-47 EOF | 0 | unchanged |

15 + 63 + 199 + 66 + 47 = 390. See coverage.tsv for supplied initial hashes and exact read ranges. Hashes are supplied inventory receipts, not freshly computed or verified digests.

## Non-naming observation — report only

SPEC.md:21-24,79-96,114-124,183-188 still describe a SQLite/sqlx persistence layer. Cargo.toml:6-15 declares no sqlx; src/lib.rs:16-66 implements an in-memory Arc/RwLock/HashMap catalog. compare-op-dbus-model.md:25,43,50 also names dependencies absent from this manifest (op-core/sqlx). This is documentation drift, not a tched naming occurrence. No repair authorized under this naming-only assignment. JEV classified the exact SPEC/lib/manifest discrepancy as report_only_generic_drift, confidence 0.99.

## Judgment and verification boundaries

One actual successful installed declarative JEV call, two typed answers retained in judgments.md. Qualified static completion: confidence 0.96. Reader full-file observation establishes zero occurrences; selected JEV excerpts do not independently certify global absence.

No searches, shell/process execution, Cargo, builds, tests, lint, formatting, smoke fixtures, runtime operations, deploy/seal/grant/credential changes, commits/staging/reset/stash or recursive delegation. No runtime, installed, tested or deployed claim. Current HEAD unqueried. Existing crate work preserved by making no edits.

Shared HANDOFF.md claim/handoff required by higher-priority session rules; no other agent's claim or SIGNALS.md changed. Reports and assignment result receipt are the only other artifacts written.

## Self-evaluation

| Axis | Score | Evidence / improvement |
| --- | --- | --- |
| Accuracy | 4/5 | Full reads and qualified static claims; source hashes are supplied, not revalidated. |
| Completeness | 5/5 | All five inventory entries through EOF; zero pending lines/dispositions. |
| Clarity | 4/5 | Coverage, observations and evidence limits separated; report paths repeat across artifacts for coordinator retrieval. |
| Actionability | 5/5 | Machine-readable result and full coverage/JEV receipts; no patch or unresolved naming action. |
| Conciseness | 4/5 | Exact JEV criteria/evidence retained as required; shortened final response avoids repeating them. |

Overall: 4.4/5. Improvement: coordinator can reconcile supplied hashes against its authoritative initial snapshot; no extra execution in this worker. User agreement expected for static naming scope, not general documentation correctness.

## Next action

Coordinator reconciles five coverage rows, zero-occurrence result and JEV receipts. No naming patch from this crate requires a build. No unread file/range or naming blocker remains.
