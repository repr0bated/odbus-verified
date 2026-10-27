# op-cache — routing and model-selection audit

Task: `tched-routing-audit-20261024/op-cache`. Auditor: Codex crate worker. Date: 2026-10-24.

## Outcome

All 28 supplied inventory files read sequentially through EOF: 13,168 lines; inventory records 452,136 baseline bytes. Zero `tched` substrings observed, including substrings in ordinary words. No occurrence-specific routing migration, deletion or rename recommended. Crate source, manifest and tests unchanged by this worker.

| Classification | Count |
| --- | ---: |
| All tched occurrences | 0 |
| routing_dispatch | 0 |
| model_selection_calling | 0 |
| filesystem_projection | 0 |
| contract_identifier | 0 |
| ordinary_word | 0 |
| dead_end | 0 |
| alive | 0 |
| not_applicable | 0 |
| repoint_to_op_llm | 0 |
| repoint_to_threetched_fuse | 0 |
| delete_dead_end | 0 |
| rename_to_3tched | 0 |
| rename_to_threetched | 0 |
| leave_as_is | 0 |
| Uncertain occurrence dispositions | 0 |

## Occurrence ledger

| Path | Line | Token/context | Role | Integrity | Required action | Jev receipt |
| --- | --- | --- | --- | --- | --- | --- |

No rows: the complete sequential read found no occurrences. A zero count does not create a synthetic leave-as-is row.

## Execution-context interpretation

- `src/agent_registry.rs:427-438`: `AgentRegistry::execute` retrieves the registered executor and invokes `executor(input).await`. This is generic function dispatch, not demonstrated model/provider selection or retired-router invocation. Jev J1: `generic_callbacks_only`, confidence 1.
- `src/grpc/agent_service.rs:83-96`: local execution requires a registered local executor; otherwise returns `failed_precondition`. Remote-registration endpoint storage elsewhere in this file does not establish a working remote receiver, an external model route or a threetched route. No speculative repointing recommended.
- `src/grpc/server.rs:43-53,116-118,158-163`: both built-in serving entry points propagate the always-error standalone-ingress guard before listener creation. Jev J2: `fail_closed_before_bind`, confidence 0.98. This retired generic compatibility ingress is not counted as a tched-associated dead end.
- Capability resolution, workstack selection and workflow callbacks do not by themselves establish model selection. Callback implementations supplied by other crates and deployed receivers were not inspected; no live availability claim.

## Authority and uncertainties

The common assignment declares `tched_router` retired and identifies `threetched` as the external binary surface, with identity-bound FUSE ingress. Older startup guidance contains historical router descriptions; those do not establish present runtime authority. No occurrence in this crate requires resolving that naming conflict.

The absence count is the primary reader's complete-file observation. Jev reviewed exact execution excerpts and reporting limits, not every file independently; J3 selects `qualified_static_report`, confidence 1. Exact submitted contexts, questions, returned answers and corrected source-range labels are retained in `judgments.md`.

No ambiguous occurrence, contradictory Jev disposition or confidence below 0.6. Initial SHA-256 values in `coverage.tsv` are supplied inventory receipts, not recomputed current hashes. Reading is sequential per file, not an atomic repository snapshot; concurrent work is preserved. The already-present NUMA comment correction visible at `src/numa.rs:1189-1190` belongs to the separate native audit, not this worker.

## Coverage and verification

- Explicit raw line ranges cover every inventory file from line 1 through its observed EOF. All 28 observed line totals match the inventory.
- Crate-root, src and src/grpc directory listings reconcile the inventory; examples/proto entries were visible in the root listing. No nested context file appeared in those complete listings.
- Report assembly reconciled 28 inventory-ordered coverage rows, contiguous ranges and 13,168 total lines. No source search/extraction was used.
- No source edits, Cargo/build/test/lint/format/custom-smoke/runtime/deploy/seal/grant/credential or Git operations performed. Only exclusive reports and mandatory shared HANDOFF bookkeeping changed.
- This is a static audit, not a runtime investigation or verification pass. No built, installed, deployed or tested claim.

## Coordinator action

Consume these five reports. No tched-related patch from op-cache requires integration. Keep unrelated cache/SQLite/NUMA concerns outside this naming/routing assignment; no new source work is proposed here.

## Self-evaluation

| Axis | Score | Evidence / limitation |
| --- | ---: | --- |
| Accuracy | 4/5 | Exact excerpts and actual Jev answers retained; initial hashes are supplied, not independently rehashed. |
| Completeness | 4/5 | All 28 files/13,168 lines covered; no atomic-snapshot guarantee during concurrent audits. |
| Clarity | 4/5 | Zero-occurrence ledger separated from generic execution observations; detailed receipts add volume. |
| Actionability | 4/5 | All required reports and explicit no-patch disposition available; external callback implementations intentionally unexamined. |
| Conciseness | 4/5 | Findings/counts and machine summary overlap to satisfy both deliverable formats. |

Overall: 4.0/5. Improvements: coordinator should compare initial hashes with its authoritative snapshot if snapshot identity is required; do not turn static absence into runtime health. Self-check: the reports expose those limits rather than implying a live routing pass.
