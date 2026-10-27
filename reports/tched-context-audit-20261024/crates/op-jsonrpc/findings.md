# Findings — op-jsonrpc tched audit

- **No tched occurrence in scope.** All seven inventory files were directly read sequentially through EOF; their line totals match the inventory (1,718 lines). No case-insensitive `tched` substring, compound, ordinary word, product name, executable, filesystem, plugin, route, or identity reference was observed.
- **No tched-specific correction justified.** Nothing in the crate's source contract creates a concrete tched-related change. JEV's fresh contextual disposition is `no_tched_specific_change` at confidence 1.0.
- `SPEC.md` describes capabilities that are not exported in the current crate root (for example, NonNet modules); this is unrelated to the tched audit and is not changed or treated as runtime evidence.

**Changes:** none in crate source.  
**Limitations:** static source only. Runtime catalog, current deployment, and live behavior were not verified; all runtime/deployment claims remain unverified.  
**Verification:** full sequential file reads and the declarative JEV call are the audit evidence. No Cargo or test/check/lint/format/smoke/runtime/deployment command was run, as prohibited by the assignment.