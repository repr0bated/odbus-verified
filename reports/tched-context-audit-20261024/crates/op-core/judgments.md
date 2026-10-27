# op-core contextual JEV judgments

Date: 2026-10-24. Method: installed declarative `judge(state, questions)` through Eval; not model routing. One occurrence context was supplied in full (source lines 221-246), together with the entire test purpose, the exact API contract from `PrivateProjectionDirectory`, owner authority about sealed catalog membership, and the source-versus-mounted-catalog distinction from the coordinator note. No operation or alternate transport was used.

## JEV call: `judge(state, {meaning, change})`

State included the exact code excerpt from `crates/op-core/src/private_projection_directory_tests.rs:221-246` and contextual source/API/authority described above. Questions:

- `meaning`: decide between `fixture_identifier` (used only as private projection fixture ID, not availability claim), `current_contract_claim`, or `insufficient`; explicitly assess whether test asserts availability.
- `change`: decide whether to retain, correct, or mark insufficient, based on evidence and effects of renaming.

Actual response:

- `meaning`: `fixture_identifier`, confidence `0.89`; probabilities: fixture_identifier `0.93`, current_contract_claim `0`, insufficient `0.07`.
- `change`: `retain`, confidence `0.52`; probabilities: retain `0.68`, correct `0.01`, insufficient `0.31`.

The meaning judgment is clear that the occurrence is a fixture ID, not a current availability claim. The change judgment is low confidence, so it is not accepted as decisive. Human disposition: retain. In the local code, this opaque string selects a test projection document; it does not grant admission, establish catalog membership, or drive a runtime route. The test's semantic contract is concurrent revocation lock-order behavior. Renaming to another arbitrary string would not be justified by evidence that the existing fixture label is false, and could obscure the provenance of the scenario. But the test does not prove `tched_router` is a currently supported plugin. External sealed/runtime contract is not established here; report this limit rather than infer retirement or availability.

Evidence location: Eval output in this session for the `judge` call; its returned typed choices/confidences/probabilities are recorded verbatim above. No repeated reframing or search was used.
