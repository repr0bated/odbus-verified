# Snowball bounded projection implementation

Production changes are in `crates/op-snowball/src/snowball.rs` and `crates/op-cognitive-mcp/src/chain_vectors.rs`. Authoritative timing records, schemas, privacy policy, vector transport and public `ChainBlockRef.data_fields`/`field` shape remain preserved.

`project_pending` now uses `pending_blocks(limit)`: a blocking filename/vector-presence scan retains only the oldest `limit` candidate paths in a heap before any selected timing JSON is read. Gaps below watermarks are included. Malformed selected JSON is warned/skipped as before; another bounded scan resumes after those filenames and fills the remaining batch, so repeated malformed old records cannot starve valid later work. Directory/selected-read/vector-inspection I/O failures propagate. Limit zero parses no payloads but still reports vector presence. `already_present` now explicitly counts timing filenames with a vector, including already-projected malformed historical JSON that no longer needs parsing. `None` remains the caller's explicit unbounded limit. Multiple refill scans can cost extra metadata traversal for extensively malformed history.

Parsed structured data is moved out of the JSON object instead of deep-cloned. Flattened long strings allocate only a UTF-8-safe prefix plus ellipsis, at most 515 bytes; they no longer retain original scalar capacity or panic when byte 512 splits a character. Rendering builds one output string rather than a vector of duplicated lines. Eager flattened fields remain for public API compatibility, but are now created only for the selected pending batch on the automatic path.

The writer now admits at most 64 queued closures through Tokio bounded mpsc, yielding asynchronously while full. Its dedicated thread uses `blocking_recv`, drains admitted jobs on sender drop, and catches job panics as before. Cancellation before admission submits nothing; already admitted jobs finish even if the caller drops its reply. The permit is acquired before boxed-job/oneshot construction. This bounds queue entries, not total bytes: callers can still retain original arguments/serialized payloads while awaiting admission, and individual jobs have no newly imposed size limit. No authoritative write is dropped to impose history retention.

Focused exact-source Rust compilation passed 10 new regressions (six selection/error/retry cases, three writer admission/drain/panic cases, one UTF-8/capacity case), plus three existing parser/render/capping tests. Logs are in `raw/`. Native Btrfs tests and full Cargo/workspace gates are owned by the parent; this source wrapper is not a complete dependency rebuild. An initial wrapper build hit incompatible prebuilt simd-json types; compiling current footprint/retention/snapshot source directly resolved it, preserving the first failure log.

Three independent before/after processes profiled 10,000 identical already-vectorized synthetic timing records on tmpfs, with a 256-item pass limit:

| Metric | Before | After |
|---|---:|---:|
| Requested Rust allocation peak above baseline | 38,432,513 bytes | 2,417 bytes |
| Live bytes with all historical records parsed | 36,911,906 bytes | No historical payloads parsed |
| Live bytes with empty pending vector retained | 2,492,226 bytes | 1,115 bytes |
| Elapsed scan/filter time across repeats | 1.992–2.148 s | 0.0263–0.0289 s |
| RSS at pending checkpoint | 58,672–58,948 KiB | 9,812–9,840 KiB |

This exercises actual captured `StreamingSnowball::blocks` versus current `pending_blocks`, not a substitute parser. Appended profiling instrumentation constructs the same object fields directly on disposable ordinary directories to avoid native-Btrfs constructor/setup costs. Both versions share the same dependency artifacts and fixture format; no Voyage/Qdrant/network call is made. Rust requested bytes do not include direct native malloc or allocator capacity, and the instrumented GNU debug-wrapper timing is diagnostic rather than a production benchmark. This establishes sharply reduced allocation and scan CPU for an already-projected history, not attribution or remediation of all live bridge RSS.

Source hashes, preserved baseline, current source capture, wrappers and exact command scripts remain in this directory. Every profiling fixture removed its owned tmpfs directory. Parent owns full verification, handoff and any subsequent release; no deployment occurred.
