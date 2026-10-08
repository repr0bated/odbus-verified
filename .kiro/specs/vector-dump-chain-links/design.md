# Design — vector dump and chain links

## Overview

One spine: **a dump freezes timing, then snapshots the live vectors, then sends
the timing snapshot and the vector snapshot as two streams through the existing
native prepared transfer into a new writable chain root on the replica; on
receive, the same pass that upserts the vector into `snowball_footprints` writes
a chain Cozo relation keyed on the Qdrant point id.** The order is what makes a
block whose vector is still being written stay accounted for on the source: its
timing record is in the frozen timing snapshot, its vector file is not in the
vector snapshot, and that block is not in the vector stream the replica
receives.

```
live vectorization (async)
   │  add_event: timing record block-{n}.json written FIRST (carries the ids)
   │  attach_vector: vec-{n}.bin lands LATER (floats only; may be mid-write at dump)
   ▼
[A] DUMP on the live vectors/timing filesystem (btrfs snapshot cannot cross volumes)
   │  1. snapshot `timing`  → read-only timing snapshot   (FIRST; is the account)
   │  2. snapshot `vectors` → read-only vectors snapshot   (live set, as-is)
   │  a block with a timing record but no vec-{n}.bin is accounted for ON THE
   │  SOURCE timing snapshot; it is not in the vector stream and the replica
   │  never receives it
   ▼
[B] SEND to the replica — two streams, same prepared transfer, into a NEW
    writable chain root <dest>/ on the replica (one root per dump)
   │  timing snapshot first, on_receive unset → received as <dest>/timing
   │  vector snapshot second, on_receive set → received as <dest>/vectors
   │  snapshot basenames are timing and vectors; receive creates
   │  remote_directory/<snapshot name> and will not replace an existing child
   │  (a received vector snapshot IS the vectors tree: vec-*.bin at its root,
   │   no timing/ child — so timing must arrive as its own sibling subvolume)
   │  the previous chain root stays; its timing and vectors are the parents,
   │  found by UUID on the destination filesystem
   │  NOT the retired bash/ssh stream_vectors
   ▼
[C] ON RECEIVE — one pass, hook set only on the second transfer
   │  the hook is given <dest>/vectors and that stream's UUID
   │  it opens the parent of that path: writable chain root <dest>
   │  read-only open of the existing timing/ and vectors/ children
   │  it does NOT call StreamingSnowball::new on <dest> or on either child
   │  (new creates state, snapshots/, and .writer.lock even on a writable root)
   │  watermark is <dest>/.vector-index-block, outside the two children
   │  ingest_received → upsert vectors into Qdrant `snowball_footprints`
   │                    (replica path: no Voyage, FlattenAll, idempotent by point id)
   │  AND, same pass, write the chain Cozo relation keyed on point_id(block_n),
   │  reading every id from the received timing record (never re-derived):
   │  links: conversation_id, turn_id?, event_hash, input_patch_hash, plugin_id,
   │         actor_id, session_id, sealed_id_digest, capability_id, subid?
   │  subid only when the dump carried it (replica has no catalog, FlattenAll)
   │  NOT linked: session_genesis
   │  the relation is NOT system_graph_edges and NOT graph_edge
   ▼
[D] UNCHANGED: retain_snapshots keeps only newest counter; live bridge stays on
    SchemaProjectionPolicy; operator CLI stays on FlattenAll
```

The arrows are the fixed order (tasks follow them). The retired `stream_vectors`
bash/ssh send is never restored (R2.2). `retain_snapshots` is left unchanged —
it keeps the newest counter (R6).

## How the current path actually works (observed, load-bearing)

The design is pinned to what the code already does; the dump slots beside the
existing set snapshot/transfer, it does not replace the inode or transfer model.

### Timing-first is already the write order (origin side)

`snowball.rs` `add_event` writes `block-{n:012}.json` first and only writes
`vec-{n:012}.bin` when the event already carried a vector; `attach_vector` fills
the vector later and refuses a block that has no timing record. `footprint.rs`
`to_block_event` leaves the vector empty. So at any instant the live `vectors`
subvolume can be missing the vector for a block that already has a timing
record, or be mid-write on one. The dump must freeze timing before it freezes
vectors so that this gap is captured as "timing present, vector absent," never
as a lost block (R1.1, R1.4). This is the pre-snowball rule from
`operation-dbus-proto` `blockchain.rs`/`streaming_blockchain.rs`, which the
current `footprint.rs` already cites.

