# Declarative JEV judgments

Task: `tched-line-audit-20261024/op-dbus-model`. Date: 2026-10-24. Agent: Codex crate worker.

## Occurrence ledger

Zero literal `tched` substrings observed by manual sequential reading, including inside longer tokens. No substantive occurrence or ambiguous naming cluster exists in this crate. No invented occurrence judgment. Leave/replacement counts are all zero.

## Successful installed judge call

Receipt: commentary `functions.eval`, title `Judging audit evidence boundaries`; Python `await judge(state, questions)` followed by `print(audit_judgment)`. Actual successful output below. No model override, routing extension, filesystem execution, source extraction, filtering, search, subprocess, network client or recursive delegation used.

State: reader-reported complete coverage of Cargo.toml:1-15, compare-op-dbus-model.md:1-63, SPEC.md:1-199, src/lib.rs:1-66 and src/models.rs:1-47, each through EOF; root/src listings match. Total 390 lines. The state explicitly says absence is the reader observation, not something selected excerpts independently certify. Naming-only authority; no build/test/deployment claim; supplied hashes not recalculated.

Exact contextual source supplied:

`crates/op-dbus-model/SPEC.md:19-24`:
```markdown
## Architecture

### Database Layer
- **Backend**: SQLite via sqlx
- **Runtime**: Tokio async runtime
- **Schema Management**: Automated table creation and migration
```

`crates/op-dbus-model/src/lib.rs:16-25`:
```rust
#[derive(Clone, Default)]
pub struct PluginCatalog {
    documents: Arc<RwLock<HashMap<String, PluginCatalogDocument>>>,
}

impl PluginCatalog {
    pub fn new() -> Self {
        debug!("Creating in-memory PluginCatalog");
        Self::default()
    }
```

`crates/op-dbus-model/Cargo.toml:6-15`:
```toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }
simd-json = { workspace = true }
serde_json = { workspace = true }
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1.6", features = ["v4", "serde"] }
thiserror = "1.0"
anyhow = "1.0"
tracing = { workspace = true }
op-state-store = { path = "../op-state-store" }
```

### Q1: ledger

Question: Given the complete reader-reported sequential coverage ledger and zero occurrence observation, which reporting decision is justified? Do not independently certify substring absence from selected excerpts.

Choice criteria:
- `complete_static_zero_occurrence`: all five ledger entries reach EOF and sum to 390 lines; report reader-observed zero occurrences and no naming patch, static audit only.
- `partial_missing_coverage`: one or more listed files/ranges remain unread or line count is inconsistent.
- `runtime_verified`: evidence proves build/test/deployment success.
- `insufficient_evidence`: ledger cannot support even qualified static completion.

Answer: `complete_static_zero_occurrence`; confidence 0.96. Probabilities: complete_static_zero_occurrence 0.97; partial_missing_coverage 0.03; runtime_verified 0; insufficient_evidence 0.

Decision: complete static naming audit based on full reader coverage, not runtime proof or independent JEV substring certification. Manual arithmetic: 15 + 63 + 199 + 66 + 47 = 390.

### Q2: documentation_disposition

Question: Evaluate the exact SPEC/lib/Cargo evidence against naming-only authority. Does this establish a naming correction, unrelated documentation drift to report only, or no discrepancy?

Choice criteria:
- `report_only_generic_drift`: SPEC describes SQLite/sqlx but provided implementation is in-memory and manifest lacks sqlx. Non-naming documentation drift; no edit under naming-only scope.
- `naming_rename_required`: exact evidence contains a tched naming defect requiring threetched/3tched substitution.
- `no_discrepancy`: SPEC and implementation clearly agree.
- `insufficient_evidence`: cannot classify the shown discrepancy.

Answer: `report_only_generic_drift`; confidence 0.99. Probabilities: report_only_generic_drift 0.99; naming_rename_required 0; no_discrepancy 0.01; insufficient_evidence 0.

Decision: report generic documentation drift without editing it. No naming defect, catalog migration or source change follows.

## Raw returned answers

```json
{"ledger":{"type":"choice","choice":"complete_static_zero_occurrence","confidence":0.96,"probabilities":{"complete_static_zero_occurrence":0.97,"partial_missing_coverage":0.03,"runtime_verified":0,"insufficient_evidence":0}},"documentation_disposition":{"type":"choice","choice":"report_only_generic_drift","confidence":0.99,"probabilities":{"report_only_generic_drift":0.99,"naming_rename_required":0,"no_discrepancy":0.01,"insufficient_evidence":0}}}
```
