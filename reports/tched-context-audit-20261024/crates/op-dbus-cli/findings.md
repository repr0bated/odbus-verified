# Findings — op-dbus-cli tched context audit

- Scope: only `crates/op-dbus-cli/` source, per fresh task `tched-context-audit-20261024/op-dbus-cli`.
- All four inventory files were read sequentially through EOF; exact spans are in `coverage.tsv`.
- No case-insensitive `tched` occurrence was found. The CLI's names and behavior are generic D-Bus plugin introspection/invocation, not references to the external `threetched` executable/surface, `tched_router`, or 3tchedFS.
- No tched-specific source correction is indicated. No source changes were made.
- Declarative JEV first returned low-confidence insufficiency for the initially summarized reads; after complete raw contiguous reads, the fresh final JEV judgment marked zero-occurrence coverage `complete_zero` (0.98) and no-change `retain` (1.0). Receipts/questions are in `judgments.md`.
- Static source review only. No build, tests, formatting, smoke, runtime, catalog, mount, authorization, or deployment operation was run. Source does not prove live/deployed state.
- Initial inventory hashes were used as receipts and not rehashed; reads were performed sequentially, not atomically. The provided per-crate coordinator-followup file was not present.
