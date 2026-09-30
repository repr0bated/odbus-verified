# Cache ownership implementation

Implemented 2026-09-30 in `crates/op-cache/src/btrfs_cache.rs` under OD-96.
No service restart, deployment, live cache mutation or authoritative history
deletion was performed. Existing investigation evidence is preserved. The
parent subsequently added a stale-hit assertion to the original cleanup probe
and updated its wrapper wiring for `managed_process`; earlier evidence was not
rewritten.

## Ownership and compatibility

The Cozo index remains an independent in-memory derived owner. Its embedding
and KV relations preserve their original column positions, but text/key values
are empty. A shared LRU bounds both relations together, default 4096 rows
(`OPDBUS_CACHE_INDEX_MAX_ENTRIES`). Eviction removes only metadata. Existing
SHA-256 vector/KV filenames are the fallback for evicted rows, reopened caches
and restored snapshots; bincode vector and raw KV file formats are unchanged.
This avoids adding a live RocksDB database to the root snapshot and avoids its
unconfigured native caches. A bounded LRU carries access count and creation
timestamp, saving a redundant Cozo read query on each hit.

File mtime now records durable last access, updated on every successful read
including memory hits. Expiry therefore works for files absent from the bounded
index, and last access survives restart. Statistics scan files with bounded
working memory; entry count/disk size no longer depend on index residency.
Embedding access totals count the current instance's operations, rather than
being a durable lifetime ledger. Cache metadata is derived, not authoritative.

Memory keys contain fixed SHA-256 hashes rather than supplied raw keys. The
fast path uses a BTreeMap, charges vector/key capacities plus a conservative
whole-node allowance per entry, and evicts before publishing under one lock.
The default entry/byte limits remain 1000 and 100 MiB, but byte accounting now
covers owned allocations rather than payload lengths alone. An entry that
cannot fit is rejected before making a fast-path copy. Returned public values
still own their bytes; the cache budget is not a bound on caller-owned results,
allocator arenas, native allocations, or kernel page cache.

## Consistency and bounded input

A per-instance mutex and cross-process flock on root `.cache.lock` serialize
file/index/memory transactions. The lock lies outside all cleared categories.
Payload writes use owned temporary files, fsync, atomic rename and parent sync.
Cached file inode/size/mtime is checked before a hit, so independent handles
observe replacement/deletion. Computation runs outside the lock and rechecks
before publishing, preserving concurrent puts rather than overwriting them
with stale computation.

Async raw API signatures are preserved. Their bounded synchronous transactions
do not yield while holding a standard mutex or create an unbounded queue of
owned blocking-task payloads. They can block a Tokio worker during storage I/O;
a future dedicated bounded owner can change execution placement if a real raw
API workload requires it. The current bridge stores blocks directly and does
not call embedding/KV insertion APIs. Its direct block writer does not acquire
this lock; block snapshots/clears remain rebuildable-cache operations, not an
atomic audit-history boundary.

`OPDBUS_CACHE_MAX_PAYLOAD_BYTES` defaults to 64 MiB, and
`OPDBUS_CACHE_MAX_EMBEDDING_DIMENSIONS` defaults to 4,194,304 (16 MiB of f32
elements plus header, also subject to the payload limit). File length is
validated before allocating; exact reads reject concurrent size changes.
Embedding encoded length/dimensions are checked before deserialization, and
trailing data is rejected. Oversized writes fail before serialization/copy.
Malformed integer limit configuration is reported instead of silently falling
back. Limits are configurable; zero disables the corresponding admissible
payload/dimension/index or memory budget.

Expiry scans actual embedding files using at most 128 selected paths at once,
deletes each backing file before invalidating its memory/index layers, and
reports deletion/index errors. It covers metadata eviction and reopen and no
longer leaves expired fast-path hits. Clear reports index failures, frees
container capacity and clears queries/diffs as well as embeddings/blocks/KV;
category subvolume roots, snapshots and lock files are preserved. Partial
storage failures can leave partial progress, but stale memory cannot conceal a
missing or nonregular backing file. Constructor/property commands use the
parent's bounded child-owner helper, including cancellation and timeout.

## Verification and requested-byte profiles

