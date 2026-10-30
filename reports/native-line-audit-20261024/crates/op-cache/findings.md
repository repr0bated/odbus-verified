# op-cache native contextual audit

Task `native-line-audit-20261024`; Codex op-cache worker; 2026-10-24.

## Outcome

Complete source review: all 28 supplied inventory entries, 13,168 source lines, sequential ranges through EOF. Directory listings of crate root, src, src/grpc, examples and proto reconciled the inventory; no additional files or nested instructions were encountered. All inventory entries were readable text; no binary/missing/symlink/dependency exception required. Supplied initial SHA-256 receipts are retained in coverage.tsv, not represented as recomputed current hashes.

Occurrence reconciliation supersedes the earlier routing-only completion: 19 case/compound `native` occurrences across six files are now individually mapped in occurrences.tsv to actual contextual JEV judgments, including both positions on MEMORY_INVESTIGATION.md:28 and the substring in Alternative Analyzer. All 19 tokens are appropriate and preserved. One associated wording defect is fixed: src/numa.rs:1189-1190 no longer claims default native memory-policy setup always succeeds. No provider/model/profile override, stale native product gate or parallel authority found; no tched/threetched overlap. No executable behavior changed.

## Native-context dispositions

| Evidence | Context | Decision |
| --- | --- | --- |
| `DESIGN.md:57-62` | Native Btrfs/NUMA fixture requirements. | Preserve storage/hardware terminology. |
| `MEMORY_INVESTIGATION.md:1-245` | Historical allocator, GNU compiler, Btrfs fixture and measurement boundaries; explicitly labels baseline as superseded. | Preserve historical measurements and native-vs-Rust allocation distinctions; no routing semantics. |
| `examples/cache_memory_probe.rs:1-175` | Synthetic Rust allocation probe distinguishes ordinary filesystem from native Btrfs behavior. | Preserve; not executed. |
| `src/btrfs_cache.rs:1801-1947` | Native Btrfs send/receive test requires privileged isolated fixture. | Preserve descriptive test requirement; not executed. |
| `src/numa.rs:901-1470` | Native-named tests/helper exercise Linux CPU affinity and set/get_mempolicy, explicitly skipping denied/unsupported default-policy setup. | Preserve all seven symbols/messages; correct adjacent always-succeed comment at1189-1190. |
| `src/btrfs_cache.rs:611-810,1586-1610`; `src/numa.rs:601-981` | Environment selections govern cache CPU/memory placement and transport children. | Hardware/storage policy, not implicit LLM configuration override. Preserve. |
| `src/agent_registry.rs:1-548`; `src/capability_resolver.rs:1-543`; both orchestrator modules | Generic capability scheduling, explicit preferred/excluded agent IDs and registered executors. | No native provider/model override established. Preserve within native-only task. |
| `src/grpc/server.rs:43-52,118-201` | Both serve paths propagate unconditional retirement error before binding. | Preserve fail-closed standalone ingress. Source does not establish deployment or canonical admission of arbitrary local callers. |

Earlier J1-J4 receipts remain evidence. J5 evaluates every actual occurrence for wording/intent: documentation preserve (0.79), Alternative compound preserve (0.98), real Btrfs fixture preserve (0.76), NUMA labels preserve with adjacent comment correction (0.84). J6 validates refined setup-specific comment (0.76); a preliminary low-confidence candidate was not used as decisive evidence. Full receipts and contrary outcomes in judgments.md.

## Exact occurrence reconciliation

| File | Occurrences |
| --- | ---: |
| DESIGN.md | 1 |
| MEMORY_INVESTIGATION.md | 8 |
| examples/cache_memory_probe.rs | 1 |
| src/btrfs_cache.rs | 1 |
| src/capability_resolver.rs | 1 (substring in Alternative) |
| src/numa.rs | 7 |
| Remaining 22 inventory entries | 0 |
| Total | 19 |

All earlier 28-file sequential reads remain evidence; no new full-file coverage fabricated. Exact-position reconciliation rereads: DESIGN.md:1-62; MEMORY_INVESTIGATION.md:1-245; example:1-175; resolver:1-280,281-543; NUMA:901-1200,1201-1470; Btrfs:1801-2100. Focused final pre-edit NUMA read:1187-1192. No searches/extraction. Assignment-directory listing contained no coordinator-followup.md.

### N1 — inaccurate native default-policy fixture comment, resolved

