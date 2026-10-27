# Native line-by-line audit coordination

Latest owner correction, 2026-10-24 (Codex `/root`, threetched-fs continuation):
use expression search, inspect context and have JEV decide. Reuse prior recorded
locations; do not restart exhaustive byte reads or substitute a contract-review
project. This supersedes older no-search/exhaustive-method statements below for
the active completion. The previous exhaustive filesystem supervisors were
stopped and the same Luna session resumed with search enabled. Other work and
source ownership remain unchanged. No build, test or deployment is implied.

Task: `native-line-audit-20261024`; coordinator Codex `/root`; 2026-10-24.
Source: owner explicitly requests native-context evaluation and fixes, JEV,
line-by-line reading without search tools, through every crate.

## Current method — owner correction

Build handoff checkpoint, 2026-10-24 21:10 UTC: 38/39 crate workers now
report complete/static-complete, plus the supplementary root assignment.
This is worker-reported coverage, not completion of aggregate evidence
reconciliation. The remaining `threetched-fs` generated-file review is partial;
its current result records prepared views through 00095 of 01710. The separate
coordinator reports 38/40 accepted native assignments and 35/40 accepted tched
assignments. These denominators include the supplementary root assignment.
The scoped GNU build for op-agents, op-gateway, op-network and op-tools passed
at 21:09:52Z (receipt `/var/tmp/native-tched-build-20261024/result.json`).
No Cargo/rustc/Clippy process remained at 21:10:03Z. Owner needs the build slot;
the active audit/build coordinator was asked to hold the next build and reach
a clean source checkpoint. Source freeze is not yet confirmed. The dataflow
task is active in parallel and remains read-only. These observations supersede
the older progress/queue statements immediately below; no workspace build,
test, deployment or end-to-end dataflow success is claimed.

Progress checkpoint, 2026-10-24: **35/39 crate workers report complete**;
aggregate source-read/occurrence/patch reconciliation is not complete.
`op-grpc-bridge` and `op-plugins` have partial receipts, with active recovery
workers confirmed by held locks and current session writes. `op-web` is active
without a final receipt. `threetched-fs` stopped with partial status and no
result.json; its continuation is queued behind the web worker to preserve the
parallel bound. Full native audit completion is NOT established.

The original queue (PID10959, started18:51:48Z) exited125 at20:19:07Z;
separate recovery/reconciliation workers continue. The filesystem continuation
entry point is task scratch `resume-fs-after-web.sh`; it resumes the same session,
requires exact missing-range coverage and records result/operation receipts.
The next requested end-to-end dataflow verification remains queued behind this
audit. No whole-repository, build/test, deployment or dataflow success is claimed.

Owner clarified: one agent per crate, **JEV declarative**, not JEV-routed; that
method worked for the concurrent tched audit. The earlier all-crate routed worker
was intentionally stopped at 18:47:58Z (exit130); its partial reports are retained
as partial evidence, not completed coverage. No application source changes from it.

39 separate crate assignments now run in bounded three-worker batches, with
exclusive per-crate report/receipt directories. Each agent performs its own full
preload and sequential source reads. Fixed agent model; no model-router extension.
JEV receives typed declarative questions about actual code/evidence and proposed
fixes through the installed judge()/jev_verify facilities. Search, shell, LSP,
recursive task and reranking tools remain disabled. eval is allowed only for
declarative JEV judgments, not arbitrary source processing or execution.

Current queue entry point: `/var/tmp/native-line-audit-20261024.ILroaQ/run-queue.sh`.
Reports: `crates/<crate>/{status.md,coverage.tsv,findings.md,judgments.md}` relative
to this report directory. Receipts: task scratch `crates/<crate>/result.json` and
`process-completion.json`; process exit is not audit completion. All assignments
still need actual coverage before completion is claimed. Startup attempt 1
rejected unavailable context tools before any model work; corrected and retained.

## Initial inventory and historical setup

Inventory at `/var/tmp/native-line-audit-20261024.ILroaQ/inventory.tsv` contains
39 crate directories, including excluded `op-uds-relay`, plus root inventory
files: 1,245 paths, 1,241 existing files and four deleted paths. Newline count:
703,668, including documentation and fixtures. Inventory is not reviewed coverage.
Source head and dirty-state baseline are saved alongside it. Directory/file
inventory used git ls-files and shell globs; no content search was used for this
audit. The previous native keyword-search result is not a coverage source.

Worker: existing OMP with installed JEV tools and router. JEV chose DEEP; its
existing model mapping is `openai-codex/gpt-6-astra`. Initial OMP default was a
persisted manually selected unavailable model (`x-preview-f-free`); using the
JEV-mapped model restored startup without changing global routing. No source
reads occurred in that failed attempt. Installed extensions had to be explicitly
loaded because default discovery did not register their tools.

The original worker was detached with closed stdin, logs, PID, session history and final
exit receipt under `/var/tmp/native-line-audit-20261024.ILroaQ/`. At launch,
PID/PGID/session 24288 had PPID1. Only read/edit/write/todo and JEV route/verify
tools are enabled. No content-search, shell, LSP or recursive task tools.
Mandatory skill preload is required for the worker independently.

OpenCode/JEV owns the simultaneous `tched` audit. Native-specific findings that
overlap those same hunks must be recorded for reconciliation; no blind rewrite
or blanket renaming. Exact before copies are required for independent fixes.

Original status.md/coverage.tsv/findings.md are stopped partial receipts. Current
progress and findings belong in the separate crate workers' reports.
No crate is claimed complete here. No tests, checks, builds, runtime writes,
seals, deployments, commits or pushes have been performed for this expanded task.
Earlier bounded source fix/build evidence remains in
`reports/native-reference-search-20261024.md`; its coverage is separate.
