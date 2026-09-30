# Btrfs cache memory investigation

This records the unchanged baseline. Production fixes and follow-up measurements
are documented in [the implementation report](../../reports/btrfs-cache-memory/20260930-implementation/REPORT.md); the findings below describe the earlier checkout.

Investigation date: 2026-09-30. Reviewed checkout:
`abefbe435f91e4c719fabcea4e07383c5c146cfc`.

The strongest memory-pressure finding on a current bridge call path is the
Snowball vector projector: it reads and parses the entire timing history before
applying its 256-block work limit. A synthetic probe reproduces allocations that
grow with history size and are freed afterward, while process RSS stays high.
This establishes allocation amplification, not an unreachable-allocation leak
or attribution of the running bridge's reported 10–12 GiB resident set.

The public BtrfsCache embedding/KV APIs have separate retention defects: an
uncapped in-memory metadata index, unaccounted key bytes, and stale fast-path
entries after embedding expiry. Current source does not call those insertion
APIs from the bridge. No production implementation, service, live cache,
snapshot, catalog, or deployment was changed for this investigation.

## Evidence and scope

The new [cache probe](examples/cache_memory_probe.rs) calls the actual public
cache API with synthetic data in an automatically removed `/dev/shm` directory.
It exercises the ordinary-filesystem fallback and userspace ownership; it does
not verify native Btrfs behavior. A tracking global allocator measures requested
Rust allocation bytes, excluding native allocator arenas, native allocations,
and kernel page cache. Measurements are deltas from an initialized cache.
Dropping that cache can produce a negative delta because its initial allocations
are part of the baseline.

The successful cache measurements used the [API wrapper script](../../scripts/probes/cache-memory-api.sh),
which compiles the current cache, NUMA and snapshot modules and the probe, linking
recorded prebuilt dependencies including `op-cozo-store`. This verifies the
current cache implementation against that artifact set; it is not a fresh
compilation of every dependency.

The [parser probe](../../scripts/probes/cache-memory-parser.sh) extracts the
unchanged parser and flattening helpers from the hash-pinned Snowball source.
It recreates the vector construction and subsequent filter with all vectors
already present. It uses recorded local dependency artifacts and a native GNU
compiler. It does not execute a full bridge, provider call, disk scan, or native
Btrfs fixture. Source hashes, toolchain, commands, output, and verification logs
are retained under `/var/tmp/opdbus-cache-memory-review/`.

## The projector allocates for the entire retained history

[project_pending](../op-cognitive-mcp/src/chain_vectors.rs), lines 313–319, calls
`self.chain.blocks()` before filtering and taking `limit`.
[StreamingSnowball::blocks](../op-snowball/src/snowball.rs), lines 529–565,
reads every timing JSON into an owned `ChainBlockRef`, then sorts the full vector.
The helper named `blocks_missing_vectors` also loads everything first.

The bridge sets `PASS_LIMIT=256` and a 30-second sweep in
[chain_vector_projector.rs](../op-grpc-bridge/src/chain_vector_projector.rs),
lines 31–35 and 202–232. Notifications can trigger another pass sooner, and a
full batch triggers an immediate next pass. Therefore the work limit bounds
embedding calls, not scan memory. This path is wired by
[MutationEngine](../op-grpc-bridge/src/mutation_engine.rs), line 2062.

Measured with synthetic 900-byte records, all already vectorized:

| Records | Retained Rust bytes during scan | Peak extra Rust bytes | Extra Rust bytes after filter and drop |
|---:|---:|---:|---:|
| 1,000 | 3,620,648 | 3,772,648 | 0 |
| 5,000 | 18,570,184 | 19,330,184 | 0 |
| 10,000 | 37,140,368 | 38,660,368 | 0 |

In the recorded 10,000-record pass, RSS was 52,044 KiB during the scan and
49,608 KiB after drop. This demonstrates why high RSS alone is insufficient to
identify a leak. It does not quantify this effect in the deployed bridge;
actual payload shapes, concurrency, and allocator state were not sampled.

A follow-up diagnostic also measured the empty `pending` vector before drop:
its reused capacity was 16,384 elements, retaining 2,490,368 bytes even after
the 10,000 records had been filtered out. The backing allocation is released
at function exit. For nonempty batches, retained vector capacity can also span
the awaited embedding operations; that extension is a source inference.

`ChainBlockRef::from_timing_bytes` additionally retains both structured `data`
and flattened strings (`snowball.rs`, lines 1052–1075). Removing only the
flattened representation in a separate 10,000-record fixture released
17,050,000 bytes. In the schema-aware bridge policy, fields are derived from
structured `data` again, so eager flattening duplicates work and storage.

