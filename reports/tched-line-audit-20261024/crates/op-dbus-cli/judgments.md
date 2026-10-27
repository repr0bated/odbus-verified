# Declarative JEV evidence

Task: tched-line-audit-20261024/op-dbus-cli. Date: 2026-10-24. Auditor: Codex crate worker.

One successful installed `judge(state, questions)` call through functions.eval (JavaScript), transcript label `Judging static audit evidence`. No routing extension, model selection, source extraction, search or arbitrary execution. `xd://eval/judge` API read before invocation.

## Evidence state

| Source | Supplied lines | Sequential ranges read |
| --- | ---: | --- |
| crates/op-dbus-cli/Cargo.toml | 25 | 1-25 EOF |
| crates/op-dbus-cli/src/identity.rs | 53 | 1-53 EOF |
| crates/op-dbus-cli/src/introspect.rs | 732 | 1-240;241-480;481-732 EOF |
| crates/op-dbus-cli/src/main.rs | 682 | 1-240;241-480;481-682 EOF |

Reader observation supplied to JEV: all four files read line by line, zero literal `tched` substrings including longer tokens; no source changes; root/src listings agree; no binary/missing/symlink/dependency entries; no builds/tests/runtime operations.
Criteria supplied: every-line/no-search audit; actual occurrences need exact contextual evidence; zero occurrence remains reader observation, not fabricated examples or excerpt-based JEV certification; only static completion justified.

## Questions and returned answers

### coverage

Question: Does the stated ledger cover every supplied inventory line without gaps? Assess ledger arithmetic/ranges only, not independent proof of substring absence.

Contrary outcomes: `complete_ledger` (four files sequentially to EOF, total1492), `incomplete_ledger` (missing file/range), `insufficient_evidence`.

Returned: choice `complete_ledger`, confidence0.98; probabilities complete_ledger0.99, incomplete_ledger0.01, insufficient_evidence0.

Decision: accept bounded ledger-completeness judgment. Manual arithmetic:25+53+732+682=1492. No independent checksum or absence certification inferred.

### reporting

Question: Given zero reader-observed occurrences and no execution, what naming and completion disposition is justified?

Contrary outcomes: `static_complete_zero_occurrences` (zero occurrence decisions, no source edits, static-only claim), `rename_required` (shown occurrence requires rename), `runtime_complete` (built/tested/deployed), `insufficient_evidence`.

Returned: choice `static_complete_zero_occurrences`, confidence1; probabilities static_complete_zero_occurrences1, rename_required0, runtime_complete0, insufficient_evidence0.

Decision: no occurrence-level naming questions fabricated. leave_as_is=0; replace_with_threetched=0; replace_with_3tched=0. Report complete static audit only. No built/tested/installed/deployed claim.

## Source naming judgments

None: no `tched` occurrence exists in the fully read assigned inventory. This is the source reader's observation, not a claim that JEV inspected omitted source. Recent SIGNALS.md:915-917 describes broader contract/producer migration disagreement; no affected literal in this crate warrants a naming hunk or cross-crate proposal.
