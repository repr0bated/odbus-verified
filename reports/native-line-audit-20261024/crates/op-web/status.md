# op-web native-line audit

Status: complete — static source review only; coordinator v3 evidence gap reconciled.

Task: `native-line-audit-20261024/op-web`. Scope: `crates/op-web/`; 65/65 inventory files read sequentially through EOF. Root, `src/`, and nested directory listings reconcile with the inventory.

Nine case-insensitive native occurrences are individually ledgered in `occurrences.tsv` and evaluated in `judgments.md`: five accurate TLS/parser terms retained; four unsupported/stale/ambiguous claims corrected. Follow-up J7 further narrowed the `tools.rs` prompt to dynamic registry discovery (`list_tools` and `get_tool_schema`), removing the unsupported NetworkManager-specific availability promise.

Coordinator v3 source-display gap resolved with supplemental complete rereads: current `handlers/chat.rs` 1-925, `orchestrator/execution.rs` 1-324 through EOF, and `orchestrator/parsing.rs` 1-185 through EOF. The v3 receipt's execution lines 325-339 are beyond the current file EOF; no current source interval remains unread. Chat contains no native occurrence; existing execution/parsing native decisions remain supported by preserved JEV evidence.

Reports: `coverage.tsv`, `occurrences.tsv`, `judgments.md`, `findings.md`. Assignment receipt: `/var/tmp/native-line-audit-20261024.ILroaQ/crates/op-web/result.json`.

No searches, shell/process use, Cargo/build/tests/lint/format/smoke/runtime/deployment actions. Initial inventory hashes are supplied pre-audit hashes, not recomputed; reads were not atomic. Source fixes are not build- or runtime-verified. No deployment claim.