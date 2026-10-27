# Declarative JEV evidence

Fresh task call, `eval` using `judge(state, questions)` after reading the assigned tool instructions. The state included the exact source occurrence excerpts, local call behavior, project naming distinction, competing interpretations, and coordinator's current note that source retires `tched_router` while source registers `threetched`, whereas mounted generation 216 reports both IDs. It explicitly disclaimed runtime/deployment proof.

## JEV-O1 — `tched_router` example (`blob_schema.rs:53`)

Questions: classify the identifier's meaning; choose the smallest safe treatment of this user-facing schema example. Candidate actions included retaining it, replacing it with `threetched`, removing it, or reporting insufficient authority.

- Meaning: `legacy_or_current_plugin_identifier`, confidence **0.41**; probabilities: legacy/current identifier 0.61, stale/misleading 0.19, other/ambiguous 0.20.
- Change: `remove_example` was the top choice, confidence **0.27**; probabilities: replace with `threetched` 0.11, retain `tched_router` 0.02, remove example 0.45, insufficient 0.42.
- Assessment: both answers are low confidence; they are not treated as decisive. Human disposition removes the named example because source-level naming and catalog-note naming conflict, and the field accepts an arbitrary ID present in the sealed catalog. The neutral wording is accurate without asserting either ID's current or deployed status. This is the smallest safe change and does not alter the request contract.

## JEV-O2 — `matched_methods` response key (`blob_schema.rs:240`)

The source builds `method_hits` from method names matching the query and emits them under the response key `matched_methods`.

- Meaning: `ordinary_word`, confidence **1.0** (probability 1.0).
- Change: `retain`, confidence **1.0** (probability 1.0).
- Human disposition: retain; the substring is ordinary English and changing the public response key would create unnecessary incompatibility.

**Locator corrections:** the initial JEV state labeled O1 as line 47 and O2 as line 247; direct numbered source reads confirmed original lines 53 and 240, respectively. Exact occurrence text and semantics were provided; the answers remain applicable. Ledger and reports use corrected coordinates.

## Boundaries

JEV is evidence, not authorization or runtime proof. No catalog operation or external source read was performed for this crate audit. Current catalog/deployment state remains unverified.