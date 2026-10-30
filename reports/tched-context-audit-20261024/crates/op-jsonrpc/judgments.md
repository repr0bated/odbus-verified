# Declarative JEV receipt — op-jsonrpc

**Successful calls:** 1 (`eval` using `judge(state, questions)`; no model routing request).  
**State supplied:** Fresh task/scope; inventory of all seven paths and listed line counts; full sequential EOF read receipt; directory listing showing those files only; explicit no-occurrence observation; crate purpose/module summary; scope restrictions; note that the optional coordinator follow-up file was absent. No prior crate audit conclusions were reused.

## Questions and criteria

1. **`coverage`** — Given the listed inventory and direct sequential EOF read receipts, what is the best supported coverage disposition? `complete_zero_occurrences` only if every listed file and full range is accounted for and no occurrence was found; otherwise `partial_or_unresolved`.
   - `complete_zero_occurrences`: all seven inventory files and full listed ranges were read through EOF; no case-insensitive `tched` substring occurs.
   - `partial_or_unresolved`: a file/range is unaccounted for or an occurrence remains unidentified.
2. **`change`** — Does crate evidence support a tched-specific source correction, considering purpose, current contract context, evidence strength, and narrow scope? Choices: `no_tched_specific_change`, `make_a_specific_tched_correction`, `insufficient_evidence`.
   - `no_tched_specific_change`: no occurrence or affected contract; no supported edit.
   - `make_a_specific_tched_correction`: concrete tched-related defect with smallest correction evidenced.
   - `insufficient_evidence`: evidence cannot support disposition despite a possible concrete issue.

## Result (verbatim structured answer)

```json
{
  "coverage": {
    "choice": "complete_zero_occurrences",
    "confidence": 1,
    "probabilities": {
      "complete_zero_occurrences": 1,
      "partial_or_unresolved": 0
    }
  },
  "change": {
    "choice": "no_tched_specific_change",
    "confidence": 1,
    "probabilities": {
      "no_tched_specific_change": 1,
      "make_a_specific_tched_correction": 0,
      "insufficient_evidence": 0
    }
  }
}
```

**Human disposition:** accept the coverage classification against the sequential read ledger; no occurrence-level disposition applies because no occurrences exist. Make no source change. JEV is a source-audit judgment, not runtime or deployment proof.