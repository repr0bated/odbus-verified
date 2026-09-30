# Btrfs cache memory and transport investigation

2026-09-30; checkout `/srv/git/odbus-verified`, branch `claude/amazing-carson-8bg4yr`, commit `abefbe435f91e4c719fabcea4e07383c5c146cfc`. Production source, live services, real caches and deployment were not changed. Existing concurrent investigation artifacts were preserved. This directory contains independent bounded probes and isolated prototypes.

## Verdict

Mixed causes were demonstrated: reachable metadata retention during unique-key growth; stale fast-path retention after expiry; child processes surviving transport cancellation; and a NUMA policy argument defect. No unreachable-allocation leak was established. Fixed-cardinality workloads and successful local Btrfs transfer cycles had bounded requested Rust memory over the tested short runs. This is **not** an attribution of the running bridge's reported multi-GiB RSS or proof of indefinite stability.

The cache's live block-writing caller bypasses the embedding/KV APIs with the metadata/expiry findings. Investigating those API defects does not explain the live bridge by itself. The earlier [memory investigation](../../../crates/op-cache/MEMORY_INVESTIGATION.md) separately analyzes the connected Snowball history parser; its prior measurements are not presented as new runs here.

## Measured results

| Case | Evidence | Conclusion |
|---|---|---|
| Fixed 16-key warm reads, 4 KiB and 64 KiB vectors | Three processes per size, 10 cycles/process, 128 reads/cycle; cycle-1 and cycle-10 requested bytes equal | No sustained requested-memory growth detected under these conditions |
| Warm KV, overwrite, clear/repopulate, construction/drop | Three processes/case, 10 cycles each; drained requested-byte endpoints equal | No growing ownership across these completed cycles |
| Four-thread read bursts | 128 reads/cycle; drained requested bytes flat; RSS/anonymous pages increase modestly | Extra resident memory does not establish live-allocation growth; thread/allocator effects remain possible |
| 256 total embedding keys, including 240 new 64 KiB texts | About 16.08 MB requested live memory at end; about 0.25 MB after clear and 0.17 MB after cache drop | Metadata remains reachable until explicit release; fast-path budget does not bound Cozo index |
| Expiry baseline and isolated invalidation | 16 x 64 KiB vectors, 10 cycles/process, three repetitions each; old files/index rows removed but baseline still returns cached value | Definite expiry correctness/retention defect; prototype removes stale hit and roughly 1 MiB retained payload |
| Cold one-byte truncation and re-save with fast path disabled | Three processes x ten cycles; malformed entry errors and every recovered vector element checked | All 30 corruption/recovery cycles passed with flat drained requested bytes |
| Real Btrfs snapshot → local send/receive → explicit cleanup | Three processes x three cycles, six members/cycle, 16 vector files plus four category payloads verified byte-for-byte | 9 complete transfers passed; every post-cleanup requested live sample was 313,614 bytes, fds 10, children 0 |
| Timed-out real forwarding helper with harmless stalled children | Three 100 ms timeouts; both children still present after another 100 ms, each trial | Cancellation does not terminate owned processes; fixture explicitly terminates/reaps them afterward |
| NUMA default/preferred/bind/interleave on separate threads | Three baseline/variant pairs; original bind/interleave return EINVAL; corrected variant reads back modes 2/3 with mask 1 | Confirmed node-mask boundary defect on this host; no multi-node performance inference |

All 39 API probe processes passed (13 configurations, three repetitions, ten cycles/process). Detailed counts, slopes, wall/CPU times, sampled latency and run variability are in [RESULTS.md](RESULTS.md), [summary.tsv](summary.tsv) and [raw/](raw/). Three steady runs contain transient three-byte checkpoint changes; their endpoints remain equal, and fitted slopes range from -0.019 to +0.091 bytes/cycle. Native measurements are in [native-summary.txt](native-raw/native-summary.txt), [cancellation.log](native-raw/cancellation.log), and `native-raw/numa-{baseline,variant}-{1,2,3}.log`.

Native post-cleanup RSS rose 12–20 KiB within each roughly 1.4–1.5 second three-cycle run while requested bytes were exactly flat. End RSS varied 35,560–36,156 KiB between runs. Requested growth was zero bytes per completed cycle and zero MiB/min over this window; such brief rates must not be projected to a production soak. The 18 observed threads stayed present after cache drop in these diagnostic processes; no accumulating child/fd count was observed.

