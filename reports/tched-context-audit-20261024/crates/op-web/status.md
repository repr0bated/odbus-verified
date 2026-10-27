# op-web tched context audit — partial

Task: `tched-context-audit-20261024`, assigned crate `op-web`, owner Luna, 2026-10-24.

## Coverage

- Read all 65 inventory files sequentially through EOF. `coverage.tsv` records each file/range; `build.rs` has an inventory-vs-file line-count discrepancy (listed 5, read through actual line 6).
- Source reads were static. No source searches or prior tched audit results were used.
- This is **not complete**: exact per-occurrence enumeration and contextual JEV judgment for every case-insensitive `tched` substring—including incidental `matched`/`fetched`/`dispatched` terms—were not completed. `occurrences.tsv` is an explicitly partial issue-group ledger, not the required exhaustive ledger. Total occurrence count is unknown.

## Main finding

`handlers/tched_router.rs` and its callers actively dispatch the old plugin ID from `tched_router_routes::ROUTER_PLUGIN_ID`, invoke `Chat`/`ListModels` with `cap.software.3tched-router.*` capabilities, read that ID's sealed schema/state, and expose its model catalog. `routes/llm.rs` actively returns the old plugin identity. `routes/mod.rs` wires old HTTP paths to parts of this surface. The assignment coordinator's source note says current registry source retires `tched_router` and registers binary-captured `threetched`; it also warns source/live identities are not universal runtime proof.

The source comment at `src/tched_router_routes.rs:12` was corrected from the unsupported claim that the old ID is canonical after rebrand to the accurate statement that current op-web compatibility paths reference it. The active runtime ID/method/capability callers remain unchanged: the canonical replacement schema is unavailable, and guessing would violate sealed-schema authority. Exact BEFORE file is `BEFORE-tched_router_routes.rs`.

## JEV

Declarative batch `jdgb-15b33b70757c9360`, model `typesafe/jev-1.14.0`, 6/6 answered. Full output in `judgments.md`.
- Active handler path: `fix`, confidence 0.76; probability fix 0.81, insufficient 0.17.
- Active LLM model route/helper: `fix`, confidence 0.75; probability fix 0.80, insufficient 0.16.
- HTTP path wiring/compatibility: `insufficient`, confidence 0.31; retain-compat 0.45, insufficient 0.44.
- SSE allowlist: `insufficient`, confidence 0.69; insufficient 0.75.
- agents helper: `insufficient`, confidence 0.41; unrelated 0.42, insufficient 0.53.
- Historical prose: `insufficient`, confidence 0.60; insufficient 0.68.

These judgments were over grouped contexts and do not satisfy the required one-occurrence (or fully mapped equivalent) JEV coverage. They are not evidence of runtime/deployment state.

## Verification / disposition

No tests, build, lint, formatting, smoke, runtime, deployment, seal, grant, credential, or git commands were run; the assignment forbids them. One source comment was corrected, with exact BEFORE file preserved. Reports and assignment receipt are marked partial. The next required source input is the authoritative current `threetched` schema/API contract and an owner decision on retaining the old HTTP URL surface; thereafter finish the case-insensitive occurrence-by-occurrence ledger and JEV review before behavioral edits.
