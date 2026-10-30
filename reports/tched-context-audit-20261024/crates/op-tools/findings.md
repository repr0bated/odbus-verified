# Findings — fresh op-tools tched audit

## Current source disposition

This continuation reread all 31 current inventory files from line 1, in inventory order, sequentially through each reader-reported EOF. No case-insensitive `tched` occurrence was observed. Accordingly, there are no local source contexts needing a meaning/intent judgment and no tched-specific correction to implement. `occurrences.tsv` remains intentionally header-only; it contains no fabricated occurrence or JEV judgment.

Fresh declarative JEV rated current-source coverage `complete_zero_current_source` (confidence 0.82), no-occurrence/no-change `0.98`, and runtime unverified `1.00`. Full question/input summaries and probabilities are in `judgments.md`. No source file changed.

## Inventory/current-source discrepancy

`inventory.tsv` records `crates/op-tools/src/security.rs` as 783 lines / 26,484 bytes. The current file reader reports 762 total lines; a direct raw request for 763-783 returned “Line 763 is beyond end of file (762 lines total).” This continuation reread the current file through EOF at line 762. Cause and equivalence to the inventory snapshot are not established; the coverage ledger preserves the inventory count and current EOF separately. This discrepancy does not by itself establish a tched-specific source defect.

## Limits

This is static source evidence only. It does not establish current plugin/catalog membership, runtime behavior, deployment state, or whether any source component is active. No search, Cargo/build/check/test/Clippy/fmt/custom smoke/runtime/deploy/seal/grant/credential action or delegation was used, per user/assignment restrictions.