# op-chat — native contextual audit

Task: `native-line-audit-20261024/op-chat`; auditor: Codex crate worker; date: 2026-10-24.

## Outcome

Complete **static occurrence reconciliation** of all54 inventory entries. `occurrences.tsv` accounts for18 native/Native/natively uses and15 supplemental literal substrings inside alternative(s)/alternative_thought, each individually presented to installed declarative JEV. Six native-related wording changes applied in two crate files. No unresolved native-specific source change or tched overlap.

This supersedes the earlier “no native defect/no source changes” conclusion: the original model-routing-focused judgments did not establish wording or admission correctness for every occurrence. Original complete-file reads remain evidence; coverage was not discarded or invented.

`coverage.tsv` is the per-file receipt: complete sequential ranges, supplied initial SHA-256, disposition, status. Directory listings of crate root, src, bin, proto, orchestration, generated proto, services, bundled skills and tests agree with the inventory. No binary/missing/symlink/dependency entry or nested guidance was encountered; `.gitkeep` was read and explicitly dispositioned as empty. Initial hashes are pre-audit evidence only; not recalculated/current-source attestations.

## Contextual dispositions

1. **Keep structured-call `Native`.** `src/nl_admin.rs:25-44,109-140` labels provider API tool-call representation, preferred over extracting calls from prose. It does not select a provider, model or configuration. J1: `representation_only`, confidence1.
2. **Keep caller-selected provider/model.** `src/forced_tool_pipeline.rs:115-190` and `src/nl_admin.rs:409-449` invoke their passed provider with their passed model. J1: `caller_preserved`, confidence0.98. `src/actor.rs:29-53,557-565,636-646` uses the explicit optional model or its public configurable `default_model` (`"default"` alias), not a native-specific product model; J2 confidence0.95. This does not attest external caller admission or the alias's resolution in another crate.
3. **Correct native-backend-as-authority instructions.** `src/system_prompt.rs:94,202,211,476,496` now requires identity-bound declared plugin operations and existing capability admission. A native rovs-ovsdb backend or a missing native tool no longer justifies shell/transport fallback. Absent/denied operations require `cannot_perform`. Preserve legitimate “native rtnetlink” and “native bridge tools” at90/185 under the amended common rule. J3-J5 evaluate actual contexts and corrected text, not just model routing.
4. **Preserve admission failures.** `src/orchestration/services/mod.rs:155-198` rejects standalone RPC admission. `src/orchestration/grpc_pool.rs:606-623` rejects unwired scoped agent execution; `src/orchestration/workstack_executor.rs:421-442,506-525,838-849` requires verified scope and scoped tool dispatch, with no unscoped fallback. J2: `fail_closed`, confidence0.99. No changes weaken these boundaries.
5. **Distinguish protocol principle from implemented support.** `DESIGN.md:170` is a draft “must support” requirement; retain it. `SPEC.md:45` now describes using native D-Bus protocols and explicitly says generic calls are unavailable in this crate; use admitted registered tools. This matches the unavailable generic actor handlers. Keep the four accurate structured-call occurrences in SPEC and five in nl_admin. All15 incidental alternative substrings were individually judged and preserved.
6. **Do not edit generated authority independently.** Both complete prost outputs were reviewed (605 and7,634 supplied lines). They hold wire messages, generic clients and dispatch adapters; no native provider/model/config override. `build.rs` derives them from the existing domain protos. No per-plugin schema source or native-config copy was added.

## Native fixes and overlaps

Applied independent native-specific hunks:

