# Declarative JEV receipt — `op-introspection`

One successful declarative `judge(state, questions)` call was made through eval after reading `xd://eval/judge`. It was not model routing. State supplied: fresh full-read inventory coverage for the 10 assigned files and their line counts (Cargo.toml 31, SPEC.md 109, compare-op-introspection.md 88, cache.rs 44, indexer.rs 531, indexer_manager.rs 52, lib.rs 110, parser.rs 26, projection.rs 234, scanner.rs 479); stated zero case-insensitive `tched` occurrences; static-source-only scope and no runtime/deployment inference.

Question `coverage`: “Classify the provided complete source-read evidence regarding occurrence coverage.” Criteria: `complete_zero_occurrences` = all inventory files read through EOF and no tched substring appears, so no occurrence-level judgment applies; `occurrence_or_gap` = an occurrence or read-coverage gap exists; `insufficient` = evidence cannot justify either result.

Answer: `complete_zero_occurrences`, confidence 0.97; probabilities complete_zero_occurrences 0.98, occurrence_or_gap 0.01, insufficient 0.01.

Question `change`: “Does the provided source evidence support a tched-specific edit within this crate? Do not propose generic cleanups or infer deployed behavior.” Criteria: `no_tched_specific_change` = no occurrence or tched-specific contract requiring correction is provided; `change_needed` = a specific tched-related correction is supported; `insufficient` = evidence cannot determine.

Answer: `no_tched_specific_change`, confidence 1.00; probabilities no_tched_specific_change 1.00, change_needed 0, insufficient 0.

These judgments support a zero-occurrence audit disposition, not a runtime or authorization claim.
