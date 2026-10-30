# op-jsonrpc native audit findings

Task: native-line-audit-20261024/op-jsonrpc. Date: 2026-10-24. Auditor: Codex crate worker.

## Outcome

Complete static audit of all seven inventory files, 1,718 lines. Root and src directory listings reconcile: Cargo.toml, SPEC.md, compare-op-jsonrpc.md, src/{lib,ovsdb,protocol,server}.rs. No nested instructions, binary, symlink, missing, generated, dependency or unlisted file entries were exposed by those listings. No file skipped. Supplied initial hashes retained in coverage.tsv; no current hashes or HEAD queried.

Zero case-insensitive/compound native occurrences observed during complete sequential reads. See occurrences.tsv explicit zero record and coverage.tsv all seven rows. No native-specific needed change, independent source edit, cross-crate proposed hunk or tched/threetched overlap.

## Contextual decisions

- J1: reviewed socket/timeout/listener/database/method selections concern OVSDB/JSON-RPC, not an implicit AI model/provider route (confidence1.0).
- J2: exact reserved ovsdb.transact is denied before custom dispatch (confidence0.98). Preserve it. Not a blanket security certification of custom handlers or external callers.
- J3: documentation/module/API drift below is generic debt, not a native-specific defect (confidence0.99). Report only.

Actual successful declarative judge call, three questions, evidence/criteria/probabilities and decisions: judgments.md. Zero-occurrence coverage is the auditor's full-read observation, not a JEV certification from excerpts.

## Separate generic observation — unchanged

SPEC.md:130–179 documents obsolete server APIs and NonNet/modules absent from current lib.rs:8–19 and source directory. compare-op-jsonrpc.md:13,32,63 reports eight Rust files/nonnet modules versus four current Rust files. lib.rs:1–6 still advertises NonNet. Current server.rs:55,68,74 exposes new(config), register_handler and Arc<Self>::run rather than the documented API. No fix: generic documentation debt is outside this native-only assignment. No general compliance/security audit or runtime correctness claim.

## Verification and limits

Evidence: full sequential reads through EOF (including docs, fixture/test source), matching root/src listings, three successful contextual JEV answers, complete coverage/zero-occurrence ledger. No searches, extraction, shell/process/network execution, Cargo, builds, tests, Clippy, formatting, smoke fixtures, runtime operations, deployment, seal, grants, credentials, staging, commits, resets, stashes or recursive delegation. No behavioral changes to verify; coordinator retains any later build/check authority. Built/tested/installed/deployed status: not performed by this worker.

Shared HANDOFF bookkeeping is the only shared-file mutation, required by higher-priority session rules; unrelated claims/work preserved. Assignment result is in /var/tmp/native-line-audit-20261024.ILroaQ/crates/op-jsonrpc/result.json.

## Self-evaluation

Accuracy4/5: source ranges and actual JEV results recorded; no runtime evidence authorized, so claims remain static. Completeness5/5: every inventory file/line and explicit zero-occurrence result accounted for. Clarity4/5: ledgers provide exact references, but judgments necessarily repeat some evidence. Actionability5/5: coordinator can reconcile all seven rows and no patch needs building. Conciseness4/5: receipt detail favors traceability over minimal size. Overall4.4/5. Improvement: coordinator can link the generic documentation observation to its existing documentation task, without expanding this audit or claiming runtime validation. Would the user agree: evidence supports scoped static completion, not deployment quality.

## Next action

Coordinator reconciles coverage, explicit zero occurrences, JEV receipts and result.json. No op-jsonrpc native patch requires a build. No unresolved native blocker.