The negative control's requested memory stayed flat. The deliberate test-only retention control measured exactly 1,048,576 bytes added per cycle and released it on drop. The predeclared 64 KiB threshold is a decision tolerance, not a validated universal detector resolution; the positive calibration was 1 MiB. Native malloc stacks, allocator resident/active statistics, kernel allocation stacks, and long-lived backlog were not measured. RSS/PSS and requested-byte deltas are independent views, not quantities to sum.

## Ownership and source evidence

| Owner/path | Allocation and intended release | Finding |
|---|---|---|
| `BtrfsCache.index` | In-memory Cozo rows store text/key/hash/path; explicit cleanup/delete/clear or cache destruction releases them | No independent byte/cardinality budget. [constructor](../../../crates/op-cache/src/btrfs_cache.rs:425), [embedding row](../../../crates/op-cache/src/btrfs_cache.rs:780), [KV row](../../../crates/op-cache/src/btrfs_cache.rs:920), [mem engine](../../../crates/op-cozo-store/src/lib.rs:217) |
| `MemoryCache` | Owns key Strings and encoded byte Vecs; eviction/remove/clear/drop releases values, HashMap clear may retain table capacity | Limits count only `data.len()`, excluding key memory and capacity; admission follows allocation and insertion; [put/eviction](../../../crates/op-cache/src/btrfs_cache.rs:204) |
| Embedding read/return | Encoded bytes coexist with decoded Vec; returned vector belongs to caller | [read/deserialization](../../../crates/op-cache/src/btrfs_cache.rs:736) has no explicit file/dimension bound. Harness releases returned values. Fast-path reads clone bytes before decoding |
| KV read/return | Whole file buffer plus cloned fast-path value, then caller-owned returned Vec | [get_raw](../../../crates/op-cache/src/btrfs_cache.rs:843); concurrent operations multiply peaks without an in-flight byte budget |
| Cleanup | Materializes selected records and copied tuples before deletion | [cleanup_old](../../../crates/op-cache/src/btrfs_cache.rs:1145) fails to evict fast path, ignores file-deletion errors and re-evaluates selection for index removal. Concurrent refresh race remains untested |
| Transport | Pipes and spawned copy task; stdout/stderr collected by wait_with_output | [pipe helper](../../../crates/op-cache/src/btrfs_cache.rs:132) streams payload, but diagnostics are unbounded and no cancellation guard terminates/reaps children or aborts the detached copy task |
| Snapshot manager | Paths/configuration/mutex in heap; six RO member subvolumes on disk | [create](../../../crates/op-cache/src/snapshot_manager.rs:201) preserves failed incomplete sets; send retains local successful set. Cleanup observed here was explicit fixture ownership, not automatic method cleanup |
| NUMA | Bounded topology/configuration/counters; per-thread kernel policies | [set_mempolicy](../../../crates/op-cache/src/numa.rs:702) uses real syscalls; bind/interleave `maxnode` omits the highest bit on this host |

The single production constructor is [OptimizedSnowball](../../../crates/op-snowball/src/btrfs_numa_integration.rs:117); its [block writer](../../../crates/op-snowball/src/btrfs_numa_integration.rs:155) writes `blocks/by-hash` directly. Workspace caller searches found no production embedding/KV insertion or cache remote-transport caller. This is current source reachability, not proof of the running binary's complete allocation profile.

SQLite is not the Btrfs cache index in this revision. Its dependencies exist elsewhere in the crate; this investigation does not misattribute their page caches or WALs to these exercised Cozo paths. The code and previous prompt's SQLite assumptions differ.

## NUMA and ephemeral transport details

Affinity is genuinely applied through `sched_setaffinity(0, ...)`. The disposable thread's readback narrowed CPUs 0–15 to CPU 0. Default and Preferred(0) policy readbacks succeeded. Existing cache configuration fields alone do not demonstrate placement of all workers or already allocated pages.

Source-only issues remain: cache LocalNode uses HashMap iteration rather than current CPU; MostMemory uses total capacity; Disabled still reaches fallback affinity in transport; affinity/policy calls occur on an async worker with no restoration and failures are logged/ignored. Fixed masks lack sufficient input validation. The single-node fallback reuses a node-meminfo parser for `/proc/meminfo`. The direct Snowball caller uses a dedicated writer, so its thread model differs from the cache transport methods. These are not experimentally demonstrated multi-node leaks.

