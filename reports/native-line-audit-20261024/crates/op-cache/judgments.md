# Declarative JEV receipts — op-cache

Successful installed `judge()` call through functions.eval (JavaScript), 2026-10-24. No router/model-selection extension loaded. Source evidence read sequentially before submitting. Results are source judgments, not runtime verification.

## J1 — routing/configuration/admission context

Evidence:
- `src/grpc/server.rs:43-52,118-120,160-164`: validate_transport_security requires loopback then unconditionally bails that standalone cache ingress is retired; both serve entry points propagate this before binding.
- `src/btrfs_cache.rs:611-700,1586-1610`: OPDBUS_CACHE_PLACEMENT selects local/round-robin/most-memory/disabled; OPDBUS_CACHE_MEMORY_POLICY selects bind/preferred/interleave/default; configure_transport_command uses these to configure NUMA placement of btrfs/ssh children.
- `src/capability_resolver.rs:134-309`: registry candidates match required capabilities; excluded/disabled filtered; scoring uses capabilities, latency, explicit preferred_agents, priority, parallelizable flag, not provider/model/native profile.

Declared questions and contrary outcomes:
1. Does shown standalone listener permit binding without canonical admission? Choices blocked / permitted / insufficient. Answer `blocked`, confidence 0.88; probabilities blocked 0.92, permitted 0.04, insufficient 0.04. Decision: preserve rejection; no new ingress/admission changes.
2. Are cache environment policies hardware/storage configuration or implicit model/provider/profile override? Choices hardware_storage / model_override / insufficient. Answer `hardware_storage`, confidence 1.0; probabilities 1/0/0. Decision: preserve valid NUMA and cache policies.
3. Does capability resolver implement generic agent scheduling or force native model/provider configuration? Choices generic / override / insufficient. Answer `generic`, confidence 1.0; probabilities 1/0/0. Decision: no native-specific fix in resolver.

Exact tool result:
```json
{"transport":{"type":"choice","choice":"blocked","confidence":0.88,"probabilities":{"blocked":0.92,"permitted":0.04,"insufficient":0.04}},"native_config":{"type":"choice","choice":"hardware_storage","confidence":1,"probabilities":{"hardware_storage":1,"model_override":0,"insufficient":0}},"resolver":{"type":"choice","choice":"generic","confidence":1,"probabilities":{"generic":1,"override":0,"insufficient":0}}}
```

## Failed batch facility, supported fallback

Attempted the documented `judge_batch` through functions.eval JavaScript after J1. Exact failure: `ReferenceError: judge_batch is not defined`, cell exited 1. No batch judgments were returned. The installed `judge()` had already succeeded. Reported mismatch to `xd://report_issue`; response: `Noted, thanks!`. No tools/extensions enabled or client invented. Used supported `jev_verify` for the following independent declarative evidence checks. Its contrary outcomes are supports / contradicts / insufficient; no quote was requested (empty quote), so low verbatim scores do not indicate a fabricated quotation.

## J2 — legitimate native terminology

Claim: The cited native references describe Linux affinity/memory-policy readback, Btrfs fixtures, and allocator accounting, not native provider/model/profile routing; preserve them.

Evidence submitted: `src/numa.rs` native-named helper/tests call `assert_native_memory_policy` for Default/Bind/Interleave; helper applies `NumaOptimizer::apply_memory_policy`, invokes `read_memory_policy`, and asserts mode/node readback. `read_memory_policy` invokes `libc::SYS_get_mempolicy`. `native_affinity_readback_is_exact_and_caller_is_unchanged` applies CPU affinity on a spawned thread and asserts caller affinity unchanged. `src/btrfs_cache.rs:1806-1809` ignored test labels native Btrfs send/receive as requiring root and isolated fixture cleanup. `examples/cache_memory_probe.rs:1-5` explicitly excludes native Btrfs snapshots/kernel cache/live data from the ordinary-filesystem probe. `MEMORY_INVESTIGATION.md:25-29` distinguishes native allocator arenas/allocations and kernel page cache from counted Rust allocations.

