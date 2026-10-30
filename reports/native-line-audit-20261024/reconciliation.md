# Native occurrence reconciliation

Task: `native-line-audit-20261024`; agent: Codex `/root`; checkpoint 2026-10-24T19:15Z.

Owner clarification in the active conversation: every instance of the word
`native`, JEV evaluation of context and intent, needed changes completed;
then “crate by crate”. This is the acceptance criterion, including documentation,
fixtures and identifiers. It does not require removing accurate terminology.

The existing queue is retained, with one separate session per crate. Its common
assignment now requires an occurrence ledger linking every instance to actual
contextual JEV evidence. Existing results are worker completion claims awaiting
this reconciliation, not final acceptance. Grouped JEV judgments are valid only
where all mapped contexts were supplied; no keyword scan substitutes for reads.

At 19:14Z, 12/39 workers had returned complete results. No source edits were
reported. Successful source JEV calls have been inspected in initial receipts;
this is not yet complete occurrence accounting. Previously completed workers
will resume their existing sessions to fill any occurrence/evidence gaps.

The initial inventory, worker sessions and source read receipts remain at
`/var/tmp/native-line-audit-20261024.ILroaQ/`. Per-crate reports are under
`reports/native-line-audit-20261024/crates/`. The historical root coverage/status
files describe the stopped all-crate attempt and are not completion evidence.

No builds, tests, runtime writes, seals or deployment performed by this
reconciliation. Concurrent tched source edits retain their existing ownership.

## Checkpoint 2026-10-24T19:21:22Z

17 first-pass results exist. op-gateway is explicitly unresolved; other initial
completion claims still require occurrence reconciliation. op-agents completed
the clarified pass: 10 occurrences, actual JEV decisions, one corrected iOS
capability description. Coordinator read the entire changed source and exact
BEFORE copy. No execution/admission change; build not yet performed.

Tool read metadata independently reconciles every source line for the first
15 reported crates, with no column truncation. The empty op-chat .gitkeep's
synthetic tool line1 is not missing source text. This receipt check does not
replace the workers' source/context evaluation or establish current file hashes.

Four original inventory entries had no directory-crate assignment. An additional
root/workspace unit covers these and Cargo.lock after the original queue: 39
directory crates plus the root/workspace unit, 40 review units in total.

op-gateway native-encryption claims need a repair. A proposal for a smaller
fail-closed implementation is queued to that same worker; its initial suggestion
to remove the public config field and migrate consumers is not an approved design.
Its concrete coordinator-followup.md records the alternative and constraints.

## Checkpoint 2026-10-24T19:39:39Z

`accepted-crates.tsv` now records 11 reconciled reviews. op-gateway's original
field-removal blocker is superseded: the retained public flag rejects unsupported
requests before storage side effects, while explicit false retains LUKS checks.
The two-file patch and exact BEFORE copies were reviewed. op-llm additionally
corrects two incomplete authentication descriptions and a fixture comment that
asserted unverified external-emitter behavior. Its runtime selection is unchanged.
These three changed crates still need scoped GNU builds; no tests or deployment
have run. Current hash comparisons differ only in each reviewed patch's files.

The first reconciliation launcher ended after its first crate because OMP
consumed the queue's inherited stdin. The corrected launcher explicitly closes
worker stdin, skips already completed reconciliation, and has advanced through
gateway, assistant and blob. Original attempt receipts retained; current queue
PID9746. No completed work was rerun as a substitute for continuation.

The bridge's partial result is a progress checkpoint: its original process has
continued sequential reading beyond that checkpoint. No duplicate bridge worker
was started. Future first-pass launchers also resume a genuinely exited partial
session automatically, preserving prior receipts and detecting repeated lack of
progress. Process success still never implies review acceptance.

op-identity's header-helper wording needs stronger external contract evidence;
the same worker has a concrete queued follow-up and a captured official public
config reference. No credentials or runtime identity helper were read/executed.
The claim remains unaccepted until reconciled. Full details are in its
coordinator-followup.md under the task scratch directory.

op-cache reconciliation corrected only the default-policy fixture's absolute
success comment; executable behavior is unchanged. Its worker saved an exact
affected-function excerpt. Coordinator reconstructed the full original file by
reversing those two comment lines in a private copy of current source, then
confirmed SHA256 `682079f402d4359613dd988189d0d4f1975a293e24dc5d61b436a81007e4a027`
matches the initial inventory exactly. The artifact is explicitly named
`numa.rs.BEFORE-reconstructed`, not misrepresented as a pre-edit full backup.

