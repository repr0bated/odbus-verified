# Declarative JEV receipts

## J1 — Local meaning, accuracy, change and risk

**Evidence supplied:** `src/plugin_schema.rs:170` is a canonical OSCAL capability-form documentation example, immediately preceding the `CapabilityDecl.id` field. `:1884` is the positive `is_canonical_capability_id` predicate fixture; `:1887` is the negative fixture lacking the subject segment. The predicate's implementation checks the `cap.` prefix and terminal `@vN` syntax. The supplied owner context distinguishes the `threetched` executable/schema from `tched_router`, says not to infer runtime status or apply substring renames, and preserves plugin schema as the sole declaration point separate from principal grants.

**Questions and results:**
- Meaning across all three: `canonical_example`, confidence 0.99 (probabilities: canonical_example .99, historical provenance .01; actual plugin contract 0).
- Wording disposition: `retain`, confidence 0.95 (retain .97, insufficient .03, correct 0).
- Minimal change: `no_change`, confidence 0.98 (no_change .99, insufficient .01, targeted_change 0).
- Misleadingness risk if retained without runtime claims: score 0.23 on 0–4 scale; confidence .83 (0: .81, 1: .18, 2: .01, 3/4: 0).

Human disposition: retain all three as illustrative canonical capability identifiers. They are not evidence of a live registration, grant or deployment.

## J2 — Read coverage and completion status

**Evidence supplied:** Full path inventory, exact sequential contiguous ranges to EOF for all 18 files, line totals, three individual occurrence paths/lines/contexts, and statement that reports had not yet been written.

**Questions and results:**
- Coverage: `complete`, confidence 1.00.
- Occurrence disposition completeness: `complete`, confidence 0.62 (complete .75, incomplete .20, insufficient .05). Confidence is moderate; ledger presents all three local contexts and substring positions for coordinator confirmation.
- Completion before reports were written: `not_yet`, confidence 0.43. This correctly described the then-current state; coverage/findings/occurrences/judgments/status and both result receipts have now been written.

## Superseded low-confidence subanswer

J1 also included a coverage question answered `incomplete` at confidence 0.31 (probabilities complete .05, incomplete .54, insufficient .41). This was low confidence and the prompt contained only an aggregate statement, not the exact path/range receipts. Do not use it as the final coverage judgment. J2 re-evaluated coverage with the detailed contiguous ranges and judged complete at 1.00.

## J3 — Final task completion and receipt consistency

**Evidence supplied:** All 18 inventory files read sequentially through EOF; three contexts judged, with J1 no-change disposition; coverage/occurrence/findings/status/judgment reports written; both result receipts read back as complete (18 reviewed, 0 pending, 0 changed files, 3 occurrences); assignment prohibits build/tests/runtime/deployment, none run; those states explicitly unverified.

**Questions and results:**
- Task completion: `complete`, confidence 0.89 (complete .93, partial .07, blocked 0).
- Receipt consistency: `consistent`, confidence 0.94 (consistent .96, inconsistent .04, insufficient 0).

Human disposition: complete static audit; coordinator owns the scoped build after reconciliation.