Actual result: `verdict=supports; verbatim=0.06; misleading=0.2/3; confidence=0.94; tokens(in/out)=869/78`.

Decision: preserve hardware/storage/library wording and test helper names. Not a native profile/provider override.

## J3 — generic selected-agent cache collision

Claim: The shown grpc orchestrator and ad-hoc workflow cache IDs omit the selected agent sequence. With caching enabled, identical input can reuse output from different explicitly selected agents. This is a generic cache-key defect, not evidence of a native-specific provider/profile override; report for coordinator without editing under native-only ownership.

Evidence submitted: `src/grpc/orchestrator_service.rs` execute_workstack loops selected agent_ids, computes current-input hash, and queries get_step_internal(workstack_id, step_index, input_hash). A hit returns cached output without agent execution. execute and execute_agents derive workstack_id as `ws-<hash(req.input)[..12]>` and pass agent_ids separately. `src/grpc/cache_service.rs::make_cache_key` hashes workstack_id:step_index:input_hash. `src/workflow_executor.rs::execute_sequence` derives workflow_id as `adhoc-<input_hash[..8]>`; cache lookup uses that ID, step index and current-input hash and bypasses execute_step on hit. Assignment limits edits to native-specific defects.

Actual result: `verdict=supports; verbatim=0.06; misleading=0.3/3; confidence=0.95; tokens(in/out)=833/78`.

Decision: report generic correctness observation O1 and concrete coordinator keying contract; no native-only patch or deployed-failure claim.

## J4 — generic expired-cache recursive mutex

Claim: WorkstackCache::get calls invalidate_entry while retaining its non-reentrant std::sync::Mutex database guard on the expired-entry branch; invalidate_entry tries to lock the same mutex, producing a self-deadlock risk. This is general cache debt outside native-specific edit scope.

Evidence submitted: `src/workstack_cache.rs` stores `Mutex<rusqlite::Connection>` imported from std::sync. get locks db, reads entry, then calls invalidate_entry if now > expires_at with no drop(db). invalidate_entry immediately locks self.db again. Nonexpired get later uses its guard for stats updates. Assignment limits independent edits to native-specific defects.

Actual result: `verdict=supports; verbatim=0.08; misleading=0.7/3; confidence=0.86; tokens(in/out)=728/78`.

Decision: report source-only risk O2; propose dropping guard before invalidation with concurrent-writer review, not automatic unrelated repair. No runtime reproduction.

## J5 every occurrence context

Clarified owner scope: assess every case/compound `native` use for wording, claimed behavior and intent, not only routing. Earlier complete line coverage retained. Sequential contextual rereads accounted for 19 occurrences in six files. No coordinator-followup.md appeared in the complete assignment-directory listing.

Successful installed `judge()` call supplied one shared evidence state containing all actual occurrence contexts:

- `DESIGN.md:58-62`: “Native fixture tests must check reopen, child data in snapshots, read-only members, actual send/receive, preservation of historical sets and category roots, and the real writer's affinity after a write.” It also requires Btrfs privileges and says parser/ordinary-filesystem tests do not establish host behavior.
- `MEMORY_INVESTIGATION.md:27`: “not verify native Btrfs behavior”; line28 first occurrence “native allocator arenas”, second “native allocations”; line42 “a native GNU” compiler; line43 “or native” Btrfs fixture excluded by parser probe; line196 “native GNU toolchain”; line212 “successful native” GNU fallback build; line239 “Neither is a native Btrfs correctness test or a production heap profile.” Opening baseline disclaimer and recorded earlier checkout were supplied, with all historical measurements/reproduction limitations read.
- `examples/cache_memory_probe.rs:3-5`: full temporary /dev/shm ordinary-filesystem disclaimer, including line4 “this does not exercise native Btrfs snapshots, kernel cache, or live data.” Supplied code context: tracking GlobalAlloc forwards to System, counts requested layout sizes.
- `src/capability_resolver.rs:505-527`: line510 “Alternative Analyzer” is a display name for a second faster analyzer; test explicitly prefers original analyzer and asserts it is selected. This literal substring occurrence is included even though its meaning is not the standalone word.
- `src/btrfs_cache.rs:1807-1943`: exact ignored-fixture reason “native Btrfs send/receive requires root; isolated fixture deletes only its own subvolumes”; actual fixture creates cache/snapshots, sends/receives every member, verifies data/ro, source mutation isolation, reopen and owned cleanup. No passing/deployed claim.
- `src/numa.rs:1191,1336,1341,1344,1354,1363,1422`: all three assert_native_memory_policy calls (Default/Bind/Interleave), helper declaration, both SKIP native policy readback messages, and native_affinity_readback_is_exact_and_caller_is_unchanged declaration supplied. Helper cfg-Linux thread, node0 skip, EPERM/EACCES/ENOSYS setup skip, actual set/get_mempolicy assertions and affinity/caller invariants supplied. Adjacent original comment at1189-1190 claiming MPOL_DEFAULT always succeeds supplied explicitly.

