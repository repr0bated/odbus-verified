# op-blob tched contextual naming audit

Task `tched-line-audit-20261024/op-blob`; Codex; 2026-10-24.

## Outcome

Complete static audit: all 19 inventory files and 5,146 supplied newline-counted lines read sequentially through EOF. Root/src/tests directory listings agree with inventory. No binary, missing, symlink, dependency-only or nested context entry encountered. Lockfile, manifest, documentation, module tests and integration tests included; nothing excluded for size or generation.

Four literal `tched` substrings, all inside already-correct `threetched` identifiers in a single CLI flag reflection fixture:

| Source | Exact token | Disposition | Reason | JEV confidence |
| --- | --- | --- | --- | --- |
| `src/descriptor.rs:877` | `threetched.read` | leave_as_is | External CLI fixture capability, already qualified | 1.00 |
| `src/descriptor.rs:878` | `obs.threetched.configure@v1` | leave_as_is | External CLI fixture taxonomy, not filesystem branding | 0.99 |
| `src/descriptor.rs:886` | `threetched` | leave_as_is | Schema name for the same external CLI fixture | 0.93 |
| `src/descriptor.rs:888` | `threetched` | leave_as_is | Descriptor plugin identity matches that schema | 0.96 |

Counts are literal source occurrences, not unique strings: leave_as_is 4; replace_with_threetched 0; replace_with_3tched 0. No other `tched` substring observed in the full sequential reads, including ordinary-word substrings. No crate source edits, proposed cross-crate hunks or native-audit overlap.

## Evidence and limits

- `coverage.tsv`: each inventory path, requested sequential read ranges through EOF, supplied initial SHA-256, finding and completed status. Reads also returned adjacent context; ledger records requested ranges only.
- `judgments.md`: exact source context, four typed questions with contrary outcomes, actual successful installed JEV receipt and decisions. JEV used only for declarative evidence judgments, never model routing or source skipping.
- No searches, shell/process commands, Cargo/build/tests/lint/format/smoke/runtime/seal/deploy/grant/credential actions. No compilation or execution result claimed.
- Supplied hashes are retained initial inventory evidence, not freshly computed hashes. HEAD not queried. Audit is of the observed source, not an immutable repository snapshot or deployment proof.
- Shared `HANDOFF.md` received only the required session claim/handoff bookkeeping under higher-priority instructions; coordinator remains responsible for overall reconciliation. `SIGNALS.md` and other workers' files/reports were not changed.

## Next action

Coordinator reconcile this report and `/var/tmp/tched-line-audit-20261024.gnGtpg/crates/op-blob/result.json`. No op-blob naming patch requires a build.

## Self-evaluation

Accuracy 4/5: exact source/JEV receipts preserved; initial hashes not independently recomputed. Completeness 5/5: all 19 entries, every line and all four occurrences dispositioned. Clarity 4/5: occurrence table is direct; exact receipt expands the report. Actionability 5/5: no pending file or naming hunk; coordinator has explicit result/coverage paths. Conciseness 4/5: evidence duplicated only where needed for durable judgment context. Overall 4.4/5. Improvement: coordinator should preserve these receipts alongside its final snapshot identity; no new check or mutation requested here. User agreement expected for the requested source-only scope, not runtime correctness.
