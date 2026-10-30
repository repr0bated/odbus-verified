# Fresh contextual JEV judgments

## README historical-capture wording (`examples/README.md:29`)

Input supplied to declarative `judge`: exact sentence plus surrounding README lines 28–32, and coordinator evidence noting current source retirement of `tched_router`, current source registration of `threetched`, generation-216 catalog containing both IDs, and the explicit limit that neither alone proves runtime status. Question asked whether wording identifies historical capture rather than current-schema authority and whether a tched-specific edit is needed; alternatives were retain, correct, insufficient.

Result: **retain**, confidence **0.85**; probabilities retain 0.90, correct 0.03, insufficient 0.07. Human disposition: retain. Rationale: the README explicitly says the files are historical captures, preserves provenance, and directs regeneration from intended source; it does not claim that `tched_router` is the current schema authority. This does not establish deployment/runtime state.

Three occurrences were observed and judged; see the occurrence ledger. This completes the assigned inventory review.

## Introspection gap identifier (`examples/zeroclaw-introspect-gaps.json:2220-2221`)

Input supplied to declarative `judge`: both exact path entries, plus their placement in the upstream `rust_zeroclaw_structs` missing-path list and adjacent API type names. Asked whether the `tched` substring is tched-specific, whether a source change is warranted, and whether retaining the upstream path preserves meaning.

Result: **incidental**, confidence **1.00** (probabilities relevant 0, incidental 1, uncertain 0). Human disposition: retain unchanged. `FetchedCatalog` is an upstream field/type name with ordinary English morphology; no tched-specific runtime semantics or scoped defect is evidenced. The two line occurrences are separately recorded.

No other `tched` substrings observed during full sequential review of the crate inventory. No tched-specific source edits were warranted. No searches, Cargo, tests, runtime actions, delegation, or deployment were performed.
