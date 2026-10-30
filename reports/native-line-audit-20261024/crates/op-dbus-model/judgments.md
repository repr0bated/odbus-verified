# Declarative JEV receipts — op-dbus-model

Task: native-line-audit-20261024/op-dbus-model. Date: 2026-10-24.
Facility: installed eval `judge(state, questions)`; documentation read at `xd://eval/judge`.
One actual successful `await judge(state, questions)` call, transcript label `Judging crate source semantics`. No model routing, source search, reranking or arbitrary eval execution.

## State / source references

The single state included the complete executable implementation from `src/lib.rs:1-66`, all structs and field comments from `src/models.rs:1-47`, and the complete `Cargo.toml:1-15` (whitespace compacted in the submitted excerpts). It included exact documentation claims from `SPEC.md:21-24,79-84` and `compare-op-dbus-model.md:23-25,46-50`. This is source evidence, not proof of downstream behavior or deployment.

Full-file coverage: Cargo.toml:1-15; SPEC.md:1-199; compare-op-dbus-model.md:1-63; src/lib.rs:1-66; src/models.rs:1-47. No `native` occurrence was found during this full sequential read; no artificial occurrence was introduced for JEV.

Key submitted code evidence:

```rust
let plugin_name = document.schema.name.clone();
self.documents.write().expect("plugin catalog lock should not be poisoned").insert(plugin_name.clone(), document.clone());
info!(%plugin_name, "Plugin catalog document mirrored in memory");
Ok(())
```

`get_document` performs `.get(name).cloned()`; `list_documents` performs `.values().cloned().collect()` on the same lock-protected map. `PluginCatalogDocument` contains `schema: PluginSchema`, `dbus_path: String`, `service_name: String`, `storage_path: String`, and `source: String`, with serialization derives and no implementation block. `PluginSchema` is imported from `op_state_store`.

Documentation evidence submitted:

- SPEC.md:21-24: `### Database Layer`, `- **Backend**: SQLite via sqlx`, `- **Runtime**: Tokio async runtime`, `- **Schema Management**: Automated table creation and migration`.
- SPEC.md:79-84: `### Schema Creation`, `pub async fn create_schema(pool: &SqlitePool) -> Result<()>`, and the claim of idempotent database initialization.
- compare-op-dbus-model.md:23-25: `- Internal crate integrations: op-core, op-state-store.`
- compare-op-dbus-model.md:46-50 lists sqlx as an external runtime dependency.
- Complete Cargo.toml contains neither sqlx nor op-core; complete lib.rs contains only PluginCatalog and ModelError, not create_schema.

## Questions and actual results

### runtime_selection

Question: Classify this crate implementation: does it implicitly select or override a model/provider/config, versus only store and retrieve caller-supplied catalog data? Do not infer anything about external consumers.

Explicit outcomes:
- `data_mirror_only`: Complete implementation clones supplied documents into memory and reads them back; no provider/model/config selection or rewrite branch.
- `implicit_override`: Code selects or rewrites a provider/model/config independent of caller-supplied document.
- `insufficient_evidence`: Shown implementation insufficient to determine crate-local behavior.

Result: `data_mirror_only`; confidence **1**. Probabilities: data_mirror_only=1; implicit_override=0; insufficient_evidence=0.
Decision: no independent native-specific selection fix in this crate.

### path_and_schema_authority

Question: Do dbus_path, service_name, storage_path and source define an executing alternate config/schema authority here, or are they merely data fields alongside imported PluginSchema? Public data structs alone are not evidence of executing admission bypass. Limit judgment to this crate.

Explicit outcomes:
- `data_fields_no_execution`: Imported PluginSchema held directly; string fields have no executing path/config logic or separate plugin schema definition here.
- `parallel_executing_authority`: Source defines or invokes independent runtime authority or path-based config mutation.
- `insufficient_evidence`: Cannot determine local field behavior.

Result: `data_fields_no_execution`; confidence **1**. Probabilities: data_fields_no_execution=1; parallel_executing_authority=0; insufficient_evidence=0.
Decision: preserve data fields and imported schema; no hard-coded native path, alternate selector or executing admission boundary established here. No claim that external consumers enforce admission.

### documentation_currency

Question: Compare historical documentation claims of SQLite/sqlx, create_schema and op-core with the complete manifest and Rust implementation. Is that a native-specific selection defect, generic stale documentation, or insufficient evidence?

Explicit outcomes:
- `generic_stale_documentation`: Docs describe database API/dependencies absent from shown crate; mismatch is not native routing/config override.
- `native_selection_defect`: Discrepancy proves native provider/model/config override in crate.
- `current_documentation`: Manifest and implementation support SQLx create_schema API.
- `insufficient_evidence`: Shown implementation/manifest cannot establish discrepancy.

Result: `generic_stale_documentation`; confidence **0.91**. Probabilities: generic_stale_documentation=0.93; native_selection_defect=0; current_documentation=0; insufficient_evidence=0.07.
Decision: document generic stale-doc observation; no unrelated doc repair under native-only assignment.

## Verification boundary

JEV classified already-read source. It granted no mutation authority and established no build, test, runtime, admission or deployment result. All three source decisions agree with direct review.

## Occurrence reconciliation — clarified whole-word/case/compound scope

Updated common instructions read in full through line 145. The assignment-directory listing contains no coordinator-followup.md. This continues the existing session; prior mandatory preload, full-file coverage and actual successful JEV receipts above remain evidence.

All five crate files were reread sequentially through EOF for exact occurrence accounting, including every documentation/example/comment/identifier: Cargo.toml:1-15 (#D52F), SPEC.md:1-199 (#BB79), compare-op-dbus-model.md:1-63 (#8FE8), src/lib.rs:1-66 (#1E11), src/models.rs:1-47 (#EE28). These observed read snapshot tags match the earlier reads; no fresh content hash was computed. Directory listings still contain exactly those five files.

Result: **zero instances of native, including case and compound forms**. See occurrences.tsv for the explicit zero-occurrence row and coverage.tsv rows 2-6 for full-file coverage. No context/intent/wording occurrence exists for a supplemental JEV judgment. The three earlier actual JEV source-semantic judgments remain recorded; they are not misrepresented as per-occurrence judgments or proof of the absence count. The zero finding comes from the complete sequential review, as expressly allowed by the clarified instructions.

No native-specific wording or behavior change is needed, no overlap remains unresolved, and no new JEV call was needed or fabricated during reconciliation. Generic stale documentation remains outside this native occurrence task.