| File/line | Correction |
| --- | --- |
| `SPEC.md:45` | Qualify native D-Bus principle with actual generic-call unavailability and admitted tools. |
| `src/system_prompt.rs:94` | Remove claimed write-tool absence plus native-backend-justified shell setup fallback; require admitted declared operation or `cannot_perform`. |
| `src/system_prompt.rs:202` | Native D-Bus/rtnetlink backends remain behind authoritative identity-bound projection/declared operations, not a parallel transport alternative. |
| `src/system_prompt.rs:211` | Common native-tool instruction explicitly preserves identity and capabilities, rejects shell/transport fallback. |
| `src/system_prompt.rs:476` | Missing native tool is not shell permission; require `cannot_perform` for unavailable/denied operations. |
| `src/system_prompt.rs:496` | Minimal prompt carries the same declared-operation/admission boundary. |

Exact pre-edit hunks: assignment `native-fix.BEFORE`, explicitly **excerpts only**, with source snapshot tags and complete-read receipt references, not falsely described as whole-file copies. Native spellings remain: `Native`→`native` at SPEC45 and `natively`→`native` at prompt94; no extra native occurrence was introduced. Line counts/positions are unchanged.

No proposed overlapping tched hunk; tched/threetched comments in `proto/chat.proto`, generated chat types and `src/chat_service.rs` were preserved for their owner. No caller outside this crate was edited or treated as audited. No shared HANDOFF/SIGNALS writes during reconciliation.

## Existing limitations observed, outside native-only repair scope

- `src/chat_service.rs:103-158` explicitly echoes a stub response rather than invoking generation. Provider/model text in that stub proves neither routing nor a working chat workflow.
- `src/agent_tools.rs:288-315` returns a success-shaped legacy stub without dispatch. The separate pool implementation rejects actual execution as unwired. No new stub/fallback was created, and this audit does not certify legacy execution.
- Fixed topology/tool lists and old deployment/service-manager guidance in prompt/docs are not live-state evidence. They were not broadly rewritten during a native-specific audit.
- Existing generic tests, including source-wording assertions, were read but not run or re-pinned. This native audit makes no test-quality or broader security-clean claim.

## Verification and deployment limits

Evidence: complete sequential file reads, per-occurrence ledger, and actual installed declarative JEV. New receipts: J3 `jdgb-15b32b095b4c308a` (33/33); J4 `jdgb-15b32b363f8c308b` (8/8); J5 `jdgb-15b32b762d0c308c` (7/7). Low-confidence decisions were not accepted as authority: source was inspected more closely, focused questions resolved parser/spec/shell ambiguities, and final instruction text was manually compared with the declared identity boundary. `judgments.md` preserves all outcomes/confidences, including low confidence and changed conclusions.

Source edit receipts establish that six exact hunks were applied. J5 checks final wording statically; it does **not** establish runtime enforcement. The remaining legacy prompt/tool/topology debt and availability of external backend tools are not certified.

No searches, arbitrary execution, Cargo, build, tests, lint, format, smoke fixtures, runtime operations, credentials, grants, seal, deployment or Git mutation. Runtime/build verification is intentionally unperformed under this assignment; coordinator owns authorized build reconciliation. No built/tested/deployed claim. HEAD/current hashes were not independently sampled; supplied hashes remain initial inventory evidence.

## Self-evaluation

| Axis | Score | Evidence / improvement |
| --- | --- | --- |
| Accuracy | 4/5 | Claims tied to sequential reads and JEV; static-only, no current hash or runtime attestation. |
| Completeness | 4/5 | All54 entries through EOF; all18 native uses plus15 supplemental substrings individually judged; six independent changes resolved statically. No runtime proof. |
| Clarity | 4/5 | Separate retained vocabulary, selection, admission and limitations; see coverage for exact ranges. |
| Actionability | 4/5 | Coordinator receives complete ledger, decisions and result receipt; generic debt remains outside authorization. |
| Conciseness | 4/5 | Per-file ledger plus bounded decisions; generated lines recorded as ranges, not duplicated. |

Overall4.0/5. Coordinator next action: reconcile the two-file patch and receipts, then perform the authorized build separately. Native occurrence review is complete; build/runtime/deployment status remains unverified. User-agreement check: every occurrence context and intent evaluated, no blanket rename, no unauthorized execution or unrelated cleanup.
