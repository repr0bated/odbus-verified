# op-assistant-grpc native contextual audit

Task: `native-line-audit-20261024`; date: 2026-10-24; reviewer: Codex.

## Outcome

**No native-specific defect found; no source edits.** All 31 inventory entries read completely in sequential ranges, including build inputs, nine protobuf files, all Rust source and embedded tests, and the integration test file. Per-file ranges and supplied pre-audit SHA256 receipts: [coverage.tsv](coverage.tsv). No literal `native` occurrence found by full-file reading. No search tools used.

Directory listings of the crate, `src`, `src/bin`, `proto`, `proto/assistant`, and `tests` matched the supplied inventory. No additional files, nested instructions, binary/nontext files, symlinks, deleted/missing entries, standalone lockfiles, or checked-in generated files were shown in this crate. Generated proto output is referenced through OUT_DIR by build.rs/lib.rs, not a checked-in inventory entry. Dependency crates were not recursively audited or edited.
## Every-occurrence reconciliation

The updated common assignment and `reconcile-occurrences.md` expand review beyond routing to every case/compound form, including comments, documentation, identifiers and fixtures. Reconciliation of the retained complete source reads finds **zero occurrences in the assigned crate**, not merely zero model-routing defects. [occurrences.tsv](occurrences.tsv) records the explicit zero-occurrence conclusion and points to the unchanged complete [coverage.tsv](coverage.tsv); its `#` lines are metadata, not occurrence rows.

No `coordinator-followup.md` appeared in the complete assignment-directory listing. Current crate/source/protocol directory listings showed no added source file. Original full-file reads remain the coverage evidence; no new source-read ranges or fresh hashes are claimed. No unreviewed occurrence context exists to supplement with a JEV call. Original successful source-context judgments remain recorded, but are not misrepresented as per-occurrence judgments or a machine search proving absence.

There is no native wording/claimed behavior in this crate requiring semantic correction, no unresolved native change and no overlap/blocker. Generic stale prose remains unrelated, as described below. Report text outside the assigned crate is not recursively included as a source occurrence.


## Contextual dispositions

1. **Explicit selection is not an implicit override.** `proto/assistant/model.proto:33-37`, `src/models.rs:49-63`: SwitchModel carries caller-supplied model_id and optional agent/session scope. `src/agents.rs:66-111` forwards requested create/update model values. No provider selection, native-specific gating, model substitution, profile materialization, or config write exists in these reviewed paths. JEV: explicit_forwarding, confidence 1.00.
2. **Standalone application access is intentionally fail-closed.** `src/auth.rs:23-50`, `src/server.rs:97-129`: all eight application services use an interceptor that never returns success. SID1 structural validity is insufficient; OIA1 is rejected. This is canonical-admission preservation, not a native-product routing gate. Reflection/health are separate; no claim of their authentication or of live exposure. JEV: fail_closed, confidence 1.00.
3. **Retained transport options do not establish fallback execution.** `src/transport.rs:113-121`: D-Bus failure propagates; Rpc mode returns disabled error. `src/dbus_service.rs:24-28` denies the generic passthrough. Neither is justification to add an alternate client around identity admission. JEV: no_http_fallback, confidence 1.00.
4. **Cognitive contract consumption is not a parallel native schema.** `src/cognitive_client.rs:64-90` reads the sealed cognitive source contract, checks the method declaration, and forwards its source hash with PluginV1.Call. Remaining code adapts memory/soul/namespace operations. No native route/schema authority introduced here; no change.

Substantive declarative JEV questions, contrary and insufficient-evidence choices, evidence references, full answers, probabilities and confidence are in [judgments.md](judgments.md). Final native repair disposition: no_repair_evidenced, confidence 0.92.

## Declined scope / limitations

- Stale authentication and HTTP-fallback prose in `src/lib.rs:8-10` conflicts with fail-closed implementation; topology prose in the standalone entry is also historical. These are not native-specific defects, so no unrelated cleanup.
- Existing incidental/default-value tests and general memory/pagination/conversion behavior were read but not rewritten as part of the native-only request.
- Other-crate backend implementations and external callers were not audited: this crate contains no native-specific occurrence requiring an external caller trace. Conclusions are bounded to this crate, not global selection correctness.
- Supplied hashes are pre-audit receipts, not recomputed current hashes. Source was not edited; no before-copy needed.
- Source-only review. No Cargo, build, tests, Clippy, formatting, custom smoke, runtime, deployment, seal, service, identity, credential or grant operation. No command or runtime success claimed. No behavioral change requiring runtime verification was made.
- No threetched/tched hunks touched; no overlapping native finding or proposed cross-crate hunk.

## Coordinator next action

Reconcile this zero-change, complete crate audit with sibling coverage. No crate-specific fix/build action generated by this audit.

## Self-evaluation

Accuracy 4/5: source/JEV evidence is explicit; no runtime verification, intentionally outside authority.
Completeness 4/5: every listed file and directory reviewed; external dependency behavior not claimed.
Clarity 4/5: per-file ledger and decision receipts separate evidence from limitations; mandatory preload history remains in transcript rather than duplicated in report.
Actionability 4/5: coordinator has complete coverage and no-change disposition; global reconciliation remains their task.
Conciseness 4/5: raw JEV answers repeat structured decisions for traceability.
Overall 4.0/5. Improvement applied: native-absence claim bound to full-file coverage; stale docs explicitly separated from executable behavior. User agreement is not independently established.
