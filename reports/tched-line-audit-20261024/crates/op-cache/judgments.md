# Declarative JEV receipts — op-cache

Task `tched-line-audit-20261024/op-cache`, Codex crate worker, 2026-10-24.

## Occurrence review

No `tched` substring observed during complete sequential reads of all 28 inventory files, including longer tokens, comments, examples, embedded tests, documentation and protocol definitions. There are no source occurrences to submit for leave/threetched/3tched contextual judgment. No occurrence or rename candidate was fabricated. All occurrence outcome counts are zero.

## Successful JEV call: audit evidence boundaries

Tool: `functions.eval`, Python, title **Judging audit evidence boundaries**. Supported installed `await judge(state, questions)` facility from `xd://eval/judge`; no routing extension, model selection, source extraction, arbitrary filesystem execution or tool enabling.

State supplied: task ID; evidence type explicitly limited to full sequential read receipts and the per-file ledger; the exact 28 path/range pairs reproduced in `coverage.tsv`; observations of complete root/src/grpc/examples/proto listing agreement, zero agent-observed substrings, zero source changes, zero build/test/runtime/deployment actions, and supplied—not recomputed—initial hashes. State explicitly says there is no occurrence to invent. Paths are relative to `crates/op-cache/`.

Question `coverage` (choice):

> Do the listed sequential ranges fully cover all listed files through their declared ends, or leave a gap? Evaluate the ledger only; do not certify unseen source absence.

Criteria:
- `complete_static_coverage`: Every listed file has contiguous ranges from 1 through its declared end.
- `incomplete_coverage`: A file range is missing or noncontiguous.
- `insufficient_evidence`: The supplied ledger cannot resolve coverage.

Actual answer:
```json
{"type":"choice","choice":"complete_static_coverage","confidence":0.72,"probabilities":{"complete_static_coverage":0.81,"incomplete_coverage":0.18,"insufficient_evidence":0.01}}
```

Decision: ledger is contiguous through the supplied end of each file. Independently checked the 28 endpoints against the inventory and sequential read receipts; their supplied newline counts sum to 13,168. JEV does not certify source text it was not given.

Question `claim_boundary` (choice):

> Given these observations, what completion claim is justified? Do not invent occurrences or treat a source audit as runtime proof.

Criteria:
- `static_audit_only`: Report full sequential source review and agent-observed zero occurrences, with no source changes and no build/test/deployment claim; JEV judged the ledger, not unseen source.
- `runtime_verified`: Evidence establishes builds/tests/runtime deployment success.
- `must_invent_occurrence`: A fake tched source occurrence must be submitted to complete a zero-occurrence audit.
- `insufficient_evidence`: Cannot identify an honest reporting boundary.

Actual answer:
```json
{"type":"choice","choice":"static_audit_only","confidence":1,"probabilities":{"static_audit_only":1,"runtime_verified":0,"must_invent_occurrence":0,"insufficient_evidence":0}}
```

Decision: report this as a completed static naming audit only. This successful call is an evidence-boundary judgment, not a claim that JEV performed contextual naming analysis on nonexistent occurrences. No judge failures or fallback calls occurred.
