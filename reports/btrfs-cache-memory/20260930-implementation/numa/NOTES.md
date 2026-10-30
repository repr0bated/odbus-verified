# NUMA implementation verification

Production changes are limited to `crates/op-cache/src/numa.rs`, NUMA writer selection in `crates/op-snowball/src/btrfs_numa_integration.rs`, and the constructor/selection/test sections of `crates/op-cache/src/btrfs_cache.rs`. Transport integration is the parent agent's work.

## Behavior

- NUMA nodemasks are dynamically sized machine-word arrays, bounded against oversized configured IDs and checked against the calling thread's allowed memory nodes. Bind/Interleave now pass highest selected node + 2, matching the observed kernel boundary and Preferred. Default uses a null nodemask. Empty/invalid masks fail explicitly.
- CPU affinity reads dynamically sized masks, intersects requested CPUs with the current allowed mask, rejects empty intersections, and compares actual kernel readback. No fixed cpu_set_t capacity indexing remains.
- `NumaOptimizer::configure_command` prepares all masks in the parent. The child pre-exec hook performs only affinity/memory-policy syscalls and preallocated affinity readback. It performs no filesystem reads, logging, heap allocation or locking. Kernel rejection fails spawn. Parent affinity and policy are unchanged. Explicit placement returns unsupported on non-Linux.
- Current-node detection uses sched_getcpu instead of whitespace-splitting /proc/self/stat. Both system and node meminfo parse the numeric field after the colon. Fallback CPU IDs come from actual allowed affinity; unreadable memory becomes unknown/zero rather than invented capacities.
- Topology refresh redetects hotplug nodes and memory. Sparse node IDs are sorted; round-robin uses real IDs; MostMemory compares current free memory with a deterministic node-ID tie break. Unknown node preferences fall back to actual/local selection.
- Unset OPDBUS_NUMA_STRATEGY defaults to LocalNode, matching the enum default and preserving the original writer's default pin. Explicit disabled now prevents writer placement instead of falling back and pinning anyway.
- The dedicated Snowball writer selects the highest allowed CPU within the selected node, with a checked override. A failed preferred memory policy reports that the successful CPU pin remains active.
- Cache transport placement returns no node for Disabled, samples actual current node for LocalNode, filters permitted CPUs, refreshes topology/free memory for MostMemory and uses stable IDs for round-robin. Constructor metadata records permitted initial placement; it does not prove a runtime worker is pinned.

## Evidence

`run-tests.sh` compiles the exact current production NUMA file with pinned GNU dependencies, without Cargo/shared builds. `tests.log`: 16 passed, 0 failed, 2 ignored. Real Bind([0]) and Interleave([0]) tests are active and verify kernel mode/mask on short-lived threads. Pure tests cover machine-word boundaries, invalid/empty IDs, malformed ranges, sparse strategies, allowed CPU intersections and system/node memory parsing. The child test verifies actual child affinity and unchanged parent affinity/memory policy.

The initial fixture used an excessive get_mempolicy readback maxnode and failed EINVAL; `tests-initial-query-fixture-error.log` preserves that failure. The fixture now sizes readback from actual possible nodes. Production apply calls were already succeeding; this was a readback fixture error.

`run-native-policy.sh` compiles the exact production NUMA file and spawns only its own probe children. `native-policy.log`: 12 successful children (3 repetitions of Default, Preferred(0), Bind([0]), Interleave([0])). Every child reports the selected single CPU and correct mode/mask; parent CPU/memory policy remains unchanged after every child.

Final build sampled group RSS: unit222068KiB, native273768KiB; limits1GiB compilation/512MiB probes, timeouts180s compilation/120s probes. No kernel/global tuning, host service, catalog, live cache or deployment changes were made.

rustfmt checks and git diff whitespace checks pass for numa.rs and btrfs_numa_integration.rs. Cache placement and full Snowball integration tests are handed to the parent's normal-source verification; this standalone wrapper does not compile those modules. Multi-node execution, hotplug races and non-Linux cross compilation remain unverified.