Btrfs fixtures used the actual filesystem, source/category subvolumes, RO snapshots, zstd compression **policy**, and the unchanged production forwarding helper exposed with one visibility adapter. The sender/receiver were local Btrfs executables, not SSH. All six received members were RO and payload bytes matched. The parent snapshot's nested child stub was empty or absent after receive, as expected; separate members carried the data. Actual compressed extents and physical NUMA page locality were not benchmarked.

A first native fixture attempt incorrectly required the empty nested stub directory to survive send/receive and attempted to unset a received subvolume's RO property. Those were harness errors, not product findings. Its log is preserved as `native-initial-fixture-error.log`; only the recorded disposable fixture was removed using Btrfs subvolume deletion. The corrected harness completed all nine cycles and removed all fixture paths. It never force-converted received production snapshots.

The cancellation trials exercise the exact helper with finite idle child executables. They establish survival after cancellation, not an indefinite lost-heap allocation or a remote SSH leak. Timeout/disconnect/retry recovery and partial remote receive contents remain untested. Current incomplete-set preservation has no bounded recovery path in the reviewed owner; repeated failures therefore merit a separate bounded test. Default 24-set rotation is existing code, not a proposed archival feature. A rotation-versus-active-transfer race is source-supported but untested.

## Tested prototypes and remaining mitigations

1. **Expiry invalidation — lifecycle/correctness repair prototype.** [expiry-mitigation.diff](expiry-mitigation.diff) removes expired hashes from the fast path after the Cozo deletion. Repeated serial expiry tests lose the stale hit and release payload memory; [RESULTS.md](RESULTS.md) includes timing evidence. It is not production-ready: concurrent refresh/deletion identity consistency and failure ordering still need design and tests. It does not cap the Cozo index. No API or stored format changes.
2. **NUMA mask boundary — policy correctness prototype.** [numa-maxnode-prototype.patch](numa-maxnode-prototype.patch) changes highest-node+1 to highest-node+2, matching existing Preferred behavior and the kernel mask-import boundary. Three comparisons pass with readback. [check-numa.sh](check-numa.sh) fails on the baseline (exit 1) and passes on the variant (exit 0). It does not validate masks >= one machine word, empty/invalid nodes, restrictive cpusets or multi-node performance. No throughput/memory benefit is claimed.

Prioritized untested work, each contingent on the corresponding measured mechanism:

| Candidate | Class/evidence | Benefit, tradeoff, validation/rollback |
|---|---|---|
| Scope transfer children, output readers and copy task under cancellation; cap diagnostics | Lifecycle repair; survival reproduced | Prevent work surviving request cancellation. Requires backpressure and complete reaping; test stalled sender/receiver, second-spawn failure, high stderr and partial receive before adoption. Keep change isolated; no host privileges except native fixture |
| Separately budget Cozo metadata; retain compact hashes instead of full texts where contract allows | Bounded retention; unique-key growth measured | RAM growth becomes explicit/admitted. Needs durable-index/reopen correctness and no loss of authoritative data. Hash format/API compatibility review; rollback source/build only |
| Hashed fast-path keys; account capacity/overhead, acquire byte budget before copying, evict under one lock | Peak and retention reduction; source-supported | Avoid huge keys and concurrent overshoot; possible contention. Compare key sizes and in-flight bytes, preserve hits/data; no host changes |
| Batched cleanup with stable selected identities and fast-path invalidation | Lifecycle and peak reduction | Avoid O(expired rows) collections and refresh races. Validate delete failure and concurrent reads/writes; must preserve cache correctness |
| Allocator comparison or decay/purge tuning in isolated build | Retention experiment; residual RSS alone is insufficient evidence | Could reduce resident arenas at CPU/latency cost; first obtain compatible native heap/arena profile. No allocator interposer stacking; rollback binary only |
| Bounded buffered I/O/mappings, selective cold-file advice or compression comparison | I/O/peak experiment | Needs page-cache attribution and workload justification; page faults/I/O/CPU can worsen. No global cache dropping or production remount |
| Dedicated NUMA workers, first-touch policy or bounded per-node sharding | Placement/performance experiment | Requires multi-node hardware and actual page placement. Single-node policy correctness is not a performance result; rollback config/build |
| Pressure-aware admission, scoped cgroup memory control, THP/reclaim/compressed swap | Operational containment/experimental performance | Deferred: no evidence supports host tuning in this investigation. Privileged isolated environment and explicit rollback required; never a substitute for fixing unreleased ownership |

