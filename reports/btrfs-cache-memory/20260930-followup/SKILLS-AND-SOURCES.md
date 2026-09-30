# Skills and reference coverage

Six actual installed skills were read in full. Four are required by this checkout's AGENTS.md; they are **not** six specialist memory skills.

| Skill | Exact loaded path | Purpose |
|---|---|---|
| grpc-expert | `/srv/git/odbus-verified/.agents/skills/grpc-expert/SKILL.md` | Mandatory repo schema/control-plane preload; precedence over generic gRPC |
| grpc-protocol-expert | `/srv/git/odbus-verified/.agents/skills/grpc-protocol-expert/SKILL.md` | Mandatory transport/protocol preload |
| json-render | `/srv/git/odbus-verified/.agents/skills/json-render/SKILL.md` | Mandatory architecture preload; no UI work |
| ovs-db-analysis | `/srv/git/odbus-verified/.agents/skills/ovs-db-analysis/SKILL.md` | Mandatory preload; no OVS changes |
| debug | `/home/jeremy/.claude/remote/plugins/a3f7d226a8113882/skills/debug/SKILL.md` | Reproduce, isolate, diagnose; evidence before repair |
| testing-strategy | `/home/jeremy/.claude/remote/plugins/a3f7d226a8113882/skills/testing-strategy/SKILL.md` | Focused boundary/lifecycle tests and explicit coverage |

Agents independently loaded mandatory repository skills. Discovery searched the provided skill catalog, local Codex roots, repository skills and installed Claude engineering skills. No specialist Rust-allocation, Linux-memory, Btrfs or NUMA skill was found; no skill was installed. Generic chat-memory skills were not treated as heap-diagnostics skills.

Authoritative references consulted on 2026-09-30:

- [Rust GlobalAlloc](https://doc.rust-lang.org/std/alloc/trait.GlobalAlloc.html): requested Rust allocator calls do not establish all native or kernel memory. Allocation behavior can depend on optimization. These are diagnostic GNU builds, not production release profiles.
- [Linux cgroup v2](https://docs.kernel.org/admin-guide/cgroup-v2.html): distinct anon/file/kernel counters and hierarchical accounting. Here the probe belongs to root cgroup `/`; root counters are host context, not attributable cache memory. `memory.current`, `memory.events` and `memory.pressure` are absent here; `/proc/pressure/memory` also absent.
- [Linux NUMA policy](https://docs.kernel.org/admin-guide/mm/numa_memory_policy.html) and [set_mempolicy](https://man7.org/linux/man-pages/man2/set_mempolicy.2.html): thread policies and restrictions; policy readback is distinct from physical page-placement/performance validation.
- [Linux mempolicy source](https://raw.githubusercontent.com/torvalds/linux/master/mm/mempolicy.c): `get_nodes` decrements `maxnode` before importing the mask. The recorded host tests independently demonstrate the off-by-one failure and corrected behavior; upstream master alone is not proof of the installed kernel implementation.
- [Btrfs subvolumes](https://btrfs.readthedocs.io/en/latest/btrfs-subvolume.html): nested subvolumes require separate snapshots/streams. Fixtures verify all six cache members and full byte equality. Compression policy is inspected; actual compressed extent ratios are not measured.
- [Tokio child processes](https://docs.rs/tokio/latest/tokio/process/struct.Child.html): cancellation does not imply automatic termination. Current pinned source is Tokio 1.53.1 per Cargo.lock; its local `process/mod.rs` and `io/util` were inspected for output accumulation and streaming buffers. The actual source helper cancellation was tested with harmless local children.

Use source evidence in REPORT.md and measured outputs before generalizing any reference. No recommendation assumes Linux page cache is a Rust heap leak or that NUMA policy cures lost ownership.