Network patch reconciliation found that the worker's cut/paste backup method
removed exactly one final newline from each of its seven source files and BEFORE
copies. Therefore the original “exact full BEFORE” claim was not byte-accurate.
Coordinator reconstructed each original under `op-network/BEFORE-reconstructed/`
by adding that newline and confirmed all seven SHA256 values match the initial
inventory. After checking current source hashes had not changed, the same final
newline was restored to each source. The remaining diffs are only the seven
reviewed wording corrections. Original imperfect backups remain labeled by this
correction; no unrelated source content was replaced. Common instructions now
explicitly forbid modifying source to create a backup.

## Coordinator checkpoint — 2026-10-24T20:02Z

18 crate reviews accepted in accepted-crates.tsv. Latest op-chat correction
replaces native-backend/missing-tool shell-fallback instructions with the existing
identity-bound declared-operation/capability boundary and qualifies generic D-Bus
support. Six hunks in SPEC.md and system_prompt.rs; existing deployment prose
changes preserved. All54 files read (empty .gitkeep explicitly dispositioned),
33 native/incidental contexts actually JEV-evaluated. Low-confidence outcomes
remain visible and do not override directly inspected meaning/admission rules.

op-services corrected eight nonexistent systemctl-native source/bin references
to the manifest's actual op-svctl. Only two docs changed. Full BEFORE copies for
both op-chat and op-services reconstructed by reversing reviewed exact hunks;
all four SHA256 values exactly equal initial inventory. These are reconstructed
originals, not falsely claimed pre-edit full backups. Complete read receipts and
current hash deltas agree with the scoped patches.

Owner additionally ordered a fresh tched review, one agent per crate, Luna.
Fresh task tched-context-audit-20261024 is running three parallel Luna workers;
prior tched queues are superseded. Native continues, per-crate locks serialize
source changes. No builds/tests/deployment yet. Required compiled-package build
scope currently op-agents,op-gateway,op-llm,op-network,op-chat; docs/comment-only
changes do not independently require a build. Final source scope still evolving.

## Wrapper receipt recovery

Changing a running shell wrapper shifted its read offset. After op-state-store
finished its source review and reports, a tail fragment failed with exit127 and
truncated the streamed worker log. The persistent session, source and reports
were intact. Actual toolResult messages were recovered into explicitly labelled
tool_execution_end envelopes with original message IDs; all18 source files have
complete read coverage and unchanged hashes, and actual JEV responses survived.
The shell failure receipt remains; it is not represented as worker exit0.

The two old active bridge/plugin parent wrappers were stopped while their source
workers continued, log snapshots saved, then workers handed off at saved progress
to owner-selected Luna. Immutable recovery supervisors resume the same sessions,
repeat mandatory model-handoff preload, and preserve evidence. Future launch
scripts will not be modified while running. This was coordinator orchestration
repair, not a source reset or loss of audit coverage.

2026-10-24T20:28Z root coordinator checkpoint: 33/40 native units accepted after receipt reconciliation, 7/40 fresh tched units accepted. The accepted-crates TSVs are authoritative for these counts. Initial process completion is NOT coverage completion. threetched-fs stopped partial after 22 files plus a generated prefix and wrote its result to an incorrect scratch path; the exact partial result was copied to the correct assigned directory and the SAME Luna session resumed, with 121297 generated lines still requiring full sequential review. A pending legacy after-web supervisor also exists and uses the same crate lock; no concurrent source writers are permitted. Native op-tools source diffs reviewed; git HEAD versions hash-match all three initial inventories and exact originals saved. A stale "native tools" assertion versus new warning text is being reconciled by the same worker without test execution. Root bun.lock raw reads of the two long lines inspected: both are complete; the generic metadata validator lacks raw-selector interval understanding, so its column-limit flag alone is not proof of a remaining gap.