The 512-byte field clipping also fails to bound allocated capacity:
`scalar_to_string` clones the complete string before `flatten_sorted` truncates
it (`snowball.rs`, lines 1143–1182). For one 65,536-byte scalar the flattened
string had length 515 (including the ellipsis), but capacity 65,536. The
structured copy also retained 65,536 bytes. The probe measured 131,492 retained
bytes and 263,304 peak extra bytes for that parsed record, returning to baseline
on drop.

Repair direction: enumerate/check vector presence before parsing payloads;
select a bounded batch and read only those records. Retain a reliable recovery
scan for missed notifications and out-of-order/missing vectors. Build flattened
fields lazily for consumers that use them; copy only a UTF-8-safe bounded slice.
Preserve all authoritative timing data, schema privacy filtering, and Btrfs
member transport. Deleting history is not a memory repair.

## The fast path does not bound the metadata index

[BtrfsCache::new](src/btrfs_cache.rs), lines 425–482, opens an independent
in-memory Cozo database, then applies memory limits only to `MemoryCache`.
[CozoGraphShuttle::new_in_memory](../op-cozo-store/src/lib.rs), line 217,
selects the `mem` storage engine. Every unique embedding stores its complete
original text (`btrfs_cache.rs`, lines 780–792); every KV row stores its complete
key (lines 920–931). Fast-path eviction leaves those index rows untouched.

Consequently even a disabled fast path retains metadata proportional to all
inserted texts/keys. Rows remain reachable until explicit cleanup/delete/clear
or cache destruction. This is unbounded lifetime retention, not evidence of
lost pointers. KV rows have no age cleanup; embedding age cleanup has no
production caller in this checkout.

With both fast-path limits set to zero, the `index` probe measured 8,437,140
extra Rust bytes after 128 distinct texts and 16,873,998 after 256. Each text
contained 65,536 payload characters plus its distinguishing prefix; each vector
contained one float. Clearing embeddings reduced the extra allocation to
276 bytes. This confirms metadata ownership outside the disabled fast path.

Repair direction: use a compact index with a separately enforced budget or an
appropriate persistent Cozo owner, and avoid retaining full source text solely
for a hash-to-file lookup. Keep category subvolumes and their transport intact.

## Raw keys bypass the fast path byte limit

`MemoryCache::put` counts only `data.len()` (`btrfs_cache.rs`, lines 204–224).
The `HashMap` owns key strings, and `put_raw` stores `format!("kv:{}", key)`
(line 937). Key length is unrestricted. The entry-count cap therefore cannot
bound key memory; even empty values can retain arbitrarily large individual
keys. The Cozo index independently stores another copy.

With limits of 32 entries and 32 payload bytes, the `keys` probe inserted 32
keys containing 262,144 payload characters each and one-byte values. It retained
16,794,192 extra Rust bytes. Clearing only the fast path freed 8,389,024 bytes,
leaving 8,405,168 bytes with the KV index intact. Clearing all data reduced the
extra allocation to 4,176 bytes. The freed fast-path allocation is dominated
by keys; it also includes values and per-entry overhead.

Repair direction: use fixed-size hashed fast-path keys and account for retained
allocation capacity and container overhead when defining a memory budget.
Avoid allocating/cloning an inadmissibly large value before checking admission.

## Embedding expiry leaves values in the fast path

`cleanup_old` deletes selected vector files and Cozo rows but never removes
`emb:<hash>` from `MemoryCache` (`btrfs_cache.rs`, lines 1145–1204).
`get_embedding` checks memory first and ignores access-update failure
(lines 675–680). An expired vector can therefore remain readable until LRU
eviction, explicit clear, or cache destruction. Retention is bounded by the
fast-path configuration, but expiry semantics are wrong.

The `cleanup` probe puts eight vectors into the cache, calls `cleanup_old(-1)`
to deterministically select fresh fixture rows using a future cutoff, verifies
that all eight backing files are gone, and reports whether the value is still
readable. It then clears the fast path and verifies a miss. The negative age is
a fixture shortcut; the defect is the same for genuinely old rows.

Observed: `removed_rows=8 remaining_files=0 hit_after_cleanup=true`.
Extra Rust allocations fell only from 2,101,768 to 2,098,816 bytes after expiry.
Explicitly clearing the fast path reduced that to 1,056 bytes and the lookup
missed. Thus the fixture reproduces both retained payloads and stale reads.

