# op-introspection native audit findings

Task: native-line-audit-20261024/op-introspection. Date: 2026-10-24. Auditor: Codex crate worker.

## Outcome

Complete static audit of all ten inventory files and every observed native occurrence. Root and src directory listings reconcile: three root files, seven Rust sources; 1,704 lines read through EOF. No binary, missing, symlink, dependency-vendored or unlisted file entries encountered. Supplied MIME `text/x-c` for indexer.rs is an inventory classification; actual content is Rust and was read fully. No inventory entry excluded. Coverage.tsv records ranges and supplied initial hashes (not fresh hashes).

Two occurrences, both preserved:

1. SPEC.md:88 — native full-text search means the built-in Cozo FTS capability.
2. src/indexer.rs:3 — Cozo's native full-text index means the same built-in capability over the documented six fields.

Both actual contexts and implementation were given to installed declarative JEV. J1 preserves accurate wording (confidence0.88); J2 finds no shown provider/model override or independent plugin definition (confidence1). `::fts create` at indexer.rs:124, search_text construction at285-294 and `~dbus_index_items:search` at353/357 provide implementation evidence. Both occurrences resolved; no independent native-specific source defect or necessary patch. No tched/threetched hunk/cross-crate overlap or proposed caller migration.

## Generic observations — separate, report-only

J3 classifies these as unrelated generic issues (confidence1); no runtime impact claim beyond exact source behavior.

- G1: SPEC.md:19-30,69-81 and compare-op-introspection.md:11-21,35-42,51-55 list ten Rust files and absent mod.rs/hierarchical.rs/cpu_features.rs as present. Actual source listing contains seven files. Comparison:76 still lists rusqlite; Cargo.toml:23 uses op-cozo-store. src/lib.rs:8 still says FTS5 although indexer.rs uses Cozo FTS. These are stale inventory/backend descriptions, not inaccurate uses of native.
- G2: Public `IntrospectionParser::parse`, parser.rs:12-19, ignores `_xml` and returns an object with empty interfaces and children. Scanner.rs:94-461 implements XML parsing separately, and lib.rs:73 uses the scanner. Existing parser behavior is not a native-specific defect; caller use/impact outside the crate was not inspected or certified.

## Verification and limitations

Performed: complete sequential source/doc/fixture reads, directory reconciliation, three successful installed declarative JEV choice answers, per-file and per-occurrence ledgers. No search/extraction/rerank, shell/process execution, Cargo/build/test/lint/format/smoke fixtures, runtime/deploy/seal/grant/credential actions, commit/staging/reset/stash or recursive delegation.

No crate source edits. No built/tested/installed/deployed claim. HEAD not queried; inventory hashes remain pre-audit receipts. Dependencies/external callers and runtime identity enforcement are not certified by a crate-local source audit. Shared HANDOFF bookkeeping follows higher-priority session requirements only; other agents' content preserved. Coordinator owns repository-wide reconciliation and any required scoped builds.

## Handoff

No pending source range, native judgment, native remediation or overlap. Coordinator: reconcile coverage.tsv (10 complete rows), occurrences.tsv (2 resolved rows), judgments.md (3 actual answers), and result.json in the assignment directory. No op-introspection patch from this audit requires a build.

## Self-evaluation

Accuracy4: exact source references and actual JEV receipts retained; dependency/runtime behavior not verified. Completeness4: all assigned files/occurrences accounted for; no whole-repository or external caller certification. Clarity4: native dispositions separated from generic observations; report has necessary receipt duplication. Actionability4: coordinator receives exact report/result paths, no patch to integrate. Conciseness4: main outcome is short; judgment rubrics retained for auditability. Overall4.0/5. Improvement: coordinator may archive the session-local artifact receipt alongside durable judgments; no additional source action warranted within this assignment. User agreement self-check: static audit scope and manual-check restrictions preserved.
