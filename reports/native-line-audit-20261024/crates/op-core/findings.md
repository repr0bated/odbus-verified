# op-core native contextual audit findings

Task `native-line-audit-20261024/op-core`; 2026-10-24; Codex crate worker.

## Outcome

Full static review complete: **22/22 inventory files**, **5,187 supplied newline-counted lines**, all read sequentially through EOF, including docs and all tests. Root/src/tests directory listings match the inventory; no additional nested guidance, binary, missing, symlink or dependency entries were exposed. Every supplied entry is individually recorded in [coverage.tsv](coverage.tsv). Initial hashes are supplied pre-audit receipts; they were not recomputed or claimed current.

**Every-occurrence review complete; no native-specific needed change found. No crate source edit.** Earlier routing-only occurrence accounting is superseded by the clarified context/intent/wording review below. No needed correction overlaps tched/threetched or another crate. Existing fixture names were left intact for the coordinator workstream.

## Native occurrences and contextual dispositions

The conservative lexical ledger has **four occurrences**: two standalone/compound storage uses and two embedded native substrings in ordinary-English Alternative/alternative. Each occurrence has a separately addressed successful JEV context-and-intent judgment, not merely a routing classification. [occurrences.tsv](occurrences.tsv) records exact tokens, line/position, meanings, references, outcomes/confidence and unchanged/complete dispositions.

| Source | Context and intent | Wording/behavior decision |
| --- | --- | --- |
| `src/config.rs:56` | `Alternative paths to check (in order of priority)`; embedded native in Alternative; ordered environment-file fallback array following explicit custom-path handling. | Preserve accurate ordinary-English wording and array-order claim. OC1 confidence **0.97**. |
| `src/identity_projection_transaction_tests.rs:72` | `bail!("native commit outcome uncertain")`; synthetic error from the durable callback, exercising pending-state denial and exact-operation recovery. | Preserve accurate simulated storage-failure intent; no claim that this fixture executes a real database commit. OC2 confidence **0.92**. |
| `src/private_document_publication.rs:173` | `clear-marker/old-state rollback alternative`; embedded native in alternative; documentation scoped to exact-operation durable recovery. | Preserve: recovery Abort retains Pending; old SHM is not passed as callback authority. Pre-effect commit Abort is a separate API path. OC3 confidence **0.98**. |
| `src/projection_transactions_tests.rs:8` | `native-write failure`; single-shot test-only file/manifest publication failure, consumed before rollback; separate panic mode covers interruption. | Preserve accurate storage-boundary fault-simulation description; no actual syscall-interception claim. OC4 confidence **0.98**. |

Actual supplemental question/evidence/criteria/answer receipts: [judgments-occurrences.md](judgments-occurrences.md), also linked from [judgments.md](judgments.md). Earlier `native_usage` remains historical routing evidence, not sufficient by itself under the clarified scope.

Coverage basis: retained full sequential reads of all 22 inventory files, including documentation/tests. Complete sequential rereads of these four occurrence-bearing files confirmed current contexts; no new full-crate reread is claimed. Current `private_document_publication.rs:259` has an unrelated comment wording change since the prior read; preserved untouched. No coordinator-followup.md appeared in the assignment directory listing. No unresolved occurrence or native-specific change remains.

## Other selection/admission surfaces evaluated

- `src/config.rs:53-143,168-191`: generic environment-file priority, explicit `OP_ENV_FILE`, caller-supplied defaults. Existing valid Unicode environment entries skip assignment. No native provider/model/profile names or native-specific preference. Preserve; JEV confidence **1**. This does not assert universal safety for malformed/non-Unicode environment values or prove external callers select correctly.
- `src/types.rs:182-194`: `AgentDefinition.model: Option<String>` and generic config map are stored data, not a selection engine or routing gate. Preserve; JEV confidence **1**.
- `src/projection_shm.rs:37-61,170-195,234-252`: canonical raw-state and separate credential storage locations, plus a configurable test root. These are storage boundaries, not duplicate model schemas or hard-coded native profiles. Retired projection path constant is documented non-fallback; strict snapshots do not consult it.
- `src/identity_projection_transaction.rs:82-199,223-287` and `src/projection_transactions.rs:80-98,737-754`: uncertain durable credentials remain gated; exact operation token and authoritative durable callback required for recovery. No native model prerequisite. Preserve; JEV confidence **1**. Filesystem confinement does not establish upstream capability/identity admission.
- `src/private_projection_directory.rs:1-6,117-155`: explicitly separates trusted caller authorization/sealed contract pinning from filesystem confinement. `gemma_brain` in publication fixtures is a retained model-agnostic plugin ID, not a Gemma model selection instruction.
- Remaining complete-file coverage: manifest/dependency declarations, error/status/publication types, execution tracking, repository metadata, service-path helpers, security zone helpers and source regression guards contain no native model-routing override. This conclusion rests on full-file reads, not keyword search or JEV-driven file selection.

