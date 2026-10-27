# Findings — op-runit-systemctl native audit

Task: native-line-audit-20261024/op-runit-systemctl. Date: 2026-10-24. Auditor: Codex crate worker.

## Outcome

Complete static native audit: five files, 1,129 observed lines through EOF, one native occurrence, one contextual JEV request / three typed answers. No native-specific defect or source correction required. No unresolved native-specific blocker or tched overlap.

## N1 — retain built-in command terminology

`crates/op-runit-systemctl/src/main.rs:16`, occurrence1:

```rust
//! | restart <svc>     | `sv restart <svc>` (native, not stop + start) |
```

Meaning: runit's own sv restart command instead of an emulated stop-plus-start operation. `src/dbus.rs:409-441` validates/authorizes before dispatch, uses `sv restart` for an existing live entry, and separately establishes an enable link and requests `sv up` for an absent live entry. Both branches were supplied to JEV. This is descriptive command terminology, not model/provider/config selection. Preserve it. J1 wording confidence0.89, selection confidence1; actual evidence/questions/results in judgments.md. No deployment claim or promise that every restart branch invokes the restart subcommand.

## Coverage / resource completeness

Inventory has exactly Cargo.toml, README.md, src/bin/svd.rs, src/dbus.rs, src/main.rs. Directory reads of crate root, src and src/bin reconcile those five files; no extra files, binary/symlink/dependency entries or nested instructions encountered. Full sequential ranges and all five supplied initial SHA256 values are in coverage.tsv. Initial newline counts equal observed EOF line totals:26+110+110+789+94=1,129. No truncated source lines or unread tail encountered. Documentation, manifest, implementation, CLI and embedded tests reviewed, not sampled. No coordinator follow-up present in assignment directory listing.

Zero native occurrences in Cargo.toml, README.md, src/bin/svd.rs and src/dbus.rs; explicit complete-file coverage provides the zero-result reference. occurrences.tsv contains the sole source occurrence and distinct line/position. No searches/extraction were used.

## Separate generic observations — unchanged, not native findings

- README.md1-29,69-96 still describes retired s6 names, bus/interface paths, tools and package commands; manifest and current source instead identify runit. README90 even suggests sudo cargo. Existing root guidance explicitly supersedes those instructions. This text has no native occurrence and correcting it is outside this narrowly assigned native audit.
- src/bin/svd.rs1,73,106 still says s6d in its comment/error/usage while the Cargo binary is svd and its proxy targets runit. Generic stale labels, not native-specific changes.
- src/dbus.rs10,13 documents /etc/sv while actual path constants delegate to op_core::runit at26-28; external constant implementation not read, so no present-path claim inferred.

These observations are source-only; do not infer live exposure, execution failure or installed artifact state. Do not restore s6, broaden this task into generic cleanup, or reinterpret a native audit as a general quality pass.

## Verification / limits

Verification performed: complete sequential source reads, directory/inventory reconciliation, actual declarative contextual JEV call and report receipts. No crate source changed, so no affected consumers or docs require migration and no BEFORE backup needed. No searches, Cargo/build/check/test/lint/format/custom fixture/smoke/runtime/deploy/seal/grant/credential/git mutation or recursive delegation. HEAD/current hashes not queried; supplied hashes remain initial receipts. Built/tested/installed/deployed status: not established by this audit. Coordinator owns any separately authorized build/reconciliation.

## Self-evaluation

Accuracy4/5: exact occurrence and implementation supplied to JEV; initial hashes not recomputed and runtime unobserved, explicitly limited. Completeness4/5: all five files/1,129 lines and one occurrence accounted for; static-only scope cannot establish external/deployment behavior. Clarity4/5: ledgers separate retained native terminology from generic stale s6 docs; full judgment receipt is verbose for one occurrence. Actionability4/5: no native patch required; coordinator can aggregate named reports, generic documentation remains outside scope. Conciseness4/5: single occurrence row and focused finding, mandatory evidence repeats context across reports. Overall4.0/5. Improvements: coordinator may separately authorize generic s6 documentation repair; keep source-audit and runtime claims distinct. Expected user agreement: full crate native request satisfied without blanket renaming or unrelated edits.
