# Findings — op-mcp-aggregator tched context audit

Date: 2026-10-24  
Scope: `crates/op-mcp-aggregator/` and exclusive audit reports.

## Outcome

All 14 inventory files were read sequentially through EOF. The case-insensitive `tched` substring appears twice, both inside ordinary English `matched` in `config.rs:529` and `config.rs:542`. Contextual declarative JEV classified both as accurate descriptions of client detection pattern matching and recommended retain (confidence 0.97 for change disposition). No tched-specific correction is supported. No crate source file changed.

## Occurrences

See `occurrences.tsv` for exact line/position, context, authority, JEV mapping, disposition and change status. The two log messages occur only after their corresponding pattern predicate succeeds. They identify the client and pattern; they do not refer to product/filesystem/executable names.

## Boundaries and limitations

- Current implementation source establishes only the local client detection behavior and logging semantics. It does not prove build, caller, live runtime, or deployment state.
- Scope is only `op-mcp-aggregator`; no cross-crate conclusions or migration actions.
- Assignment prohibits Cargo/build/check/test/Clippy/fmt/smoke/runtime/deploy/seal/grant/credential actions. None were run.
- No nested agent instructions or coordinator follow-up file were listed in the assigned crate or assignment directory. `compare-op-mcp-aggregator.md` mentions absent/partial historical artifacts (`src/unused/context.rs`, `src/groups.rs.patch`); they are not present in the supplied source inventory/current crate listing. Their reference does not require a tched-specific change and is not treated as runtime authority.
- Runtime/catalog/deployment state remains unverified.
