# Btrfs cache, NUMA and timing

The owner reaffirmed this architecture on 2026-09-23. It follows the original
`operation-dbus-proto` design in `branding/product-baseline.md` §12 and
`crates/op-snowball/{DESIGN,REQUIREMENTS}.md`, especially requirement R5.
The later global five-subvolume cap and directory-conversion rule are withdrawn.

## Storage ownership

The cache has a Btrfs root and five category subvolumes:

```text
cache/                         Btrfs subvolume
  embeddings/                  Btrfs subvolume
  blocks/                      Btrfs subvolume
  queries/                     Btrfs subvolume
  diffs/                       Btrfs subvolume
  kv/                          Btrfs subvolume
```

Reopening the cache preserves those subvolumes. An existing ordinary directory
on Btrfs is not silently converted, deleted or accepted as the intended
subvolume. The optional ordinary-filesystem cache path does not provide Btrfs
snapshot or transport semantics.

Snowball separately owns the authoritative append-only timing stream and its
vector/state projections. Its timing, vector and state paths are Btrfs
subvolumes. Cache contents are rebuildable; they do not replace the timing
record, canonical event chain, identity credential or grants.

## Snapshot and transfer contract

A Btrfs snapshot of the cache root does not include its nested subvolume data.
Each cache snapshot set therefore contains six read-only snapshots: `root` and
one for every category. A completion marker is published only after every
member is verified. Send/receive transfers each member explicitly. A partial
set is not advertised as complete. Historical parent-only snapshots remain
preserved and are excluded from automatic rotation.

Retention counts complete cache sets. Its default is 24, configurable with
`OPDBUS_MAX_CACHE_SNAPSHOTS`; zero is invalid. It is not a global subvolume cap.
Cache members are snapshotted sequentially, so complete coverage does not mean
one atomic instant across concurrent cache writers. Snowball's authoritative
timing/vector/state set uses its shared writer lock for consistent construction.

## NUMA placement

`OptimizedSnowball` detects topology and requests CPU affinity and preferred
memory placement on a dedicated writer thread. Timing, vectors and mirrored
cache blocks use that writer; Tokio worker threads are not pinned by this path.
CPU affinity, memory-policy acceptance and actual memory locality are distinct.
Neither a successful constructor nor a Btrfs subvolume proves physical NUMA
placement. Verify the live writer's CPU mask and report memory-policy failures
separately. Placement failures must not erase or weaken durable timing records.

## Verification

Native fixture tests must check reopen, child data in snapshots, read-only
members, actual send/receive, preservation of historical sets and category roots,
and the real writer's affinity after a write. Send/receive tests need the
necessary Btrfs privileges and an isolated fixture. Passing parser or ordinary
filesystem tests alone does not establish that this path works on the host.