`cache-validation/wrapper.rs` compiles the exact current production modules
with native GNU rustc and recorded prebuilt dependency/native archives. Final
focused execution: **25 passed, 0 failed, 1 ignored**. It includes the placement
agent's pure NUMA regression. Fixture roots were owned temporary directories on
`/dev/shm`, automatically removed; native Btrfs send/receive is outside this
ordinary-filesystem test evidence. Build/test logs are `final-*-build.log` and
`final-tests.log` in `cache-validation/`. Parent-managed Cargo/native verification
is recorded separately and must not be inferred from these wrappers.

Regressions cover combined index bounds/empty text-key columns, fallback after
eviction/reopen, key/capacity/node accounting, concurrent memory insertion,
durable-access expiry, stale recomputation, backing failure, forged/truncated/
trailing/oversized embeddings, independent handles, compute/put races, and clear
ownership. Existing basic cache tests also pass.

The new allocation probe was copied from the original with module wiring
changed. The parent subsequently tightened the original cleanup assertion and
wrapper wiring as described above. A tracking allocator counts
requested live Rust bytes against an initialized-cache baseline. It excludes
RSS, allocator arenas, native allocations and kernel memory.

| Case | Baseline retained bytes | Fixed retained bytes | Reduction |
| --- | ---: | ---: | ---: |
| 256 unique 64 KiB source texts, memory fast path disabled | 16,873,998 | 134,912 | 99.20% |
| 32 unique 256 KiB keys, 32-byte payload budget | 16,794,192 | 17,184 | 99.90% |
| Eight expired vectors after cleanup | 2,098,816 | 1,760 | 99.92% |

Expired files: zero. Post-expiry hit: false. Explicit embedding/full clear
returned live-allocation deltas to zero. Metadata still scales with admitted
row count until the independent 4096-row cap; large input text/key length no
longer scales retained metadata. The regression independently checks a three-
row cap and surviving disk lookups rather than assuming these 256-row profiles
demonstrate saturation.

Paired warm-read probes use the same runtime, dependency artifacts, `/dev/shm`
fixture and verified 1000 reads per size. Baseline source is the earlier
follow-up's unchanged `source/btrfs_cache.rs` (SHA-256
`406dfaba16786a5a3733c7b15ac7021ebb865015a7d899be8258f604b4033239`). Median of
five fresh-process runs:

| Payload | Baseline us/read | Fixed us/read | Change |
| --- | ---: | ---: | ---: |
| 256 bytes | 2,116.133 | 1,342.043 | -36.6% |
| 65,536 bytes | 2,153.412 | 1,348.419 | -37.4% |

These debug-artifact synthetic results compare this workload only. The first
fixed variant retained two Cozo queries and was about 20% slower; retaining
fixed counters in the bounded LRU removes that redundant query. Kernel page
placement, native allocations, production bridge RSS and network transport
latency are not attributed by these measurements.

Final warm probe for optional parent eBPF observation:
`/var/tmp/opdbus-cache-final-latency-current`, no arguments, default cache limit
environment. It removes its own `/dev/shm` fixture on exit.

Read-only integration review of `managed_process.rs`, the snapshot temporary
owner, transport admission and stream/receive methods found no new blocker:
commands have bounded diagnostics/timeouts, cancellation drops owned children,
permits survive cleanup, and the existing six-member layout is preserved.
Remote failed sends leave an uncommitted operator-inspectable directory; this
work does not claim remote recovery or atomic multi-member snapshots.

## Default NUMA child portability follow-up

The final transport integration initially made a rejected `MPOL_DEFAULT`
reset fatal to child spawn. This differed from the prior best-effort default
and could disable ordinary transport in seccomp-restricted containers.
`NumaOptimizer::configure_command` now still attempts the default reset, but
tolerates only `EPERM`, `EACCES` and `ENOSYS` for that default mode. Under those
restrictions the child inherits its existing memory policy; a reset is not
guaranteed. Explicit Bind, Interleave and Preferred requests remain strict,
as do every other kernel error and CPU placement failures.

Two regressions cover the exact error classification and a real child-scoped
seccomp filter that rejects `set_mempolicy`. On this host the filter ran, rather
than being skipped: default CPU-pinned execution succeeded, all three explicit
memory policies failed with `EPERM`, and parent CPU/memory readback was unchanged.
No filter or placement policy was installed on the test runner, service or host.
The isolated exact-source native GNU NUMA wrapper passed **18 tests, 0 failed,
2 hardware-specific ignores**. Build peak process-group RSS was 204,868 KiB,
below a 786,432 KiB guard. Logs and wrapper are
`cache-validation/numa-portability-{build,tests}.log` and
`cache-validation/numa-portability-wrapper.rs`.