Original src/numa.rs:1189-1190 said MPOL_DEFAULT “should always succeed.” The associated assert_native_memory_policy helper explicitly skips EPERM/EACCES/ENOSYS during default-policy setup (1357-1365). Corrected only the two comments to state restoration when permitted and skips for denied/unsupported setup. Native token/caller, helper behavior, assertions and permissions preserved. Exact BEFORE affected-function copy saved at `/var/tmp/native-line-audit-20261024.ILroaQ/crates/op-cache/numa.rs.before-1187-1192`; fresh pre-edit snapshot D7DE, successful edit receipt CF82. No unresolved native-specific change remains.

Historical GNU compilation wording remains historical: this review does not repeat or freshly establish those recorded build receipts. Btrfs root wording is preserved as conventional privileged fixture guidance, not an exhaustive claim about every capability-enabled environment. None of the fixture names/messages asserts this audit ran a test.


## Coordinator observations — outside native-specific edit ownership

These are source-level risks, not runtime reproductions, and do not make a blanket native rename or unrequested refactor appropriate.

### O1 — cache identity omits selected agent sequence

- `src/grpc/orchestrator_service.rs:148-238,371-374,435-546,559-567`: workstack ID is derived only from original input; per-step lookup combines that ID, step index and current-input hash. A hit skips executing the selected agent. Both resolved and explicitly supplied agent sequences share this behavior; streaming also uses input-only ID.
- `src/grpc/cache_service.rs:75-79,90-110`: key has no agent identity.
- `src/workflow_executor.rs:276-372`: ad-hoc workflow ID is `adhoc-<input_hash[..8]>`, again omitting sequence; the cache hit skips selected agent execution.
- Consequence [INFERENCE]: same input with a changed selected sequence can return cached output from the prior sequence. This is generic caching correctness, not a demonstrated native-specific profile override or deployed symptom.
- Proposed coordinator contract: include ordered agent sequence in each generated workstack/ad-hoc ID (as `src/orchestrator.rs:294,398-404` already does), consistently across unary/stream/explicit entry points; preserve verified caller scope and invalidation behavior. No patch applied.
- JEV grounded support: confidence 0.95, misleading 0.3/3.

### O2 — expired workstack cache path recursively locks database mutex

- `src/workstack_cache.rs:95-129`: get() retains `let db = self.db.lock().unwrap()` while invoking `invalidate_entry` for expired entries.
- `src/workstack_cache.rs:224-242`: invalidate_entry immediately locks the same std::sync::Mutex.
- Consequence [INFERENCE]: expired get can self-deadlock. No runtime reproduction attempted.
- Proposed coordinator hunk: release db guard before calling invalidate_entry (matching `src/workflow_cache.rs:167-175`); retain original invalidation/error semantics and review concurrent-writer handling. No patch applied.
- JEV grounded support: confidence 0.86, misleading 0.7/3; static lock path only.

Known SQLite migration debt in four files is already explicit in root AGENTS.md. It is not a native-routing defect and was not expanded into this task.

## Verification and limitations

Evidence is source reads, edit receipt and declarative JEV only. No searches, recursive delegation, Cargo, tests, smoke fixtures, lint, formatting, runtime operations, deployment, sealing, identity or grant changes. Only two comment lines changed; no significant behavioral change to exercise. Build/test/deployment status remains unestablished; coordinator owns any authorized checks after reconciliation.

One documented tool mismatch: `judge_batch` was not defined in JavaScript despite xd://eval/judge documentation; the failed attempt evaluated no source. Reported to xd://report_issue and used supported jev_verify checks; successful judge() and jev_verify receipts are preserved.

Administrative exception to per-crate common instructions: higher-priority repository/developer handoff requirement required a minimal op-cache claim and completion handoff in shared HANDOFF.md. No other agent row or source was overwritten. No SIGNALS edit.

## Self-evaluation

Accuracy 4/5: exact occurrence mapping and actual JEV receipts; historical runtime receipts not freshly reproduced. Completeness 4/5: 28-file coverage retained, all 19 case/compound occurrences dispositioned and sole wording correction applied; static-only authority limits execution evidence. Clarity 4/5: native wording repair separated from generic risks. Actionability 4/5: coordinator receipt includes exact patch/BEFORE hunk and generic observations. Conciseness 4/5: details split into coverage/occurrence/JEV ledgers. Mean 4.0/5. Next: coordinator aggregate reconciliation; no unresolved native-specific op-cache action.