### The aligned-triple snapshot (what the dump is *not*)

`create_snapshot_aligned` takes the writer lock, computes one counter, and calls
`commit_snapshot_set_sync(snapshot_dir, counter, &sources)` where `sources` is
the fixed `[(timing,…),(vectors,…),(state,…)]`. That function snapshots all
three labels into a staging dir, writes a `.committed` marker, and renames to
`SNP-{counter}`. `transfer_snapshot_set` then loops `SNAPSHOT_LABELS` and sends
every member with `on_receive: None`. The doc comment on `create_snapshot` says
this "takes one aligned triple and does not do that [vector] send."

The dump is a different operation: a two-step snapshot (timing then vectors, not
one atomic triple) and two sends, timing first and vectors second, with the
arrival hook on the second send only. It reuses `btrfs_snapshot_readonly` and
the native prepared transfer; it does not reuse the three-label hard-wiring of
`commit_snapshot_set_sync` as the dump's own snapshot step.

### The native prepared transfer (reuse — R2)

`replication.rs` `transfer_prepared_snapshot` is the one-way native path:
`btrfs send` over a pipe into `ssh … btrfs receive -- <dir>`. Preflight requires
a read-only subvolume root with snapshot/received provenance. `validate_parent`
pins the incremental parent to the **same filesystem** and a distinct stream
identity; the kernel `send` ioctl checks the actual incremental relationship.
Receive creates a **new read-only subvolume** at
`remote_directory/<snapshot name>` and finds the parent by UUID there. It does
not replace an existing child of that name. `on_receive` is an explicit remote
executable invoked at the end of that one transfer, with the received
snapshot's absolute path and its stream UUID — never a shell fragment. The dump
sends both members through this exact entry (R2.1). `on_receive` is set only on
the vector transfer, the second one (R2.7).

### The replica ingest (reuse, with one correction — R3)

`chain_vectors.rs` `open_replica` builds a `ChainVectorIndex` with `voyage:
None` and the default `FlattenAll` policy. `ingest_received(from_block)` reads
the vectors present and `upsert_block`s them into `snowball_footprints`.
`upsert_block` writes an **allow-listed** payload whose every value is read from
the **timing record** the block was parsed from (`block.hash`, `block.category`,
`block.field("data.metadata.…")`), never from `vec-{n}.bin`; the comment is
explicit: "never the session genesis." `point_id(block_num) =
uuid::Uuid::new_v5(BLOCK_POINT_NAMESPACE, "block:{n}")`, the namespace fixed
forever, so re-ingest overwrites the same point in place.

Three facts the design must respect:

- `ingest_received` reads `self.chain`, a `StreamingSnowball` opened in `build`
  at `OPDBUS_SNOWBALL_PATH`. `StreamingSnowball::new(root)` expects
  `root/{timing,vectors,state}` subvolumes and a `root/snapshots` directory, and
  it looks for `root/timing/block-{n}.json` and `root/vectors/vec-{n}.bin`. A
  received vector snapshot **is** the `vectors` tree — `vec-*.bin` at its root,
  with no `timing/` child — so pointing `OPDBUS_SNOWBALL_PATH` at the received
  vector snapshot, or opening the index against it, would not find any files.
  The ingest must read a chain root whose `timing/` and `vectors/` children are
  the two received subvolumes.
