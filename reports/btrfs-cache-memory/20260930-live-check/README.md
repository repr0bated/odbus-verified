# Live bridge memory check — 2026-09-30

Five read-only observations of `op-grpc-bridge`, PID 1123, with unchanged
`/proc/1123/stat` starttime 601 ticks. The first four samples were collected by
the delegated measurement agent; the primary collected the fifth. Raw samples,
commands and tool-calculated figures are retained alongside this report.

| UTC | RSS (KiB) | Process swap (KiB) | Sum (KiB) |
| --- | ---: | ---: | ---: |
| 17:30:57 | 13,242,664 | 4,730,852 | 17,973,516 |
| 17:31:38 | 13,243,660 | 4,729,856 | 17,973,516 |
| 17:31:58 | 13,243,660 | 4,729,856 | 17,973,516 |
| 17:32:19 | 13,443,008 | 4,530,612 | 17,973,620 |
| 17:42:01 | 13,488,848 | 4,484,432 | 17,973,280 |

Over 11 minutes 4 seconds, RSS plus process swap fell 236 KiB (0.230 MiB).
The initial 82-second sample rose only 104 KiB (0.102 MiB). Changes in resident
RAM were almost entirely offset by process swap. VmData (18,489,680 KiB),
VmSize (19,070,588 KiB), VmHWM (17,332,276 KiB) and thread count (52) were
unchanged. Concurrent Cargo compilation changed host memory pressure.

This supports a short-term plateau of the measured process. It does not prove
that the application freed allocations, establish a whole-day trend, or
identify which change caused the plateau. RSS plus swap is a coarse process
footprint, not a heap-allocation measurement. No debugger, allocation profiler,
memory dump, cache flush or service action was used. The first sample's
smaps extraction failed; its status/stat/free/vmstat readings remain usable.
The fifth sample did not include smaps.

A later pre-cutover sample at 20:28:06 UTC still has PID 1123/starttime 601:
RSS 15,215,380 KiB plus swap 2,677,428 KiB equals 17,892,808 KiB. This is 80,472 KiB
(78.586 MiB) below the 17:42:01 sum over 2h46m05s. VmData is 18,397,576 KiB,
VmSize 18,946,400 KiB and threads 52. The raw record is
`pre-cutover-late-sample.txt`. The lower aggregate supports absence of a
monotonic process-footprint climb across these endpoints; it still does not
identify allocator reclamation or prove the later source fixes caused it.

The next sample at 21:03:21 UTC, still PID 1123/starttime 601, reverses that
decline: RSS 12,586,180 KiB plus swap 5,626,120 KiB totals 18,212,300 KiB.
That is 312.004 MiB above 20:28 and 233.418 MiB above 17:42. VmData is 18,730,660 KiB,
VmSize 19,270,744 KiB and threads 52. The ongoing release build changes host
pressure, but pressure alone is not evidence explaining the increased sum.
The raw record is `pre-activation-sample.txt`. These observations show a
variable process footprint with a later increase, not monotonic reclamation
or proof that the full leak investigation is resolved.

## Running executable provenance

At 17:32 UTC, `/proc/1123/exe` resolved to
`/opt/op-dbus/golden/bin/op-grpc-bridge`. Its SHA-256 was:

```text
7582eab8f94ee7a3dfdff888721d16bb41f258e9248d9d8bbdbb9648dd279255
```

That digest matches the golden manifest's bridge entry. The manifest names
commit `9dd1156` and build `20260930T013207Z`; the user reports several golden
deployments. A manifest commit label alone cannot identify uncommitted source
that may have been compiled, so an earlier blanket statement that the memory
fix was not deployed was too strong and is withdrawn.

Read-only ELF inspection found the earlier `OPDBUS_CACHE_MEMORY_MAX_ENTRIES`
and `OPDBUS_CACHE_MEMORY_MAX_BYTES` configuration markers and the linked
`BtrfsCache::new` implementation. These limits already exist in source at
`9dd1156`. It did not find the new `OPDBUS_CACHE_INDEX_MAX_ENTRIES`,
`OPDBUS_CACHE_MAX_PAYLOAD_BYTES`, `OPDBUS_CACHE_MAX_EMBEDDING_DIMENSIONS` or
`OPDBUS_CACHE_TRANSFER_TIMEOUT_SECS` markers from the later cache repair.
The measurements therefore must not be attributed to that complete later
repair without stronger build provenance. No deployment was performed by
this measurement pass.

ELF checks used `sha256sum`, `nm --defined-only` and `strings` with explicit
symbol/marker filters. The installed `nm -C` did not demangle these Rust v0
symbols; a namespace-formatted grep was not a valid absence check.

## New release baseline

At 22:14:30 UTC, after deliberate release activation, the bridge is a new
process: PID28384/starttime5062684. Its executable SHA256 is
`8b828b2088635907c5fdb7a1e4a5377226828f8178fc6ca74e4a7564dd5cb9aa`,
from received release `20260930T212601Z-identity-46c8a19`. RSS is1,566,040KiB,
process swap0, VmData1,677,268KiB and threads46. Source/release/runtime graph
imports and scoped identity checks preceded this baseline. The raw evidence
is `post-release-baseline.txt`. Comparing this new process with PID1123 cannot
establish allocator reclamation or sustained leak resolution; subsequent
same-process measurements are required.
