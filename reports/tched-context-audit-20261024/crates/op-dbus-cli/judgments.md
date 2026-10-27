# Declarative JEV judgments — op-dbus-cli

Fresh source evidence: all four files in `inventory.tsv` were read sequentially through EOF, using contiguous raw line ranges. The directory listing reconciles with the inventory. No case-insensitive `tched` occurrence appears in the source. Local source contract: a generic `dbus-plugin-cli` introspects and invokes methods on the `org.opdbus.v1.plugins` D-Bus tree and displays the configured session identity. No product-specific `threetched`, `tched_router`, or 3tchedFS wording appears in this crate.

JEV execution: installed declarative `judge(state, questions)` through the eval prelude; no model override, code extraction, search, or routing was used.

1. Initial coverage/change assessment (successful call; response retained):
   - Coverage: `insufficient` 0.95 confidence, because initial state referenced rendered-output gaps.
   - Change: `retain` 0.89 confidence.
   - The initial insufficient coverage answer was not accepted as final; I then obtained complete raw source ranges.
2. Final assessment (successful call; response retained):
   - Coverage: `complete_zero` 0.98 confidence (probability 0.99).
   - Change: `retain` confidence 1.0 (probability 1.0).
   - State supplied exact file paths, complete raw line spans, EOF claims, matching directory listing, local CLI/D-Bus contract, no product-specific reference, and explicit source-vs-runtime limitation.

No occurrence-level judgment was necessary because the complete sequential review yielded zero occurrences. This is not a claim about live catalog membership, deployed state, or runtime behavior.
