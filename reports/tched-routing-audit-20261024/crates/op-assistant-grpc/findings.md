# op-assistant-grpc routing and model selection audit

Task `tched-routing-audit-20261024/op-assistant-grpc`; Codex crate worker; 2026-10-24.

## Outcome

All 31 supplied files read sequentially through EOF: 3,655 lines. Zero literal `tched` substrings observed, including substrings inside ordinary words. No occurrence-specific dead ends, reroutes, deletions, renames or leaves. This is a complete static read of the assigned inventory, not proof of live deployment or working provider/model execution.

`coverage.tsv` records every path, actual sequential spans consumed and supplied initial SHA-256. Root, src, src/bin, proto, proto/assistant and tests listings reconcile with the inventory; no nested operating-guidance files encountered. No search/filter/extraction tools used. Checksums are copied initial inventory receipts, not independently recomputed current hashes. No source files changed.

## Complete occurrence ledger

| File:line | Token | Role | Integrity | Required action | JEV receipt |
| --- | --- | --- | --- | --- | --- |
| None across all 31 files | — | — | — | — | No occurrence-level question exists |

Counts: routing_dispatch 0; model_selection_calling 0; filesystem_projection 0; contract_identifier 0; ordinary_word 0. Integrity: dead_end 0; alive 0; not_applicable 0. Actions: repoint_to_op_llm 0; repoint_to_threetched_fuse 0; delete_dead_end 0; rename_to_3tched 0; rename_to_threetched 0; leave_as_is 0. Zero leaves means zero occurrence rows, not zero files preserved; all 31 source files were left unchanged.

## Context and uncertainty: U1 — model wrapper is not a demonstrated retired-router caller

- `src/models.rs:24-62` forwards `models.list`, `models.get`, `models.switch` through `AssistantClient`; `switch_model` preserves request model/agent/session IDs in its JSON parameters.
- `src/client.rs:30-40` builds a JSON-RPC envelope and delegates to the configured transport.
- `src/transport.rs:113-143` dispatches D-Bus only; direct HTTP RPC returns an explicit transport error. The configured destination/path/interface are used for the D-Bus method call. There is no `:8084`, `tched_router`, model-family dispatcher or direct provider generation call in the audited files.
- `src/server.rs:110-113` places ModelService behind `wireguard_auth_interceptor`; `src/auth.rs:23-50` rejects unverified credentials and returns unavailable even after structurally opening SID1. This establishes fail-closed source behavior, not a runtime measurement.
- `src/dbus_service.rs:20-28` also refuses its generic standalone passthrough with AccessDenied.

Competing options: migration to `op-llm` would require evidence that the intended operation is direct model/provider selection; external autonomous-agent operations would require the identity-bound threetched FUSE contract; obsolete compatibility wrappers might instead be deleted. No external receiver/caller read or runtime operation was authorized in this single-crate occurrence pass, and no literal tched occurrence ties these wrappers to the retired plugin. Do not choose or count any of those migrations from method names alone. No ambiguity about a specific occurrence remains because the occurrence set is empty. JEV `model_route`: no_tched_specific_finding, confidence 0.92.

## Separate non-tched observation: G1 — stale module documentation

`src/lib.rs:8-10` says authentication verifies genesis and D-Bus failure falls back to JSON-RPC over HTTP. This conflicts with the actual fail-closed bodies in `src/auth.rs:23-50` and `src/transport.rs:113-122`. The stale wording is not evidence that fallback executes or that retired-router dispatch exists. Report-only generic documentation drift; no source mutation and no tched count. JEV `documentation`: generic_documentation_drift, confidence 1.

## Evidence boundary

`judgments.md` retains the exact supplied state, source contexts, typed questions/criteria, returned answers, confidence and probability distributions: one installed declarative JEV call, three answers, confidence 0.92–1. Whole-crate absence is the sequential reader observation; JEV's selected excerpts do not independently certify absence. Its completion answer supports static_report_only, confidence 1.

No Cargo, builds, tests, lint, formatting, smoke fixtures, runtime operations, deployment/sealing, grants or credential changes. No commit/staging/reset/stash. No provider availability, external receiver, invocation equivalence or live catalog claim. HEAD not queried. Shared HANDOFF bookkeeping only satisfies the higher-priority session contract; no other shared docs changed.

## Coordinator next action

Reconcile the 31 coverage rows, empty occurrence ledger and three JEV receipts into the task aggregate. No source patch from this crate requires building. G1 is optional separately scoped documentation work; U1 must not be converted into a confirmed reroute without external caller/receiver evidence.

## Output self-evaluation

| Axis | Score | Evidence / limit |
| --- | --- | --- |
| Accuracy | 4/5 | Exact source ranges and JEV receipts; initial hashes not recomputed, no runtime claim. |
| Completeness | 4/5 | All 31 inventory files / 3,655 lines; concurrent later source changes are not covered by a static read. |
| Clarity | 4/5 | Explicit empty ledger and separate U1/G1; source-reading and supplied-hash provenance require distinct evidence records. |
| Actionability | 4/5 | Coordinator reconciliation specified; external receiver migration intentionally undecided for lack of scoped evidence. |
| Conciseness | 4/5 | Required receipts preserve full questions/context; some provenance repeats across required report files. |

Overall 4.0/5. No critical self-evaluation issue. Improvement: preserve source snapshot/receipt linkage when aggregating concurrent audits; do not expand this report's static claims. Self-check: deliver as-is within the requested report-only boundary.