## Out-of-scope observations for coordinator reconciliation

### Generic UTF-8 summary truncation panic — source-only

`src/execution.rs:88-96,119-126`: `ExecutionRecord::complete` truncates output at 1,000 **bytes** through `&s[..max_len]`. For 999 ASCII `a` bytes followed by `é` (1,001 bytes total), byte offset 1,000 lies inside the final UTF-8 character and the slice panics. This can interrupt completion bookkeeping for non-ASCII output. JEV classified `generic_panic`, confidence **0.95**.

No edit: generic defect, not an independent native-specific override. Concrete proposed remedy for separately authorized work: choose a valid UTF-8 boundary at or below the byte budget before slicing. Verification would exercise this boundary and an ASCII truncation case; neither was executed here.

### Historical documentation/service-path disagreement — unresolved runtime applicability

`SPEC.md:19-45,61-84` and `compare-op-core.md:23-59,76-82` describe deleted `connection.rs`/`message.rs`, a stale source count and an `op-execution-tracker` dependency absent from the current manifest. They are historical inventory, not current authority.

`src/runit.rs:8-24,38-40` declares `/etc/sv`; root `AGENTS.md` declares `/etc/runit/sv`, which also appears in `tests/no_s6_regression.rs:27,128-131`. Source disagreement is observable; whether a live compatibility symlink makes this harmless is **unverified**. No runtime read or source fix, because service-layout reconciliation is outside the native-only assignment.

## Verification, scope and deployment limits

- Review evidence: complete sequential file reads, directory/inventory parity, five earlier installed declarative JEV source judgments, and four supplemental per-occurrence context/intent/accuracy judgments. No substantive JEV answer fell below 0.6 confidence.
- No searches, recursive delegation, arbitrary eval execution, Cargo/build/test/lint/format/custom smoke, runtime operation, seal, deploy, service restart, identity/credential/grant/config mutation, commit/staging/reset/stash.
- No application behavior changed; no before-copy needed. Tests were read, **not run**. No built/installed/deployed/tested claim.
- No Git commands; current HEAD was not independently queried. Preserve all existing dirty work. Parent coordinator handles reconciliation and any required builds.
- Earlier session shared HANDOFF bookkeeping remains historical; this occurrence reconciliation did not write shared HANDOFF/SIGNALS. Coordinator owns aggregate status. Findings/coverage/judgment reports remain exclusively under this crate's assigned report directory.

## Handoff / next action

Coordinator: reconcile this completed every-occurrence no-source-change audit, its four-row occurrence ledger and actual JEV receipts. No native patch needs building. Generic defects remain outside this assignment; no generic repair or runtime verification was attempted.

## Self-evaluation

Accuracy **4/5**: exact source/line evidence and actual JEV receipts; external runtime/caller behavior intentionally unverified. Completeness **4/5**: all 22 files covered and every native occurrence dispositioned; no runtime safety guarantee beyond the assignment. Clarity **4/5**: source findings and generic observations separated; receipt document is necessarily longer. Actionability **4/5**: concrete UTF-8 counterexample/remedy and coordinator next action; its repair needs separate scope. Conciseness **4/5**: per-file ledger and exact judgment reproduction add length but preserve auditability. Overall **4.0/5**. Improvements: coordinator should independently reconcile source snapshot currency before relying on this static receipt, and separately authorize the generic boundary repair if desired. User agreement check: this reports only static audit completeness, not end-to-end acceptance.
