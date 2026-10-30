# op-agents naming findings

Task: tched-line-audit-20261024; date: 2026-10-24.

## Outcome

Complete static review of all 149 supplied text entries, 22,356 supplied newline-counted lines. Every file read from line 1 through EOF, sequential ranges retained in `coverage.tsv` with the supplied initial SHA-256. No searches, sampling, skipped generated sources, or size exclusions.

One literal substring occurrence:

| Source | Token | Intent | Disposition | JEV evidence |
| --- | --- | --- | --- | --- |
| `crates/op-agents/src/bin/gen-agent-defs.rs:10` | `matched` | Ordinary English: output layout deliberately matches a reader | `leave_as_is` | J1, confidence 1, `judgments.md` |

No product-name occurrence requiring `threetched` or `3tched`. Counts: leave_as_is 1; replace_with_threetched 0; replace_with_3tched 0. No source files changed and no coordinated rename hunks needed. Concurrent native-audit and unrelated work left untouched.

## Coverage disposition

All inventory entries were readable text, including files heuristically marked text/x-c or text/x-asm. No binary, missing, deleted, symlink or dependency entry required exclusion. Directory listings reconciled with supplied crate inventory; no nested AGENTS entry encountered. Supplied hashes are initial coordinator receipts, not independently recomputed current hashes. No Git state sampled; no current commit claim.

## Out-of-scope observation

The naming occurrence is within stale prose: `src/bin/gen-agent-defs.rs:8-11` says generator is orphaned, but `src/lib.rs:12` declares it. Naming-only ownership does not authorize repairing that unrelated assertion. No application-behavior or security-quality verdict is implied by this naming audit.

## Verification and deployment limits

Verification here is complete sequential static reading plus successful declarative JEV naming judgment. No source mutation means no changed behavioral path to run. No Cargo/build/tests/lint/format/smoke/runtime/deployment/sealing/grant/credential operations, staging, commit, reset or stash. Built/tested/installed/deployed status is not established by this worker.

Coordinator next action: reconcile the coverage and J1 receipt into the overall audit; this crate has no naming patch to build or deploy.

## Self-evaluation

Accuracy 4/5: exact occurrence context and successful JEV result recorded; initial hashes not independently revalidated.
Completeness 5/5: all 149 supplied files read, one occurrence dispositioned, zero pending entries.
Clarity 4/5: explicit table and coverage ledger; detailed preload is in tool history rather than duplicated here.
Actionability 5/5: coordinator has coverage, judgment, findings and assignment result, with no unresolved naming hunk.
Conciseness 4/5: full per-file ledger is necessarily repetitive to satisfy the requested audit evidence.
Overall 4.4/5. Improvement: coordinator should bind receipts to its reconciled current revision; do not infer live state from this static review.
