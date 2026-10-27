# op-chat contextual tched audit

Task `tched-line-audit-20261024/op-chat`; 2026-10-24; Codex crate worker.

## Outcome

All54 supplied inventory entries read sequentially through EOF, including generated protocol code, docs, bundled skills, tests and the empty marker. Supplied newline count29,595. Directory listings reconcile the54 entries. No missing/binary/symlink/dependency-only inventory entry prevented reading. Markdown entries with Python/JavaScript MIME remain text and were read fully. No nested agent instruction file appeared in the traversed directory listings.

19 literal `tched` substrings found and dispositioned: **19 leave_as_is; 0 replace_with_threetched; 0 replace_with_3tched**. No crate source edits or proposed cross-crate hunks. No native-audit overlap.

## Every occurrence

Paths below relative to `crates/op-chat/`. Each row represents one literal substring occurrence.

| Path:line | Token | Disposition | Reason |
| --- | --- | --- | --- |
| proto/chat.proto:7 | threetched | leave_as_is | Already-qualified external LLM routing owner |
| proto/chat.proto:8 | threetched's | leave_as_is | Same external routing owner's state |
| proto/chat.proto:45 | threetched-selected | leave_as_is | External provider selection comment |
| proto/chat.proto:46 | threetched-selected | leave_as_is | External model selection comment |
| src/agent_tools.rs:183 | matched | leave_as_is | Ordinary keyword-matched agent-set variable |
| src/agent_tools.rs:184 | matched | leave_as_is | Same set's emptiness check |
| src/agent_tools.rs:192 | matched | leave_as_is | Same set's descriptor membership check |
| src/chat_service.rs:6 | threetched | leave_as_is | Already-qualified external provider/model routing owner |
| src/memory_loop.rs:204 | matched | leave_as_is | Ordinary semantic retrieval vocabulary |
| src/memory_loop.rs:227 | unmatched | leave_as_is | Ordinary durable-entry fallback vocabulary |
| src/orchestration/grpc_pool.rs:8 | Batched | leave_as_is | Ordinary parallel execution vocabulary |
| src/orchestration/proto/op_chat.chat.rs:13 | threetched-selected | leave_as_is | Correct generated provider comment from chat.proto |
| src/orchestration/proto/op_chat.chat.rs:16 | threetched-selected | leave_as_is | Correct generated model comment from chat.proto |
| src/orchestration/skills_builtin/python_testing_patterns.md:50 | test_patched | leave_as_is | Python mock patch example, not branding |
| src/system_prompt.rs:31 | 3tched | leave_as_is | Qualified infrastructure platform branding |
| src/system_prompt.rs:35 | mail-3tched | leave_as_is | Existing Incus object identifier; no migration authorized |
| src/system_prompt.rs:249 | /run/3tched-lungs.socket | leave_as_is | Qualified local lungs socket path; no migration authorized |
| src/system_prompt.rs:256 | mail-3tched | leave_as_is | Same Incus object in topology table |
| tests/e2e_memory_loop_voyage_qdrant.rs:137 | unmatched | leave_as_is | Ordinary semantic-memory regression diagnostic |

Seven external-qualified names, four qualified infrastructure/object/path names, eight ordinary English substrings. All other inventory entries contain no literal occurrence as observed during full sequential reads.

## Evidence and uncertainty

`coverage.tsv` records every path, supplied initial SHA256 and actual complete sequential ranges. Hashes are inventory receipts, not newly recomputed digests. No Git command or HEAD query performed.

`judgments.md` records two successful installed declarative judge calls covering nine contextual choices, plus three supplemental jev_verify calls. The documented judge_batch symbol was unavailable (`ReferenceError`); reported to tool QA. Low-confidence and insufficient results are disclosed, not converted into support. Direct full-source inspection resolves ordinary matching vocabulary and retained platform/object/socket names; preserving already-qualified naming avoids inventing migrations.

## Boundaries

Static naming audit only. Existing generic stubs and historical topology/design debt were not repaired. No claim that the chat pipeline, topology or deployment works. No searches, shell/process/network commands, source filtering, Cargo, build, tests, lint, formatting, custom smoke fixture, runtime operations, deployment, reseal, credential/grant changes or recursive delegation.

Only audit reports/result and required shared HANDOFF bookkeeping were written. No application source changed; no build/test/deploy result is claimed. No BEFORE source copy required because no source edit was proposed or applied. Coordinator owns reconciliation.

## Self-evaluation

| Axis | Score | Evidence / limit |
| --- | --- | --- |
| Accuracy | 4/5 | All19 occurrences have source lines and dispositions; initial hashes were not independently recomputed |
| Completeness | 4/5 | All54 inventory entries and29,595 supplied lines read; static manual coverage, not a machine-generated search count |
| Clarity | 4/5 | Per-occurrence table separates external, infrastructure and lexical contexts; judgment receipt is necessarily longer |
| Actionability | 4/5 | No source patch needed; coordinator can reconcile coverage/result and disclosed weak judgments |
| Conciseness | 4/5 | Compact findings table; full required evidence retained separately in judgments.md |

Overall4.0/5. Improvement: coordinator should reconcile the supplied initial inventory against any subsequent concurrent source changes before whole-task closure. This is not a request to run unauthorized checks. User agreement check: complete static naming coverage, not runtime certification.

Next action: coordinator reconcile these receipts with the other crate reports. No op-chat naming patch needs building.