Declared questions and contrary outcomes:

- `documentation`: assess every supplied DESIGN/investigation/example native occurrence for semantic and wording accuracy within requirements/historical boundaries. Choices preserve / change_needed / insufficient.
- `compound`: assess Alternative Analyzer actual context. Choices preserve accurate second-analyzer label / change_needed / insufficient.
- `btrfs_wording`: assess native label and conventional root/privileged integration-fixture wording, not deployed state. Choices preserve / change_needed / insufficient.
- `numa_wording`: assess all seven labels/messages and helper semantics, including adjacent absolute success assertion. Choices preserve / comment_change / native_change / insufficient.

Actual successful result:

```json
{
  "documentation": {"type":"choice","choice":"preserve","confidence":0.79,"probabilities":{"preserve":0.86,"change_needed":0.13,"insufficient":0.01}},
  "compound": {"type":"choice","choice":"preserve","confidence":0.98,"probabilities":{"preserve":0.99,"change_needed":0.01,"insufficient":0}},
  "btrfs_wording": {"type":"choice","choice":"preserve","confidence":0.76,"probabilities":{"preserve":0.84,"change_needed":0.15,"insufficient":0.01}},
  "numa_wording": {"type":"choice","choice":"comment_change","confidence":0.84,"probabilities":{"preserve":0.1,"comment_change":0.88,"native_change":0.01,"insufficient":0.01}}
}
```

Decision: preserve all 19 actual tokens. Correct only the inaccurate always-succeed comment adjacent to the Default native fixture. Per-occurrence mapping is occurrences.tsv. The first J5 tool attempt had an extra closing brace: SyntaxError, expected comma; no judgment returned. Corrected syntax, successful result above; no evidence skipped.

## J6 comment repair validation

First candidate: “MPOL_DEFAULT restores the kernel default policy when permitted. The helper reports a skip if the kernel denies memory-policy syscalls.” Submitted with exact source behavior to jev_verify. Result `supports — low confidence, review; verbatim=0.23; misleading=1.3/3; confidence=0.36; tokens(in/out)=752/78`. Not accepted as decisive. Reviewed exact helper: its skip applies to default-policy setup, not every later failure.

Refined candidate:

```rust
        // MPOL_DEFAULT restores the kernel default policy when permitted.
        // The helper skips denied or unsupported default-policy setup.
```

Successful `judge()` supplied the complete exact original test function (1187-1192), complete helper (1344-1380) and refined two-line comment. Asked whether this specifically describes the Default fixture/setup guard without promising all errors skip; choices accurate / inaccurate / insufficient.

Actual result:

```json
{"accuracy":{"type":"choice","choice":"accurate","confidence":0.76,"probabilities":{"accurate":0.84,"inaccurate":0.15,"insufficient":0.01}}}
```

Decision: applied only the two comment lines. Exact BEFORE affected-function copy: `/var/tmp/native-line-audit-20261024.ILroaQ/crates/op-cache/numa.rs.before-1187-1192`; source freshly reread before mutation, snapshot D7DE. Edit receipt CF82 shows corrected comment, unchanged caller and 1470 source lines. This is comment semantics evidence, not runtime/test verification. No executable code, assertions, skip conditions, identities or grants changed.
