# op-cache contextual naming findings

Task: `tched-line-audit-20261024/op-cache`. Auditor: Codex crate worker. Date: 2026-10-24.

## Outcome

Complete static review of every line of all 28 supplied inventory files: 13,168 supplied newline-counted lines. No `tched` substring observed, including longer tokens. No source patch required or applied.

| Occurrence disposition | Count |
| --- | ---: |
| leave_as_is | 0 |
| replace_with_threetched | 0 |
| replace_with_3tched | 0 |
| unresolved or deferred occurrence | 0 |

These are occurrence counts, not counts of files left unchanged. All 28 crate files were left unchanged by this worker.

## Coverage and reconciliation

- `coverage.tsv` lists every inventory path, exact sequential ranges read, supplied initial SHA-256, finding/action and final status.
- Complete crate root, src, src/grpc, examples and proto directory listings agree with the 28-file inventory. All text read successfully; no binary, missing, symlink, dependency-link or extra file encountered.
- Generated specification/comparison text, protocol definitions, the example and all embedded tests were read, not excluded or sampled.
- Initial hashes are inventory provenance only. No independent rehash or Git HEAD query performed. Source reads had no truncation gaps; raw ranged reads used for complete text where applicable.
- Concurrent native-audit reports and unrelated crate/source work were not modified. No cross-crate rename, sealed identifier migration or native-audit hunk overlaps proposed.

## JEV evidence

`judgments.md` records a successful installed declarative `judge()` call: ledger coverage choice `complete_static_coverage` (confidence 0.72); reporting boundary choice `static_audit_only` (confidence 1). No occurrences exist for contextual naming judgment. JEV did not independently inspect or certify absence across unseen source; absence is this worker's full-read observation.

## Execution limits and next action

No searches, grep/sed/awk, programmatic extraction, shell commands, Cargo/builds/tests/lint/format/smoke fixtures, runtime operations, deployments, seals, grants or credentials changed. No recursive delegation or commits. No built/tested/installed/deployed claim.

Coordinator: reconcile this completed coverage/judgment/result receipt with the per-crate queue. No op-cache naming patch from this worker requires building. Generic cache behavior and historical documentation quality are not certified by this naming-only audit.

## Self-evaluation

Static deliverable only; overall 4.2/5. Accuracy 4: absence relies on full human-style reads, not independent machine search (prohibited). Completeness 5: every inventory file/range and zero-occurrence disposition recorded. Clarity 4: ledger and judgment report separate to keep evidence inspectable. Actionability 4: coordinator reconciliation remains external to this crate worker. Conciseness 4: repeated execution limits make the static/runtime boundary explicit in independent receipts. No axis below 4; no substantive deliverable gap identified. User-agreement self-check: expected yes on full-read scope and explicit absence of build/runtime claims.
