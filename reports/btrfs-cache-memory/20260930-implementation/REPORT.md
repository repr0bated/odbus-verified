# Btrfs cache, process and projection repairs

Date: 2026-09-30. Repository `/srv/git/odbus-verified`, branch
`claude/amazing-carson-8bg4yr`, baseline commit
`abefbe435f91e4c719fabcea4e07383c5c146cfc`. Changes are in the working tree;
no service restart, binary installation, catalog reseal, push or deployment.
Earlier baseline evidence is preserved in `../20260930-followup/REPORT.md` and
`crates/op-cache/MEMORY_INVESTIGATION.md`.

## Findings and implemented repairs

The investigation reproduced reachable retention and transient amplification,
plus incorrect resource ownership. It did not establish an unreachable malloc
leak or attribute the complete running bridge RSS to these paths.

- **Cache ownership/bounds:** SHA-256 fast-path keys; conservative accounting
  for keys, vector capacity and map nodes; eviction and insertion under one
  lock; no copying of entries that cannot fit. Derived Cozo rows are compact
  and bounded across embedding/KV relations (default 4096), without retaining
  full source texts/keys. Metadata eviction does not delete payload files.
- **Cache correctness:** deterministic file lookup survives metadata eviction
  and reopening; mtime records durable access; inode/size/mtime validation
  rejects stale fast-path hits across handles. Mutex plus filesystem flock
  serialize file/index/fast-path transactions. Payloads are written to unique
  temporary files and atomically renamed. Expiry scans actual vector files in
  batches of 128 and invalidates memory and metadata after deletion. Errors
  propagate. Stats derive cardinality from disk, independently of index caps.
- **Allocation limits:** raw payloads default to 64 MiB, embeddings to 4 million
  dimensions. Length/format checks precede decoding allocation. Environment
  overrides are explicit; invalid values fail construction. Compute callbacks
  run outside cache locks; publication rechecks concurrent writes.
- **Process ownership:** 64 KiB captured-output caps, 60-second administrative
  command deadlines, concurrent draining and child status checking. Stream
  copying uses fixed-size Tokio buffering without a detached copier. Failure,
  timeout, cancellation and second-child spawn failure kill/reap owned children.
- **Ephemeral transport:** two admitted transfers per process; unique temporary
  six-member sets never enter manual rotation. Owned creation finishes safely
  after caller cancellation and cleans its result. Source sets and partial
  local receives clean up on cancellation; successful destination data stays.
  Cleanup refuses unexpected objects, retries briefly and disables further
  admission after persistent cleanup failure. Manual retention remains 24.
- **NUMA:** validated dynamic masks, correct kernel maxnode, allowed CPU/node
  intersections, actual current CPU/node, deterministic sparse-node selection,
  refreshed available-memory choice, honest disabled behavior. Pre-exec
  placement applies to actual children, without changing Tokio workers. The
  Snowball writer remains a dedicated thread; explicit disabled is honored.
  A default-policy reset tolerates only EPERM/EACCES/ENOSYS in restricted
  children; explicit placement policies still fail rather than claiming success.
  A child-only seccomp fixture verifies this distinction without host tuning.
- **Snowball:** select pending filenames before parsing payloads, bound the
  automatic batch to its requested size, include gaps, refill after malformed
  records, preserve I/O errors. Move parsed data instead of cloning; clip long
  scalars without retaining their old capacity or splitting UTF-8. The writer
  queue admits 64 jobs, yields while full and drains admitted writes.

## Measured results

| Controlled fixture | Before | After |
|---|---:|---:|
| 10,000 already-vectorized records: peak requested Rust allocation during pending scan | 38,432,513 B | 2,417 B |
| Same fixture: retained bytes with empty pending result | 2,492,226 B | 1,115 B |
| Same fixture: elapsed scan, three independent processes | 1.992–2.148 s | 26.3–28.9 ms |
| 256 large-text metadata rows, fast path disabled | 16,873,998 B | 134,912 B |
| 32 large keys / 32 payload bytes | 16,794,192 B | 17,184 B |
| Eight expired vectors: retained requested bytes | 2,098,816 B | 1,760 B |
| Eight expired embeddings: stale fast-path lookup | readable | absent |
| Warm 256-byte reads: median five runs | 2,116 μs/read | 1,342 μs/read |
| Warm 64 KiB reads: median five runs | 2,153 μs/read | 1,348 μs/read |
| Cancelled stalled local stream: surviving children in three trials | two | zero |

See `snowball/RESULTS.md`, `cache-validation/`, `numa/NOTES.md` and
`transport/native-{1,2,3}.log` for final raw results and qualifications.
Requested Rust allocation is not allocator-resident, native malloc or kernel
memory. Debug-wrapper latency is diagnostic, not a production capacity claim.

