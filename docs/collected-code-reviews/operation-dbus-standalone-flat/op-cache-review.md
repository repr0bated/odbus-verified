# op-cache Code Review

## Summary

Deep review comparing SPEC.md against actual implementation. The spec is auto-generated metadata with no actual specification. The crate has sound core ideas but critical bugs, massive duplication, and scope creep.

---

## Critical Issues

### Cache invalidation is a no-op
`grpc/cache_service.rs:275-301` — `invalidate_workstack` tries prefix matching on keys, but keys are SHA-256 hashes. The prefix check never matches. Invalidation silently does nothing.

**Fix:** Maintain a `workstack_id -> Vec<cache_key>` reverse index.

### Shell injection in BTRFS operations
`btrfs_cache.rs:644-685` — `stream_to_remote`/`receive_from_remote` interpolate `remote_host` and `remote_path` directly into `bash -c` strings.

**Fix:** Use `Command::new("btrfs").arg("send")` piped to `Command::new("ssh")` without shell.

---

## High — Massive Duplication

### WorkflowCache == WorkstackCache
`workflow_cache.rs` (653 lines) and `workstack_cache.rs` (499 lines) are structurally identical. Same SQLite + filesystem + zstd pattern. Only difference is column names (`workflow_id` vs `workstack_id`).

**Fix:** Delete `workflow_cache.rs`, parameterize `workstack_cache.rs`.

### PatternTracker == WorkflowTracker
`pattern_tracker.rs` (427 lines) and `workflow_tracker.rs` (741 lines) both track agent sequences, count calls, suggest promotions. `WorkflowTracker` is a superset.

**Fix:** Delete `pattern_tracker.rs`, use `workflow_tracker.rs` everywhere.

### gRPC layer reimplements the library layer
`AgentServiceImpl` doesn't use `AgentRegistry`. `CacheServiceImpl` doesn't use `WorkstackCache`. `OrchestratorServiceImpl` doesn't use `Orchestrator`. They're completely independent implementations with different bugs and slightly different formulas.

**Fix:** gRPC services should wrap library modules, not reimplement them.

---

## Medium Issues

| File | Issue | Fix |
|------|-------|-----|
| `btrfs_cache.rs`, `snapshot_manager.rs` | Mixed `log` and `tracing` | Replace `log::` with `tracing::`, remove `log` dep |
| `pattern_tracker.rs:246`, `workflow_tracker.rs:400,443,473` | Unnecessary `unsafe` for simd_json on small JSON arrays | Use `serde_json::from_str` |
| `btrfs_cache.rs:752-804` | `apply_cpu_affinity` sets affinity on a child `echo` process that exits immediately — no-op | Use `libc::sched_setaffinity` or remove |
| All mutex usage | `.unwrap()` on poisoned mutex = process crash | Use `.unwrap_or_else(\|e\| e.into_inner())` or return error |
| Proto lines 505-540 | `McpService` defined but unimplemented | Implement or remove |
| `capability_resolver.rs:313` | TODO: topological sort for agent deps never done | Implement or document limitation |
| `agent_registry.rs:59` | `from_str` is inherent method, not `std::str::FromStr` | Implement the standard trait |
| `numa.rs:2` | `#![allow(dead_code)]` blanket suppression | Remove and fix actual dead code |

---

## Architecture Issues

### The crate does too many things
Currently implements: agent registry, capability resolver, orchestrator, two caching systems, pattern detection, BTRFS snapshots, NUMA detection, embedding caching, three gRPC services, an MCP proto definition.

Should be split into:
- `op-agent-registry` — agent definitions, capability resolution
- `op-cache` — step/embedding caching, BTRFS, compression
- `op-orchestrator` — workflow execution, pattern tracking, promotion
- `op-numa` — small utility (or inline into op-cache)

### No internal workspace dependencies
Cargo.toml has zero internal deps. Proto overlaps with concepts in other crates. Suggests copy-paste evolution.

---

## What's Good

- **NUMA topology detection** (`numa.rs`) — solid `/sys` filesystem parsing, clean fallbacks, good tests
- **Agent builder pattern** (`agent_registry.rs:180-261`) — clean fluent API with capability deduplication
- **Cache key generation** — SHA-256 of `workstack_id:step_index:input_hash` avoids collisions
- **Compression heuristic** (`workstack_cache.rs:169-178`) — skip if <1KB or compressed > original
- **gRPC streaming** (`orchestrator_service.rs:430-547`) — proper error handling, clean loop break on send failure
- **Test coverage** — meaningful tests with `tempfile::TempDir`, good edge case coverage in capability resolver

---

## SPEC.md Assessment

The spec contains only auto-extracted metadata (file listing, deps, module names). No specification of:
- Problems solved or use cases
- Architecture or data flow
- Relationships between components
- API contracts or behavioral guarantees
- Caching strategy details
- NUMA integration purpose

Needs complete rewrite.

---

*Generated 2026-02-16 from full crate review*