Repair direction: evict each selected embedding from the fast path as part of
successful expiry, including failure/concurrency handling, then add a regression
that checks index, file, and fast-path visibility together.

## Reachability and other ownership checks

The only non-test `BtrfsCache::new` caller is
[OptimizedSnowball](../op-snowball/src/btrfs_numa_integration.rs), line 117.
Its active cache writer serializes block JSON directly to `blocks/by-hash`
(lines 155–204); it bypasses the embedding/KV index and `MemoryCache` APIs.
Thus those API defects should be repaired, but cannot presently be assigned as
the cause of bridge RSS growth from source evidence alone.

`SnapshotManager` retains paths/configuration and a mutex, not snapshot
contents in memory. Snapshot retention is on disk, with per-owner policy;
historical/incomplete sets intentionally have different cleanup rules. NUMA
helpers retain bounded hardware topology and counters. No reference cycle,
forgotten allocation, or accumulating NUMA mapping was found in those owners.
The send/receive helper copies a stream rather than buffering the whole stream.

Additional source risks, not measured live causes: the Snowball writer queues
owned closures through an unbounded `std::sync::mpsc` channel; a persistently
slower writer could accumulate payloads under concurrent producers.
`EventChain.events` also keeps appended events in memory without a lifetime
cap (`op-state-store/src/event_chain.rs`, lines 550–558 and 607–618); the audit
startup replay limit does not bound future appends. That ledger is a separate
owner and must not be discarded as cache cleanup.

## Reproduction and verification

Run from repository root with the native GNU toolchain. `/usr/bin/cargo` avoids
this host's login-shell dispatcher function. Build jobs are deliberately serial.

```sh
CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CXXFLAGS='-include cstdint' \
  /usr/bin/cargo build -p op-cache --example cache_memory_probe
OPDBUS_CACHE_MEMORY_MAX_ENTRIES=0 OPDBUS_CACHE_MEMORY_MAX_BYTES=0 \
  target/debug/examples/cache_memory_probe index
OPDBUS_CACHE_MEMORY_MAX_ENTRIES=32 OPDBUS_CACHE_MEMORY_MAX_BYTES=32 \
  target/debug/examples/cache_memory_probe keys
OPDBUS_CACHE_MEMORY_MAX_ENTRIES=8 OPDBUS_CACHE_MEMORY_MAX_BYTES=4194304 \
  target/debug/examples/cache_memory_probe cleanup
sh scripts/probes/cache-memory-parser.sh
```

The normal Cargo example build was attempted but intentionally interrupted
(exit 130) when it required a fresh serial RocksDB build. The successful native
GNU fallback build and all three probe processes exited 0. To reproduce that
fallback on the recorded artifact set:

```sh
OPDBUS_CACHE_MEMORY_ARTIFACT_RECEIPT=/var/tmp/opdbus-cache-memory-review/api-wrapper.bL9tH1 \
  bash scripts/probes/cache-memory-api.sh
```

The compatible `serde_json` artifact matters: selecting another feature build
causes a Rust type mismatch with the prebuilt Cozo wrapper. Full selected paths
and hashes are in
`/var/tmp/opdbus-cache-memory-review/api-wrapper.bL9tH1/provenance.txt`;
`index.log`, `keys.log`, and `cleanup.log` there contain the exact outputs.
The final receipt-based script replay also passed all three cases; its logs and
provenance are in `/var/tmp/opdbus-cache-memory-review/api-wrapper.Pm6QXY/`.
Parser rerun receipts including the post-filter diagnostic are in
`/var/tmp/opdbus-snowball-parser.mDX0Cd/`. Older equivalent parser results are
also retained under the main investigation evidence directory.

The wrapper receipt selects existing artifact filenames and records their
current hashes; it cannot replace missing build artifacts. Use the normal Cargo
build on a fresh checkout. The parser script deliberately refuses a changed
Snowball source hash until its extraction ranges have been reviewed.

The probes use synthetic data and do not call external providers. Parser output
records both live allocation bytes and RSS; cache output records live allocation
bytes. Neither is a native Btrfs correctness test or a production heap profile.

`/verify` was invoked through `scripts/check-workspace.sh verify` with
`PATH=/usr/bin:/bin`, jobs 1, incremental compilation disabled, and the required
C++ include flag. It exited 1 at existing workspace formatting drift, before
Clippy or workspace tests. Existing files were not reformatted. Full workspace
verification is therefore not passing.
