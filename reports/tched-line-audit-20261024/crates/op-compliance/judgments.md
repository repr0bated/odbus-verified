# op-compliance declarative JEV receipts

Task: `tched-line-audit-20261024/op-compliance`; date: 2026-10-24; agent: Codex / openai-codex/gpt-6.1-sol.

## Source review

No `tched` occurrence observed in either complete file, including substrings inside longer tokens. No occurrence or context/intent naming question was fabricated. This absence is the source reader's observation, not a JEV certification based on excerpts.

## Successful installed judge call

Facility: `functions.eval`, Python prelude `await judge(audit_evidence, questions)`; transcript label **Judging static audit evidence**. Actual syntax read from `xd://eval/judge`. No model selection/routing, completion model, arbitrary execution, extraction, traversal, filtering, shell or network code. One state, two independent typed questions in one call. Success output printed directly and retained below.

### Evidence state

- Task: `tched-line-audit-20261024/op-compliance`.
- Inventory Cargo.toml: supplied12 lines; observed EOF12; read1-12 with request1-40; snapshotD481; reviewer observation no `tched` substring including longer tokens.
- Inventory src/lib.rs: supplied324 lines; observed EOF324; read1-180 then181-324 with final request181-350; snapshotD710; same reviewer observation.
- Root directory receipt: Cargo.toml, src/. src/ receipt: lib.rs. No other entries shown.
- Exact source supplied to JEV: complete Cargo.toml:1-12 and src/lib.rs:91-108, reproduced below. All other source lines independently read by this worker, not supplied to JEV.
- Execution: static read only; no source edits, searches, commands, Cargo, tests, lint, format, smoke fixtures, runtime actions or deployment.
- Explicit criteria: our filesystem branding `3tched`, external executable `threetched`, preserve contracts; no observed occurrence means do not invent one; judge ledger sufficiency/reporting limits, not certify source absence from excerpts.

```toml
[package]
name = "op-compliance"
version = "0.1.0"
edition.workspace = true

[dependencies]
anyhow = "1"
thiserror = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
op-state-store = { path = "../op-state-store" }
```

```rust
pub struct LawFirm;

impl LawFirm {
    /// Review the canonical typed plugin schema. The generated JSON schema is
    /// only a view for the attorney checks; no repository schema file is read.
    pub fn review_plugin_schema(
        schema: &op_state_store::PluginSchema,
    ) -> std::result::Result<(), ComplianceError> {
        let mut v = serde_json::to_value(schema)?;
        v["plugin_type"] = serde_json::Value::String(schema.category.clone());
        v["schema"] = serde_json::to_value(schema.to_json_schema())?;
        Self::validate_structure(&v)?;
        attorneys::OliviaScal::validate_controls(&v)?;
        attorneys::EugeneRisk::validate_ai_risk(&v)?;
        attorneys::PennyPrivacy::validate_privacy(&v)?;
        attorneys::ReggieOpa::validate_policy(&v)?;
        Ok(())
    }
```

### Questions and criteria

**coverage** (choice): Does the recorded sequential coverage reach every supplied inventory line with root/src directory parity? Evaluate ledger arithmetic and continuity only, not whether the reviewer's absence observation is true.

- `complete_ledger`: Every supplied file covered contiguously from1 through observed EOF; listings agree.
- `incomplete_ledger`: Missing inventory file, range gap, or absent EOF.
- `insufficient_evidence`: Cannot determine ledger coverage.

**reporting** (choice): Given no observed naming occurrence and read-only execution, which outcome is justified? Do not fabricate a naming occurrence or runtime claim.

- `static_no_edit_report`: Record reviewer-observed zero occurrences, zero edits, complete static coverage with explicit no runtime/build/test verification.
- `rename_source`: A contextual tched naming correction is demonstrated by supplied source.
- `runtime_verified`: Build/tests/runtime behavior verified.
- `insufficient_evidence`: Cannot determine reporting boundary.

### Actual answers

```json
{
  "coverage": {
    "type": "choice",
    "choice": "complete_ledger",
    "confidence": 0.98,
    "probabilities": {"complete_ledger": 0.99, "incomplete_ledger": 0.01, "insufficient_evidence": 0}
  },
  "reporting": {
    "type": "choice",
    "choice": "static_no_edit_report",
    "confidence": 0.95,
    "probabilities": {"static_no_edit_report": 0.96, "rename_source": 0, "runtime_verified": 0, "insufficient_evidence": 0.04}
  }
}
```

Decision: complete static naming review, no source edit. Source absence grounded in sequential full-file reads; JEV supports ledger continuity and conservative reporting only. No live deployment, caller correctness, legal compliance or build/test claim.
