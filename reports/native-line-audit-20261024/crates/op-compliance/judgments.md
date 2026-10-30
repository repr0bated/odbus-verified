# Declarative JEV receipts — op-compliance

Task: native-line-audit-20261024. Date: 2026-10-24. Auditor: Codex crate worker.

## Facility and evidence

Read `xd://eval/judge`; successfully invoked installed `await judge(compliance_evidence, compliance_questions)` in a single Python eval cell titled **Judging compliance source context**. No routing, source extraction, file traversal, filtering or arbitrary execution used. Exact source evidence was supplied in the call from `crates/op-compliance/src/lib.rs:37-53,93-121,301-324`, together with full-file read coverage (`Cargo.toml:1-12`, `src/lib.rs:1-180;181-324`). No `native` occurrence was observed in either complete file; no occurrence was invented for judging.

Source evidence in the successful call included these complete functions/blocks:
- `EugeneRisk::validate_ai_risk` and its struct/comment: checks `plugin_type == "custom"`, presence of supplied `schema.model_name`, absence of `training_data_source`, returning `ComplianceError::Validation`; otherwise returns `Ok(())`.
- `LawFirm::review_plugin_schema`: serializes the caller's `&op_state_store::PluginSchema`, derives `plugin_type` from its `category` and `schema` from `schema.to_json_schema()`, then calls structural and four attorney validators.
- `LawFirm::review_schema`: parses the supplied JSON string, validates the legacy structure, and calls the same attorney validators.
- The entire `#[test] fn test_ai_act_violation` uses `model_name: "gpt-4"` in JSON fixture metadata, calls `review_schema`, and asserts an AI Act error.
- The entire typed-schema test builds a `PluginSchema` and calls `review_plugin_schema`.

The exact submitted source and declarative question objects are retained in the eval call transcript; source snapshots read were `Cargo.toml#D481` and `src/lib.rs#D710`.

## Questions, explicit contrary outcomes, answers

### selection

Question: “Classify the shown model_name validation and LawFirm entry points. Does this evidence implement an implicit provider/model/config selection override, only inspect supplied schema values, or lack sufficient evidence? Do not equate a validation failure with selecting a model.”

Criteria:
- `inspection_only`: Checks supplied schema metadata and returns validation errors, without writing authoritative model/provider configuration or selecting a runtime route.
- `implicit_override`: Writes or selects a runtime provider/model/config independently of an admitted request.
- `insufficient`: Evidence does not distinguish these behaviors.

Actual result: `{"type":"choice","choice":"inspection_only","confidence":1,"probabilities":{"inspection_only":1,"implicit_override":0,"insufficient":0}}`.

Decision: no native-specific override to remove; preserve model metadata validation.

### fixture

Question: “Classify the gpt-4 string at lib.rs:308 using cfg(test) and the surrounding call/assertions.”

Criteria:
- `test_data`: Example metadata used only to exercise missing-training-data validation; not a production model default.
- `runtime_default`: The shown value sets a production model/provider default.
- `insufficient`: Context is insufficient to classify.

Actual result: `{"type":"choice","choice":"test_data","confidence":1,"probabilities":{"test_data":1,"runtime_default":0,"insufficient":0}}`.

Decision: keep fixture string; no blanket model-name replacement.

### authority

Question: “What does review_plugin_schema establish about schema authority and identity admission? Distinguish validating a derived JSON view from creating a second authoritative plugin schema. Do not infer deployment or caller authentication.”

Criteria:
- `derived_validation_only`: Consumes the caller-supplied typed PluginSchema and derives a JSON validation view; these methods alone do not establish authenticated caller admission or live deployment.
- `parallel_authority`: Loads a separately maintained external plugin schema and makes it authoritative.
- `admission_proven`: The displayed entry point authenticates a principal and verifies exact grants.
- `insufficient`: Cannot distinguish from the provided source.

Actual result: `{"type":"choice","choice":"derived_validation_only","confidence":1,"probabilities":{"derived_validation_only":1,"parallel_authority":0,"admission_proven":0,"insufficient":0}}`.

Decision: no parallel schema authority or identity bypass demonstrated here. Caller admission and deployment remain outside what this crate's validation API establishes.

JEV judgments are source-evidence classifications, not execution proof, legal certification, or permission to mutate.

## Occurrence reconciliation

Resumed this same crate session after reading the updated common assignment in full. Earlier mandatory preload and complete reads remain evidence. The assigned directory listing contains no `coordinator-followup.md`.

Fresh sequential rereads: `Cargo.toml:1-12` (requested 1-40), `src/lib.rs:1-180;181-324` (second request 181-360). EOF confirmed at 12 and 324; snapshots remain `D481` and `D710`. Root and source directory listings still agree with the two-file inventory.

Exact occurrence count: **0**, including case variants and compound forms, across all crate code, comments, documentation and embedded fixtures. `occurrences.tsv` contains two explicitly labelled file-absence rows, not fabricated token occurrences. See `coverage.tsv` for full-file coverage.

The three successful JEV receipts above remain source-context evidence but are not represented as an exhaustive token scan or as judgments of nonexistent occurrences. No additional token-context judgment is applicable: the clarified instructions explicitly permit zero-occurrence conclusions linked to full-file coverage. No new eval execution, search, source extraction or synthetic occurrence was used. No native-specific wording or semantic claim exists in this crate to correct.
