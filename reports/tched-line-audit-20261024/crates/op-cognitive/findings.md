# op-cognitive contextual tched naming audit

Task: `tched-line-audit-20261024/op-cognitive`  
Auditor: Codex crate worker  
Date: 2026-10-24  
Outcome: complete static naming review; no crate edits required.

## Coverage

All 35 inventory files read in sequential ranges through EOF: 12,595 supplied newline-counted lines, matching observed file endpoints. `coverage.tsv` records every path, all complete requested ranges, supplied initial SHA-256, finding/action and final status. Reader context lines outside the requested ranges were additional; no collapsed footer was counted as coverage. Every range with a continuation was followed by the next source range. No source column truncation was reported.

Directory receipts: `crates/op-cognitive`, `src`, `src/bin`, `proto`, `tests`. These reconcile to the supplied files. No nested guidance, binary, missing, symlink or dependency-only entry was encountered in these listings/inventory. All inventory entries were readable text, including build source, manifest, docs, proto and inline/integration tests. Generated-at-build protobuf inclusion macros were read in `lib.rs`; no generated build output was present in the assigned inventory.

## Every literal occurrence

Case-preserving substring review includes ordinary words, not just product names. Each row below contains one literal `tched` substring.

| Occurrence | Source | Containing token | Decision | Context and intent | JEV receipt / confidence |
| --- | --- | --- | --- | --- | --- |
| O1 | `SPEC.md:30` | `3tchedFS` | leave_as_is | Catalog-pinned filesystem specialization artifacts; already correct for our OP-DBUS projection. Not a claim of deployed artifacts. | J1 `spec_fs` / 1 |
| O2 | `src/chain_vectors.rs:502` | `batched` | leave_as_is | Ordinary adjective for one retrieve per 256 blocks in Qdrant gap accounting, not a brand. | J1 `batch_word` / 1 |
| O3 | `src/chain_vectors.rs:603` | `batched` | leave_as_is | Ordinary adjective describing chunked index-presence retrieval. | J1 `batch_word` / 1 |
| O4 | `src/qdrant_shuttle.rs:149` | `fetched` | leave_as_is | Log verb: retrieved semantic trace episodes. | J2 `fetched_word` / 1 |
| O5 | `src/qdrant_shuttle.rs:196` | `fetched` | leave_as_is | Log verb: retrieved semantic matches from schema-derived query. | J2 `fetched_word` / 1 |
| O6 | `src/qdrant_shuttle.rs:575` | `threetched` | leave_as_is | Already-qualified external/plugin fixture input to deterministic UUID derivation, first equality operand. No FS intent. | J2 `external_fixture` / 0.96 |
| O7 | `src/qdrant_shuttle.rs:576` | `threetched` | leave_as_is | Same qualified fixture input, second equality operand. | J2 `external_fixture` / 0.96 |
| O8 | `src/qdrant_shuttle.rs:580` | `threetched` | leave_as_is | Same qualified fixture input contrasted with antigravity. | J2 `external_fixture` / 0.96 |
| O9 | `tests/e2e_voyage_qdrant.rs:115` | `mail-3tched` | leave_as_is | Qualified container name inside an embedding contrast document about mail services; not the external executable or a mount declaration. No evidence authorizes a container-name migration. | J3 `mail_fixture` / 0.77 |

Totals: 9 leave_as_is; 0 replace_with_threetched; 0 replace_with_3tched. Five successful naming choices in three declarative JEV calls. A fourth call supplies two accounting/boundary choices: its accounting answer has confidence0.37 and is not accepted; manual group sums and all nine occurrence-to-judgment mappings resolve the accounting. Static-only boundary confidence0.89. Full context, questions, criteria, answers/probabilities and resolutions are in `judgments.md`.

## Changes and scope

- Crate source changes: none. No BEFORE copies or proposed source hunks needed.
- Cross-crate migration proposals: none.
- Concurrent native-audit overlap: none; no source hunk was changed.
- Exclusive reports: status.md, coverage.tsv, judgments.md, findings.md.
- Shared HANDOFF.md bookkeeping is required by developer/session rules; only this worker's claim/completion entry is in scope. Coordinator remains responsible for task-wide reconciliation.
- Existing non-naming implementation/documentation concerns are outside this assignment and intentionally unchanged; this is not a general correctness or security certification.

## Verification and limitations

Evidence is complete sequential source reads, directory/inventory reconciliation, occurrence ledger and actual installed JEV answers. No searches, shell/process/network execution, source filtering/extraction, recursive delegation, Cargo, build, tests, lint, formatting, custom smoke, runtime operations, deployment, sealing, grants, credentials, commits or staging performed. Initial hashes were supplied, not recomputed. HEAD was not queried. Concurrent later edits require coordinator reconciliation; these read receipts establish reviewed source, not a stable snapshot/deployment guarantee.

No changed behavioral surface exists to exercise. Built/tested/installed/deployed status is not claimed. No naming patch from this crate requires a build.

## Handoff

Result receipt: `/var/tmp/tched-line-audit-20261024.gnGtpg/crates/op-cognitive/result.json`. Next: coordinator reconciles complete coverage, nine occurrence dispositions and JEV receipts with the shared audit. No source action remains for this worker.

## Self-evaluation

| Axis | Score | Evidence / limitation |
| --- | --- | --- |
| Accuracy | 4/5 | Exact path/range and probability receipts retained; hashes remain supplied, not independently recomputed. |
| Completeness | 4/5 | All 35 inventory entries and nine observed substrings dispositioned; concurrent later edits are not frozen by this review. |
| Clarity | 4/5 | One row per occurrence, naming audit distinguished from runtime certification; report deliberately repeats key limits for coordinator use. |
| Actionability | 4/5 | Complete result/coverage/judgment paths and no remaining source action; coordinator still owns reconciliation. |
| Conciseness | 4/5 | Compact ledger; context evidence is duplicated in judgments to make receipts inspectable. |

Overall: 4.0/5. Improvement priority: coordinator reconciles current content against initial hashes and read receipts before consolidating the audit. Expected user agreement: qualified names and ordinary English are preserved, every inventory file was read, no unauthorized runtime work was substituted.
