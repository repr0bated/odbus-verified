# op-dynamic-loader — native contextual audit

Task: native-line-audit-20261024/op-dynamic-loader
Date: 2026-10-24
Auditor: Codex crate worker; owner-directed Luna model handoff
Result: complete static native audit; no native-word occurrences or independent native-specific defects; no crate source edits.

## Occurrence reconciliation

The owner clarification requires JEV evaluation of every occurrence of “native,” including docs, comments, and fixtures. Sequential full-file review covered all eight inventory files and 585 supplied newline-counted lines through EOF (coverage.tsv). Reconciliation pass preserved those reads; relevant source/docs were read again without searches. The crate contains zero occurrences of the word `native`, case variants, compounds or uses. `occurrences.tsv` records this explicit zero-occurrence result and points to full coverage. Thus there is no occurrence to submit for JEV semantic classification and no missing occurrence judgment. No attempt was made to manufacture an occurrence or treat earlier nearby JEV questions as occurrence-specific.

The prior successful actual declarative JEV call and all three answers remain preserved in judgments.md. They address the tool-selection/admission code surrounding the zero-occurrence audit: tool-only selection (Q1), heuristic-only `systemd_status` label (Q2), and generic cache strategy bypass without proof of unauthorized execution (Q3). They do not substitute for a nonexistent native-occurrence judgment.

No `/var/tmp/native-line-audit-20261024.ILroaQ/crates/op-dynamic-loader/coordinator-followup.md` was present at the requested path. The updated common instructions and cross-crate reconciliation request were read in full.

Repeated-request reconciliation: the supplied `source-coverage-receipt-v2.json` reports all 8 inventory files covered without missing lines or column-limited reads. `changed-since-inventory.tsv` has only its header (no changed entries reported). The assignment directory listing still contains no coordinator-followup.md. These supplied receipts corroborate the retained source coverage; no new source-read, hash-computation or runtime-check claim is made.

## Coverage and source findings

All 8 inventory files read sequentially through EOF, including documentation and the embedded cache-capacity test (585 supplied newline-counted lines). Crate root and src directory listings match inventory. No extra nested directories/instructions, binaries, symlinks, missing files, generated source or lockfiles were listed. No source read was truncated. Initial hashes in coverage.tsv are supplied pre-audit receipts, not independently recomputed current hashes. HEAD was not queried.

No provider/model/config override, hard-coded native schema/path, native billing binding, or tched/threetched logic found. `DynamicToolRegistry::get_tool` caches the caller-named tool from its supplied base registry; `ExecutionAwareLoader` constructs an execution-history loading strategy. Prior JEV Q1 agrees (confidence 1). This is source-local evidence, not external caller certification.

`loading_strategy.rs:90-103` contains the historical `systemd_status` tool label. It only participates in critical-tool membership, priority and TTL; this crate does not invoke systemctl. Preserve the literal rather than guessing a new tool contract (prior JEV Q2, confidence 1).

## Generic source observations — outside native-only repair scope

### G1 — cache hits bypass the strategy's context

Evidence: `src/dynamic_registry.rs:53-78`; `src/loading_strategy.rs:6-16,35-57`.
The cache is keyed only by tool name, and a hit returns before `LoadingStrategy::should_load(name, context)`. A custom context-sensitive strategy therefore cannot veto a previously cached tool. The bundled SmartLoadingStrategy ignores context and always allows misses. This is a generic cache/strategy contract concern, not native model selection. Returning a tool is not proof that executing it bypasses downstream admission; external callers/ToolRegistry/Tool implementations were not audited. JEV Q3 preserves that distinction (confidence 1). No exploit severity assigned without those links. No edit under native-only scope.

### G2 — documented cache TTL is not enforced here

Evidence: `src/execution_aware_loader.rs:25-28` calls 300 a five-minute base TTL; `src/loading_strategy.rs:15-16,80-87` exposes/computes TTL; `src/dynamic_registry.rs:1-154` implements an LRU without timestamps/expiry or a `cache_ttl` call. `get_priority` is also not consumed by this crate's lookup path. Generic documentation/implementation drift; no runtime expiry claim or fix.

### G3 — historical dependency documentation drift

Evidence: `SPEC.md:41-44,77-81`, `compare-op-dynamic-loader.md:26,48-53` list op-core; current `Cargo.toml:20-22` lists only op-tools and op-execution-tracker internally. Preserve under native-only scope; no general documentation cleanup.

## Changes and overlap

- Crate source changed: none; no BEFORE copy needed.
- Proposed cross-crate/tched hunks: none.
- Existing source/dirty work preserved; no Git mutation.
- Reports include status.md, coverage.tsv, occurrences.tsv, judgments.md and findings.md in the exclusive report directory.

## Verification and limitations

Evidence: full sequential source reads, directory/inventory parity, reread occurrence contexts, and the three preserved successful declarative JEV judgments. No searches, shell/process commands, Cargo, builds, tests, lint, format, smoke fixtures, runtime operations, deploy/seal/grant/credential changes or recursive delegation. No build/test/install/deploy status claimed. No native patch requires building.