No allocator, kernel, swap, THP, filesystem mount or production default was changed. Before/after expiry wall/CPU timings are in summary.tsv; short instrumented runs include repopulation and differing hit/miss behavior, so they do not establish a production performance benefit or a stable overhead estimate. Latency summaries include p50, p95 and maximum, not p99. To roll back experiments, discard their standalone builds/copies under this report directory; there is no production patch to revert. Preserve raw evidence before removing reports. Temporary native data and cancellation children have already been cleaned up.

## Reproduction and limits

Start at the repository root. Exact source hashes, environment, GNU toolchain, Cargo.lock and dependency artifact hashes are recorded. Build scripts compile complete current source modules against fingerprint-pinned existing dependency artifacts. **They do not establish the full Cargo feature resolution or production release build equivalence.** Native dependencies include existing static archives; linking them does not prove their allocations were exercised or tracked. If those artifacts are absent, use a normal locked GNU Cargo build in a separately budgeted workspace before adapting the wrapper; do not select arbitrary rlibs by newest timestamp.

```sh
report=reports/btrfs-cache-memory/20260930-followup
sha256sum -c "$report/source-before.sha256"
sha256sum -c "$report/dependency-artifacts.sha256"
"$report/build.sh"
"$report/build.sh" wrapper-mitigated.rs cache-probe-mitigated
"$report/run-all.sh"
"$report/summarize.sh"
"$report/run-native.sh"
timeout -k 5 20 "$report/native-raw/cache_native_probe" cancel
# Creates only /var/tmp/op-cache-native-followup-* disposable resources:
sudo -n timeout -k 5 120 prlimit --as=1073741824 --fsize=268435456 -- \
  "$report/native-raw/cache_native_probe" native
"$report/build.sh" native_probe_numa_variant.rs native-raw/cache_native_numa_variant
"$report/check-numa.sh" "$report/native-raw/cache_native_numa_variant" /tmp/cache-numa-check.log
"$report/unit-tests-run.sh"
```

`unit-tests-run.sh` is the reduced exact-source NUMA/snapshot scope: 10 passed, 3 ignored. Three larger unit-harness compiler attempts hit the 512 MiB process-group guard, so no BtrfsCache unit-test pass is claimed. API and native probes ran independently. Repository `/verify` was executed via `scripts/check-workspace.sh verify`; it exited 1 at existing workspace formatting drift, before Clippy/workspace tests. See [verify.log](verify.log). No unrelated formatting was repaired.

Host: Artix/glibc GNU, kernel 7.2.3-x64v3-xanmod1, rustc 1.97.1, 16 allowed CPUs, one NUMA node, Btrfs source storage, 32 GiB RAM/16 GiB swap. Runtime probes ran below the 512 MiB workload limit; native children processed small synthetic fixtures. Guard sampling and phase snapshots can miss short peaks. Root-cgroup host counters are context only, and no whole-system cache/slab claim is made.

Still untested: long soak and larger cardinalities; native allocation stacks and sustained production workload attribution; dedicated cgroup accounting; multi-node physical placement; remote SSH partial failure/retry/shutdown; rotation during active transfer; payload-limit enforcement; many concurrent cold misses; failure ordering in the expiry prototype. A clean sanitizer/heap report was neither obtained nor implied. See [SKILLS-AND-SOURCES.md](SKILLS-AND-SOURCES.md) for loaded skills and official references.

A precise next validation for the remaining live-RSS question is a budgeted production-equivalent synthetic Snowball projector workload with retained native allocation stacks and phase-aligned process accounting. For cancellation, the next regression should require no surviving children/copy task after timeout and verified partial-artifact recovery. For either prototype, full Cargo verification and relevant failure/concurrency regressions remain prerequisites to production adoption. This bounded diagnostic run is complete; production leak attribution and production fixes are not claimed complete.