- `StreamingSnowball::new` is a **writer**: `ensure_source_subvolume` runs
  `btrfs subvolume create` for any missing `timing`/`vectors`/**`state`**,
  `ensure_data_dir` creates `snapshots/`, and it opens a `.writer.lock`. A
  read-only received subvolume cannot take any of those writes, and the dump
  must not create a `state` subvolume on the replica. `new(<dest>)` does that
  too, because the missing `state` child is created on the writable root. The
  hook uses a read-only open of the existing `timing/` and `vectors/` children.
  It does not call `new` on `<dest>` or on either received subvolume.
- The index watermark `.vector-index-block` is written into the chain root
  (`chain_vectors.rs`). For this dump it is `<dest>/.vector-index-block`, on
  the writable root and outside the two received read-only subvolumes.
- Today `on_receive` is `None`, so `ingest_received` is never fired by a receive
  (R3 closes that gap).

### The identity surface — the ids live on the timing record (R4)

The ids the links need are not in `vec-{n}.bin` (raw floats only). They are on
the timing record `block-{n}.json`, which `ChainBlockRef::from_timing_bytes`
parses: `hash`/`event_hash`, `category` (`plugin_id`), `action`, and the
`data.metadata.*` / `data.payload.*` fields (`actor_id`, `session_id`,
`sealed_id_digest`, `capability_id`, `input_patch_hash`, `conversation_id`,
`turn_id`). The Qdrant payload already carries these, and `session_genesis` is
deliberately absent. The chain relation records the same set as links from the
point id, read from the carried timing record and never re-derived. `subid` is
an **origin** product of `SchemaProjectionPolicy` (sealed catalog); the replica
has no catalog and stays on `FlattenAll`, so it links `subid` only when the dump
carried it on the timing record.

- `event_chain.rs` `ChainEvent`: `conversation_id` is the chat session id; there
  is no `chat_id`. `turn_id` is on the chat payload when present. The identity
  fields the links use are all present as `ChainEvent` fields or
  `data.metadata.*` entries the chain block carries.
- The chain relation records exactly these as links from the point id. It omits
  `session_genesis` and never invents `chat_id`.

### The system graph (where the links do *not* go — R3.3)

`system_graph.rs` `replace_system_graph` swaps one namespace's
`system_graph_nodes` / `system_graph_edges` as a replaced inventory snapshot in
a checked transaction. Chain vectors are not inventory. The chain relation this
spec adds is a separate Cozo relation, not `system_graph_edges` and not
`graph_edge`. The system graph may carry the chain as a single inventory node;
it never carries the per-block links.

## [A] The dump — timing first, then live vectors

The dump runs on the live `vectors`/`timing` filesystem under the snowball
writer serialization, because a btrfs snapshot cannot cross volumes (R1.5). Its
two steps are ordered, not atomic:

1. **Snapshot `timing`** into a read-only snapshot (reusing
   `btrfs_snapshot_readonly`). This freezes the set of blocks that exist.
2. **Snapshot `vectors`** into a read-only snapshot, capturing the live vector
   files exactly as they stand — including the absence of a `vec-{n}.bin` whose
   timing record is already frozen in step 1 (R1.4).

The dump's subject is timing+vectors. It does not fold `state` into an aligned
triple (R1.3); `state` is not part of a vector dump. The two snapshots are
placed in a per-dump destination layout that keeps the vector snapshot a
standalone subvolume (not nested under another subvolume), so the send in [B]
carries it as its own stream (R2.4). The dump never snapshots into an empty
subvolume that lives on a different filesystem (R1.5).

The "timing present, vector absent" case is the invariant the order exists to
protect. The account of that block lives on the **source** timing snapshot taken
in step 1: it records that the block exists and that its vector had not landed.
The vector stream in [B] does not contain that block, so the replica never
receives it; the case is observed on the source side, not reconstructed on the
replica. (The complementary case — a vector arriving before its timing record —
is handled on the replica by the existing `upsert_orphan_vector`.)

## [B] Send to the replica — two streams into one writable chain root

The dump sends **two** btrfs streams through
`replication.rs::transfer_prepared_snapshot`, into a **new** writable chain
root `<dest>/` on the replica. One root per dump (R2.6):

- the **timing snapshot** is sent first, with `on_receive` unset, and received
  as `<dest>/timing`;
- the **vector snapshot** is sent second, with `on_receive` set, and received
  as `<dest>/vectors`.

`btrfs receive -- <dest>` creates `<dest>/<snapshot name>` and does not replace
an existing child. The snapshot basenames are therefore `timing` and `vectors`
(R2.5). A later dump uses a new `<dest>`. The previous chain root stays on that
filesystem; its received subvolumes are the incremental parents, found by UUID.
After the next incremental send has succeeded, a root older than that parent
may be removed. When a parent is already gone, that member's next transfer is
a full send (R2.3).

A received vector snapshot *is* the `vectors` tree — `vec-{n}.bin` at its root,
with no `timing/` child — so the timing records cannot ride inside it. The
timing snapshot is its own send landing as the sibling `<dest>/timing` (R2.5).
The link ids live on `block-{n}.json`, not in `vec-{n}.bin`; without the timing
stream the replica could not write any links.

Each stream is sent with a `PreparedSnapshotTransfer` whose `snapshot` is that
member and whose `parent` (when incremental) is the previous snapshot of the
**same original pair on the same filesystem** (R2.1, R2.3, R2.4). The retired
`stream_vectors` bash/ssh send is not used (R2.2).

On the source the dump snapshots are not `SNP-*` sets, so `retain_snapshots`
does not see them. The send still needs the previous snapshot of that member as
its parent. After both sends succeed, the dump keeps the two snapshots just
sent and removes older dump snapshots of those members (R2.8).

`on_receive` is set only on the vector transfer. The hook runs when that second
receive has finished and `<dest>/timing` is already present. Its arguments are
the received vector path and the stream UUID. It opens the parent of that path
(R2.7, R3.1).

## [C] On receive — ingest and chain relation in one pass

The arrival hook is the `on_receive` of the vector transfer. It is given
`<dest>/vectors` and that stream's UUID, and it opens the parent of that path.
That parent is the writable chain root. Its `timing/` and `vectors/` children
are the two received read-only subvolumes. The ingest reads them through a
read-only open. It does **not** call `StreamingSnowball::new` on `<dest>` or on
either child: `new` creates a missing `state` subvolume, a `snapshots/`
directory, and a `.writer.lock` even when the root is writable. The index
watermark is `<dest>/.vector-index-block` (R3.2). Then, in one pass:

1. **Qdrant upsert.** `ingest_received` upserts the received vectors into
   `snowball_footprints` exactly as today — replica path, no Voyage, no
   re-embedding, idempotent by `point_id` (R3.3, R3.7).
2. **Chain relation.** For each vector ingested, write rows into a new chain
   Cozo relation (not `system_graph_edges`, not `graph_edge` — R3.5). Each row
   is a link **from the Qdrant point id** `point_id(block_n)` to one linked id
   (R3.6), with the id read from the received timing record under `<dest>/timing`
   and never re-derived on the replica (R4.2). The relation is written in the
   same pass as the upsert so index and relation never diverge (R3.4), and
   re-ingest overwrites the same rows in place rather than duplicating (R3.7).

### Link set (R4)

For each ingested vector, link these when the **carried timing record** has
them — every value is read from `block-{n}.json`, never from `vec-{n}.bin`:

| Link | Source on the carried timing record | Notes |
|---|---|---|
| `conversation_id` | `ChainEvent.conversation_id` | the chat session id, not a `chat_id` |
| `turn_id` | chat payload | only when the chat payload carries it |
| `event_hash` | block `hash` / `event_hash` | carried verbatim, not re-hashed |
| `input_patch_hash` | `data.payload.input_patch_hash` | provenance digest, not re-hashed |
| `plugin_id` | block `category` | |
| `actor_id` | `data.metadata.actor_id` | |
| `session_id` | `data.metadata.session_id` | |
| `sealed_id_digest` | `data.metadata.sealed_id_digest` | identity commitment |
| `capability_id` | `data.metadata.capability_id` | |
| `subid` | schema subid carried on the timing record | **only when the dump carried it**; the replica has no catalog and cannot derive it |

**Not linked:** `session_genesis`. It is arrival-proof metadata, never an
identity, and the Qdrant payload already excludes it (R4.4). No `chat_id` is
invented (R4.5). A link is written only when its id is present on the carried
timing record; absent ids are omitted (R4.6). Every id is read from the carried
timing record — nothing is re-hashed, re-minted, or synthesized on the replica
(R4.2). `subid` is an origin product of `SchemaProjectionPolicy`; the replica,
on `FlattenAll` with no catalog, links it only when the dump carried it (R4.3).

The anchor of every link is the point id, so the chain relation and the Qdrant
point are joinable: a point found by semantic search resolves to its links, and
a link value resolves back to the point ids that carry it.

## [D] What stays exactly as it is

- **Retention (R6).** `retain_snapshots` keeps only the newest counter (the send
  parent). This spec does not change that function. The dump's own snapshots
  are not `SNP-*` sets; after a successful send it keeps the timing snapshot
  and the vector snapshot just sent (R2.8).
- **Live origin projection (R5.2).** The bridge footprint path stays on
  `SchemaProjectionPolicy` (schema-described fields, `x-secret`/
  `x-opdbus-private` dropped, undeclared keys ignored). `session_genesis` stays
  off the point there too.
- **Operator CLI (R5.1).** `op-chain-vectors project` stays on `FlattenAll`.
- **Replica policy (R5.3).** The replica index stays on `FlattenAll` with no
  catalog; the chain relation is written beside the upsert, not by changing the
  embedding policy.

## Error handling

- **Vector file missing from the vector snapshot while its timing record is
  present:** the intended, protected case. Timing-first ordering puts the block
  in the source timing snapshot; the account that it exists and its vector had
  not landed lives **on that source snapshot**. The vector stream does not carry
  the block, so the replica never receives it — it is not a block the replica is
  expected to hold. A later dump carries the vector once it lands. The block is
  never dropped from the source account.
- **Vector present without a timing record (partial/out-of-order receive):**
  `ingest_received` already indexes it via `upsert_orphan_vector` (payload
  `timing_record_missing: true`). With no timing record carried, there are no
  ids to link; the chain relation records only the point id anchor until a later
  pass carrying the timing record enriches it.
- **Incremental parent absent on destination:** that member's next transfer is a
  full send (existing `replication.rs` contract). Each stream's parent is the
  matching subvolume in the previous chain root. A second receive into a root
  that already has `timing` or `vectors` is not a fallback; the dump uses a new
  root.
- **Ingest opening the chain root:** the hook is given `<dest>/vectors` and
  opens that path's parent. It never calls `StreamingSnowball::new` on `<dest>`
  or on either received child, and it never creates `state`. The watermark is
  `<dest>/.vector-index-block`.
- **Ingest failure after a successful transfer:** the transfer is already
  complete; `on_receive` failure is surfaced through the existing
  `arrival_hook_error` field on `PreparedSnapshotReport`, not hidden, and does
  not undo the received data. A re-run re-ingests idempotently.
- **Btrfs cross-volume snapshot attempt:** rejected by construction — the dump
  snapshots only on the live filesystem (R1.5).

## Verification strategy

- **Dump order:** a test proving `timing` is snapshotted before `vectors`, and
  that the vector snapshot is a standalone (non-nested) subvolume sent as its
  own stream.
- **Two streams, one new chain root:** a test that timing is sent first with
  `on_receive` unset and vectors second with `on_receive` set; that the
  basenames are `timing` and `vectors`; that they arrive as `<dest>/timing` and
  `<dest>/vectors` under a chain root created for that dump; that a second dump
  uses a different root and leaves the first root in place as the parents; and
  that the hook opens the parent of the received vector path. Assert the open
  is read-only, `StreamingSnowball::new` is not called on `<dest>` or on either
  child, no `state` subvolume is created, and the watermark is
  `<dest>/.vector-index-block`.
- **Source parents:** a test that after both sends succeed the source still has
  the timing snapshot and the vector snapshot just sent, and older dump
  snapshots of those two members are gone. `retain_snapshots` is not the
  function that removed them.
- **Missing-vector-present-timing case:** a test where a block has a timing
  record but no `vec-{n}.bin` in the vector snapshot; assert the block is
  accounted for on the **source** timing snapshot (it records the block and its
  absent vector) and that it is **not** in the vector stream — the test checks
  the source snapshot, not a block on the replica, which never receives it.
- **Ids come from the timing record:** a test asserting the links are read from
  the carried timing record and that `vec-{n}.bin` alone yields no ids; the
  replica does not re-derive any id.
- **Link set:** a test asserting the chain relation carries exactly
  `conversation_id`, `turn_id` (only when the chat payload has it), `event_hash`,
  `input_patch_hash`, `plugin_id`, `actor_id`, `session_id`, `sealed_id_digest`,
  `capability_id`, and `subid` (only when the dump carried it), each from the
  point id, and that `session_genesis` is absent and no `chat_id` exists.
- **Relation identity:** assert the chain relation is neither `system_graph_edges`
  nor `graph_edge`, and that the system graph (if it names the chain at all) does
  so as a single inventory node, not per-block links.
- **Idempotence:** re-ingest overwrites the same point id and relation rows in
  place.
- **Unchanged paths:** assert the operator CLI still uses `FlattenAll`, the live
  bridge still uses `SchemaProjectionPolicy`, and `retain_snapshots` still keeps
  only the newest counter.
- **Standard gate:** `/verify` (`cargo fmt --check` + `cargo clippy -D warnings`
  + `cargo test`) via the repo's Cargo dispatch policy.
