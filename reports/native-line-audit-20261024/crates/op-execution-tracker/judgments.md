# Declarative JEV receipts

Task: `native-line-audit-20261024/op-execution-tracker`; Codex crate worker; 2026-10-24.

Actual successful facility: `functions.eval` Python, `await judge(audit_state, audit_questions)`; title `Judging contextual tracker evidence`. Instructions read from `xd://eval/judge`. One state, three independent choice questions in one call. No routing/model selection, searches, filesystem processing, shell or arbitrary execution. The tool did not expose a separate request ID or model/version; no model/version claim is made.

## State/source evidence

All nine inventory files fully reviewed sequentially (1,200 supplied newline-counted lines), including both embedded test modules. No `native` occurrence observed. The state supplied exact source excerpts from:

- `src/record.rs:98-134,255-294,316-340`: ExecutionRecord fields, builder defaults/setters/build. Decisive lines: `policy_id: "default".to_string()`, `initiated_by: None`, `self.policy_id = policy_id.to_string()`, `policy_id: self.policy_id`, `plugin_core_hash: self.plugin_core_hash`, `tunable_hash: self.tunable_hash`, `initiated_by: self.initiated_by`. The field block in the state omitted selected doc-comments; executable lines retained.
- `src/execution_tracker.rs:79-106,109-124,147-162,189-217`: start takes `tool_name`, `input`, `initiated_by`; `let mut record = ExecutionRecord::new(tool_name, None); record.input = input.unwrap_or(Value::null()); record.initiated_by = initiated_by; record.start();`; stores a clone, then `if records.len() > self.max_history { records.remove(0); }`, broadcasts Started, logs and returns the record. Both completion/failure methods locate only `records.iter_mut().find(|r| r.id == id)` before terminal transition/counting. Excerpts explicitly marked omitted remaining duration/count/event code as read in full, not copied into the judgment state. `get_execution`, `get_active`, `get_recent` bodies supplied with formatting compressed.
- `src/execution_context.rs:80-121`: complete new/new_child/update_status/set_metadata bodies; tool names copied, child inherits trace and parent IDs, metadata assigned verbatim. No provider or model selection.
- `src/metrics.rs:85-104`: complete counter/gauge update functions; tool/status labels ignored to avoid cardinality.
- `src/telemetry.rs:64-69`: `record_event` logs provided event/details. No dispatch.

Full-file read coverage and initial source hashes are in `coverage.tsv`. The tool transcript retains the submitted evidence object, questions and returned answers.

## Q1 — selection

Question: “Do these tracking constructors/defaults/setters implement an implicit provider/model/config override? Distinguish metadata labels from actual runtime selection. Select insufficient if evidence cannot decide.”

Contrary outcomes:
- `metadata_only`: supplied default policy_id and copied hashes/tool names are record metadata, not provider/model/config selection.
- `runtime_override`: a shown branch substitutes an implicit provider/model/config authority.
- `insufficient`: local evidence cannot distinguish those outcomes.

Answer: `metadata_only`; confidence **1**; probabilities `{metadata_only: 1, runtime_override: 0, insufficient: 0}`.

Decision: preserve metadata/default policy label. No native-specific configuration override established; no patch.

## Q2 — admission

Question: “What admission conclusion does the shown local tracker code support? Do not infer external caller behavior or a deployed boundary bypass.”

Contrary outcomes:
- `tracking_not_admission`: records caller-supplied initiated_by/tool metadata; neither authenticates that identity nor dispatches tools; caller admission unverified.
- `verified_admission`: verifies identities/capabilities before executing tools.
- `proven_bypass`: executes tools while bypassing an established admission requirement.
- `insufficient`: cannot establish even the local tracking-only role.

Answer: `tracking_not_admission`; confidence **1**; probabilities `{tracking_not_admission: 1, verified_admission: 0, proven_bypass: 0, insufficient: 0}`.

Decision: initiated_by and policy fields are not authentication evidence. No local execution boundary bypass established. External caller admission/deployment not certified.

## Q3 — bounded history

Question: “Evaluate max_history=1: start A, start B before A completes, then complete A. Is losing A and ignoring its completion supported by exact local source, and is that native-specific?”

Contrary outcomes:
- `generic_eviction`: B evicts running A; completion cannot find A, so no count/completion notification; generic bounded-history behavior, not native/provider routing.
- `native_override`: sequence establishes native-specific provider/model override.
- `no_loss`: active A retained separately or completion counted despite eviction.
- `insufficient`: cannot determine sequence.

Answer: `generic_eviction`; confidence **0.97**; probabilities `{generic_eviction: 0.98, native_override: 0, no_loss: 0.01, insufficient: 0.01}`.

Decision: record source-only generic risk G1 outside independent native-specific edit scope. No runtime reproduction or proposed native patch.

## Occurrence-reconciliation addendum

Per coordinator request, all nine inventory files were reread sequentially through EOF after the updated per-crate instructions. Exact case-insensitive occurrence ledger is `occurrences.tsv`: **zero** occurrences of token `native` or native-compound term anywhere in assigned crate (source, docs, manifest, comments, inline tests). As there are no actual native occurrences, no new per-occurrence contextual JEV call is applicable. The three existing source-context judgments above are preserved as supplemental contextual findings, not misrepresented as per-occurrence assessments. No occurrence or wording change is needed.

Coordinator instruction `/var/tmp/native-line-audit-20261024.ILroaQ/reconcile-occurrences.md` read in full. `coordinator-followup.md` read attempted and absent. No shared handoff/signal edits per updated instruction.