Nine complete real Btrfs local transfers (three independent processes, three
cycles each) verified all six read-only members and embedding/category content.
Every source temporary set was removed; destination content survived until
explicit fixture teardown. Cancelled snapshot creation and a partially received
set also cleaned up in each process. In the first run, post-cycle live Rust bytes
were exactly 341,533 for all three drains, with 10 descriptors and zero children.

## eBPF evidence

Actual malloc/free uprobes, CPU perf-event sampling and kernel `kmalloc`
tracepoints attached successfully to disposable probe children using bpftrace
0.27. This supersedes the older handoff claiming uprobes were disabled.
`verification/trace-cache.bt` uses scalar aggregates and a 128-key map cap.
Tracefs was mounted only inside a private mount namespace. No persistent BPF
program, host tuning, network policy or production-process attachment was added.

The fixed warm probe recorded 402,737 malloc calls, 410,872 free calls,
161,389,153 requested malloc bytes, 427 kernel allocation events / 208,184 bytes,
and 305 CPU samples. These are allocation turnover counters, **not live-memory
or leak counts**. Baseline and fixed eBPF runs use different workloads; their
counters must not be interpreted as a before/after speed or memory comparison.
Instrumentation significantly slows malloc-heavy tests; use uninstrumented
paired measurements for latency conclusions.

## Verification and limits

Normal GNU `cargo check --locked -p op-cache --lib` passed. Normal Cargo library
tests passed **92 cache tests / 3 ignored**, and **56 Snowball tests / 2 ignored**.
Snowball first ran against tmpfs and its 13 Btrfs-dependent fixtures correctly
failed filesystem creation; rerunning on disposable native Btrfs fixtures passed
all 56. The writer pin test also passed after reusing explicit Btrfs fixture
cleanup. Combined exact-source
cache/process/snapshot/NUMA tests passed 52 tests, zero failures, three explicit
ignores. Snowball selection/queue/parser tests passed 13. Native child NUMA policy
readback passed 12/12 cases (three repetitions of four policies); parent CPU and
memory policy stayed unchanged. The available host has one NUMA node; real
multi-node performance and page migration were not validated.

The repository `/verify` command was attempted again on the final owned source
and stopped at existing formatting drift in 104 other source files, before
Clippy/tests. All ten owned Rust files pass their formatting check, and
`git diff --check` passes. The nine model-registry dispatch tests also pass in
the normal GNU Cargo harness. Changed-target Clippy with dependencies stops at
two pre-existing `collapsible_if` warnings in `op-cozo-store`; this is not a
passing workspace gate. Full target Cargo tests and changed-target
checks are recorded separately in `verification/`; consult their final results.
An initial test build repeated the same native RocksDB compilation and was
stopped; subsequent tests reuse the GNU static archive freshly built by the
successful check, via cozorocks' supported `ROCKSDB_LIB_DIR`/`ROCKSDB_STATIC`
configuration. All other normal Cargo test compilation proceeds normally.

Filesystem freshness requires stat/touch/flock work on reads and serializes
cache operations, including synchronous work exposed through existing async
signatures. Paired synthetic warm reads improved by 36–37% after removing a redundant Cozo query; disk-workload tradeoffs remain workload dependent. Caller-held
compute results and blocked admissions are not a global process byte budget.
The 64-job Snowball queue bounds entries, not arbitrary caller payload sizes;
explicit unbounded pending queries remain available to existing API callers.

Transport timeout is per member (default 300 seconds), with separate bounded
administrative commands. No SSH destination was exercised in the native local
fixture. Failed remote destinations are left uncommitted for inspection; remote
orphan reconciliation and crash/runtime-shutdown cleanup remain limitations.
A runtime must remain alive to complete asynchronous guard cleanup. Process
supervision covers direct Btrfs/SSH children, not arbitrary grandchildren.

No speculative allocator replacement, global cache dropping, swap change, THP,
IRQ pinning, XDP/AF_XDP, network namespace or packet-policy modification was
needed for the reproduced mechanisms. These were not claimed as tested fixes.

## Rollback

The patch adds no stored-format migration. Preserve payloads/history and reverse
only the listed source changes in an isolated checkout to return to the prior
implementation. Configuration can disable the fast path/index with zero entry
limits or disable placement; this is a tuning choice, not a repair for leaked
ownership. Never reset the shared working tree or delete real snapshots. Build,
release snapshot preparation, activation and monitoring of deployed RSS remain
separate work; this report does not claim production remediation.
