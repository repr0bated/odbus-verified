# Bounded exact-source API follow-up — 2026-09-30

39 independent processes passed: 13 case/size configurations × three repeats, each with ten cycles in the same process. Main suite ran 09:01:35–09:03:04 UTC; the separate cold-corruption addition followed. All final guards report exit 0; none exceeded 512 MiB RSS or 120 seconds. Largest checkpoint RSS was 68,080 KiB (66.5 MiB). No production source was modified.

The current `BtrfsCache`, `numa` and `snapshot_manager` source was copied byte-for-byte into `source/` and compiled through `#[path]`, using existing compatible GNU dependency rlibs. This is the actual cache implementation, not a simplified replacement. Source hashes before and after match, including production `btrfs_cache.rs` SHA-256 `406dfaba16786a5a3733c7b15ac7021ebb865015a7d899be8258f604b4033239`. Dependency bytes and compiler version are recorded. Dependencies were reused, not rebuilt or claimed to represent a newly verified workspace build.

All fixtures used `/dev/shm` tmpfs, explicitly the ordinary-filesystem fallback. Native Btrfs compression, snapshots and transport are outside this API suite; separate native/NUMA evidence is being assembled by the parent investigator. Placement was disabled and memory policy default. Fast path limits were 64 entries/4 MiB, except the cold case (zero/zero).

| Case (three repeats each) | Requested live bytes, cycle 1 → 10 | RSS change cycle 1 → 10 | Result |
|---|---:|---:|---|
| Warm embeddings, 16 × 4 KiB; 128 reads/cycle | 316,440 → 316,440 | +28 to +36 KiB | Returned vectors released after every read |
| Warm embeddings, 16 × 64 KiB; 128 reads/cycle | 1,299,481 → 1,299,481 | +64 to +72 KiB | Stable retained allocation at checkpoints |
| Warm KV, 16 × 64 KiB; 128 reads/cycle | 2,357,004 → 2,357,004 | +60 to +64 KiB | Includes the embedding fixture plus KV fixture |
| Overwrites, 16 × 64 KiB; 16 overwrite/read pairs/cycle | 1,299,486–1,300,030 at both ends | 0 KiB | Expected new value checked each cycle |
| Clear/repopulate, 16 × 64 KiB | 1,299,482 → 1,299,482 | 0 to +8 KiB | Clear returns to 243,466 requested bytes |
| Construction/populate/drop, 16 × 64 KiB | 173,283 → 173,283 after each drop | −20 to +60 KiB | Final release: 172,347 bytes; initial runtime threads persist |
| Four-thread bursts, 32 reads/thread/cycle, 64 KiB | 1,299,582 → 1,299,582 | +1,452 to +1,624 KiB | Rust live bytes flat while RSS/anonymous capacity rises |
| Unique text growth: bounded 256 rows | 2,884,395 → 16,077,621–16,077,627 | +26,868 to +26,932 KiB | Retained metadata tracks additional source text |
| Allocation/drop negative control, 128 × 64 KiB/cycle | 51,602 → 51,602 | +8 to +12 KiB | Final release: 50,426 bytes |
| Controlled retained 1 MiB/cycle | 1,100,179 → 10,537,363 | +9,264 to +9,268 KiB | Exactly 1,048,576 requested bytes/cycle; release: 50,427 bytes |
| Expiry then repopulate | 1,299,483–1,300,027 at both ends | +44 to +116 KiB | Deleted rows/files still hit the memory fast path |
| Isolated expiry invalidation then repopulate | 1,299,503 → 1,299,503 | +36 to +44 KiB | Expired hits absent; expired checkpoint: 243,487 bytes |
| Cold one-byte truncation/re-save, zero fast path | 247,885 → 247,885 | +64 KiB | Every truncation errors; re-save recovers all decoded elements |

All steady cases have zero cycle-1-to-cycle-10 requested-byte increase. Three repeats (warm 4 KiB, KV and clear/repopulate) contain a transient three-byte checkpoint variation; regression slopes lie between −0.019 and +0.091 byte/cycle rather than being mathematically zero everywhere. No bounded steady-growth signal was observed. This does **not** exclude leaks in native direct `malloc`, unmeasured paths, longer runs, or the live bridge. Rust `GlobalAlloc` counts requested allocation sizes; it does not measure allocator capacity. RSS/PSS/anonymous/private-dirty measurements are separate `/proc/self/smaps_rollup` checkpoints. Nonzero RSS after clear/drop is not itself evidence of still-live cache objects.

The unique-key case starts with 16 short fixture keys, then adds 24 distinct 64 KiB texts/cycle: 240 long-text rows plus 16 initial rows, at most 256 rows and about 15 MiB source text. Vector payloads for those new rows are four bytes each. Disk growth is expected for new entries; the Cozo in-memory index also stores the complete source text, independently of the fast-path cap. Clear returns requested bytes to 249,707; drop to 172,101. This is retained, uncapped index metadata tied to live rows, not demonstrated unreachable allocation.

Every completed main checkpoint reported ten descriptors and zero children. Cache cases usually retained 18 runtime/Cozo threads; one burst checkpoint observed 19 during thread teardown. Negative/calibration controls had one thread. Thread counts are checkpoints, not maxima while the four burst workers run.

Lookup latency was sampled for every one of the 128 bounded reads/cycle. Across repeats, cycle p50 embedding lookup ranges were 2.13–2.77 ms (4 KiB) and 4.18–4.84 ms (64 KiB); highest cycle p95 was 4.24 and 7.31 ms respectively. KV p50 was 1.97–2.27 ms, highest p95 3.49 ms. Four-thread burst p50 was 4.58–5.76 ms, highest p95 9.05 ms. These instrumented, unoptimized-wrapper numbers are diagnostic, not a production benchmark. Overwrite samples cover overwrite-plus-read; clear, growth, controls and cold recovery samples are batch/pair durations. Construction and expiry have wall/CPU checkpoint timing and no latency samples. `summary.tsv` preserves per-repeat wall/CPU and latency fields; raw JSONL preserves every checkpoint.

Main decoded-value assertions check returned length and first scalar/byte, including cycle-specific overwritten values. The added cold recovery mode checks every vector element. Returned values are dropped immediately; intentional retained allocation exists only in the calibration control.

`expiry-mitigation.diff` is the only behavior-changing experiment. In the isolated `source-mitigated/` copy, successful expiry deletion removes each corresponding `emb:<hash>` fast-path entry. Three repeats × ten cycles remove all 16 rows/files; baseline stays readable with about 1.23 MiB requested allocation at the expired checkpoint, while mitigation returns an absent value and about 238 KiB requested allocation. It fixes the demonstrated sequential stale-read/retention behavior. Concurrent cleanup/write races and existing ignored file-deletion errors are not resolved or validated by this experiment; it is not a production-ready patch claim.

Evidence and reproduction: `raw/`, `summary.tsv`, `source-{before,after}.sha256`, `dependency-{rlibs,native}.sha256`, `harness.sha256`, `harness-final.sha256`, `toolchain.txt`, and `COMMANDS.md`. An initial watchdog `/proc` disappearance race interrupted orchestration after a normally exiting probe; its outputs were preserved under `raw/initial-watchdog-race/`, excluded from final counts. The watchdog now tolerates `/proc` disappearing at process exit. All task-owned tmpfs directories were removed by the fixture destructor.