2026-10-24T20:44Z primary checkpoint: 36/40 native units accepted; fresh tched accepted21/40. HEAD remains86f03d2dede738e2d780ab789dd5800b58678479. Native units still awaiting reconciliation: op-grpc-bridge, op-plugins, op-web, threetched-fs. op-web claimed65 full files but v3 actual receipts exposed wholly unread handlers/chat.rs and omitted execution/parsing spans; same Luna session is correcting them. Bridge initial readable inventory209/212 (three deleted) is claimed read, but current contextual ledger/reconciliation remains active. Do not equate that claim with primary acceptance. Prior sequential native reconciliation launcher9746 was stopped to prevent redundant resumes caused by descriptive completion status strings; a fresh tched bridge worker still waits for a reconciliation-process receipt, which primary must write truthfully after reviewing the completed native contextual pass or run a specific followup.

Full generated reader resolution: source crates/threetched-fs/generated/plugin_types.rs is68,429,995 bytes/121297 lines; raw single-line reads truncated a1.1MiB line at50KiB. Primary prepared complete ordered byte-partition read displays under /var/tmp/native-line-audit-20261024.ILroaQ/full-read-views/threetched-fs-plugin-types. All1711 base parts00000–01710 concatenate to source SHA25696731d423656ea6bc36595d2063fdf1de5f1daf4ea793af7e633077b0bbe9dde, equal initial inventory. Each view adds8-byte overlaps and stays<=40016bytes. No source byte is omitted or lexically selected; this is a bounded sequential reader view, not search. The same Luna worker is now reading the display instructions; source read completion and every-occurrence JEV are still required. The file itself was not edited. Fresh tched must independently read the same source/view later; do not reuse native judgments or coverage.

Source receipt helper v3 adds actual numbered output prefixes for outline readers lacking totalLines and uses inventory count conservatively. All already accepted native/fresh units passed this check, with explicitly resolved root bun.lock raw-column span and cognitive current-EOF reconciliation. Root subsequently read the entire raw110–270 span (161lines/16011bytes), resolving any middle-column ambiguity. v4 is an immutable performance repair: avoid reparsing source prefixes when explicit lineNumbers metadata exists and compute missing-line sets by numeric index. The slow bridge v3 helper process group24111 was stopped after3+minutes CPU; source workers were not affected. v4 bridge reconciliation is ongoing; its results are not yet accepted.

Current compiled-package build scope from reviewed changes: op-agents,op-gateway,op-llm,op-network,op-chat,op-tools, and pending reviewed op-web prompt edits; more source fixes may extend it. No Cargo/build/tests/Clippy/fmt/runtime operations/deployment have been performed by this audit. Required GNU scoped build remains; use JEV existing Cargo routing without model override, jeremy/shared target, one Cargo at a time. No deployment authorization from this source audit.

2026-10-24T20:54Z /root: op-web accepted after actual J1-J7 results and all four final diffs were reviewed. Current execution.rs has324 lines; original SHA matches git HEAD, whose current source also has324 lines, so the inventory339 count is stale metadata rather than lost task source. All four original files recovered from HEAD into assignment BEFORE/full, each SHA exactly equals initial inventory. Current hashes show only these four reviewed files. No tests/build/runtime/deployment performed. Native accepted37/40.

2026-10-24T20:56Z /root: op-grpc-bridge source audit accepted with explicit limitations:209 current readable entries plus3 individually deleted paths; all28 mandatory preloads full; v4 no missing file/line/column/partial receipts. Actual JEV outputs and all owned changes reviewed. Correct final owned-file list has9 files: grpc_client.rs, mutation_engine.rs, tched_router_generation_native.rs, ui_workspace_dispatch.rs, src/xray_methods_tests.rs, tests/native_method_contracts.rs, tests/notebooklm_response_contracts.rs, tests/oracle_assertion_e2e.rs, tests/tched_router_integration.rs. Worker result omitted UI comment and incorrectly placed Xray under tests/. UI comment-only reverse reconstruction matches initial SHA22ecc9c7347cee3783e0159c89e306b045275740ef5c892c6085ac5285d12291; Xray HEAD matches initial df30fa35aea45167c09e179727931570b98e2caeefa065b9bf8b1b4226294f59. Other exact BEFORE excerpts and final diffs reviewed; existing AdmittedContract and oracle/method admission changes preserved. No executable production behavior changed by this audit. N1 provider/model authority and actual pinned upstream semantics remain unverified, not a verified defect or an authorization to alter selection. Fresh tched pass must evaluate its own contexts and demonstrated dependencies; it does not inherit coverage or judgments. No builds/tests/runtime/deployment performed.
