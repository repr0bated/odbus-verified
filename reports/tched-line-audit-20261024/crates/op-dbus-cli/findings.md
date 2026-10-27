# op-dbus-cli contextual naming audit

Task: tched-line-audit-20261024/op-dbus-cli. Date: 2026-10-24. Auditor: Codex crate worker.

## Outcome

Complete static review: four inventory files,1492 supplied/observed lines, all read sequentially through EOF. Zero literal `tched` substrings, including substrings in longer tokens. No source changes.

| Disposition | Occurrences |
| --- | ---: |
| leave_as_is | 0 |
| replace_with_threetched | 0 |
| replace_with_3tched | 0 |

## Coverage and reconciliation

Root directory: Cargo.toml and src/. Source directory: identity.rs, introspect.rs, main.rs. Listings exactly reconcile with four inventory entries. No nested context file encountered. No binary, missing, symlink or dependency entry requiring separate disposition. Coverage ranges and supplied initial SHA256 values: `coverage.tsv`. Supplied hashes are initial inventory receipts, not recomputed current checksums. No source edits means no BEFORE copy required.

Source receipt tags: Cargo.toml#80F2, identity.rs#1764, introspect.rs#4FF0, main.rs#FF9E. Full reads include existing tests and embedded completion shell text; shell snippets were read, never executed.

Mandatory 28 skill/reference files read completely, including paginated remainders; root AGENTS.md fully read. Recent SIGNALS.md:915-917 records migration/branding conflicts outside the assigned inventory; no literal affected here. Existing dirty work and concurrent native-audit edits preserved. No cross-crate proposed hunk, generated edit or native-audit overlap from this task.

## Evidence and limitations

One actual declarative JEV call returned two typed choices: complete_ledger(confidence0.98) and static_complete_zero_occurrences(confidence1). See `judgments.md` for evidence, questions, contrary outcomes, answers and decision. Source absence is line-by-line reader evidence, not JEV excerpt-based certification.

No searches, filtering, reranking, shell commands, Cargo, tests, lint, formatting, smoke fixtures, runtime operations, seal/deploy/grant/credential actions, recursive delegation, staging/commits/reset/stash. No build/test/installation/deployment claim. Current HEAD not queried under source-only/no-command assignment. Higher-priority shared-session rules required HANDOFF claim and closing entry; no SIGNALS mutation.

Coordinator next action: reconcile coverage/zero occurrence/JEV/result receipts. No naming patch in this crate requires a build.

## Self-evaluation

Accuracy4/5: exact ranges/source tags and JEV output retained; current hashes/HEAD not independently queried (intentional execution boundary). Completeness5/5: all4 inventory entries/1492 lines reviewed, zero unresolved occurrences. Clarity4/5: separate coverage, judgments and findings aid traceability but require multiple report reads. Actionability5/5: exact receipt paths and coordinator reconciliation action supplied. Conciseness4/5: mandated evidence repeated across receipts for standalone use. Mean4.4/5. Improvement: coordinator may aggregate this zero-change receipt into the common report without duplicating prose. User-agreement self-check: bounded static conclusion matches the requested source-only scope.
