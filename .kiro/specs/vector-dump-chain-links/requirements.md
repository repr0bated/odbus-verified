# Requirements — vector dump and chain links

## Why this spec exists

Live vectorization writes vectors into the `vectors` subvolume as an async
projection: a chain block's timing record is written first, and its vector file
lands later when the embedder catches up. A **dump** is the operation that
captures that live state and sends it to a replica.

A correct dump has an order that the current path does not have:

1. Snapshot `timing` first, so the record of every block that exists is frozen.
2. Snapshot the live `vectors` next, capturing whatever vectors have landed.
3. Send the timing snapshot and the vector snapshot as two streams into a new
   writable chain root on the replica.

The timing-first order is load-bearing. It names a block whose vector file was
still being written when the vector snapshot was taken: the timing record is in
the frozen timing snapshot, the vector file is not in the vector snapshot. The
account of that block — that it exists and that its vector had not landed — lives
on the **source** timing snapshot. The replica never receives such a block,
because the vector stream does not contain it; the gap is observable on the
source side, not on the replica.

The link ids are not in the vector file either. `vec-{n}.bin` is only the raw
f32 floats. `conversation_id`, `turn_id`, `actor_id`, `session_id`,
`event_hash`, `input_patch_hash`, `plugin_id`, `sealed_id_digest`, and
`capability_id` all live on the **timing record** (`block-{n}.json`). A dump
that sent the vector snapshot alone could not write those Cozo links on the
replica. So the ids must travel with the dump — read from the timing record that
belongs to that same dump — and the replica must not re-derive them.

The current `op-snowball` path does not do this. `create_snapshot` takes **one
aligned triple** of `timing`, `vectors`, and `state` under a single counter,
and `transfer_snapshot_set` sends all three members. There is no dump that
sends the timing snapshot and the vector snapshot as two streams after a
timing-first snapshot, and nothing on the receive side ingests the vectors into
Qdrant and a chain relation in the same pass. On receive, `on_receive` is
`None`, so `ingest_received` is never called.

This spec defines the vector dump (timing-first snapshot, then live-vector
snapshot, then two prepared-transfer streams into a new chain root) and the
chain links written beside the Qdrant upsert on receive. It is a design against
the files below, not a restoration of the pre-snowball code.

## Original code — read it, do not restore it wholesale

There was no snowball then. The pre-snowball implementation lived in
`/home/home/artix/git/operation-dbus-proto/crates/op-blockchain/`. It is the
source of the timing-first snapshot order. Its send was the vector snapshot
alone, and that send is not carried over.

- `blockchain.rs` — `add_event` writes the timing file first. Vectors are async
  projections filled later.
- `footprint.rs` — `to_block_event` stores an empty vector (timing is
  authoritative; the vector is attached later). The current
  `crates/op-snowball/src/footprint.rs` preserves exactly this behavior and
  cites both original files.
- `streaming_blockchain.rs` — `create_snapshot` writes `timing-{hash}`, **then**
  `vectors-{hash}`. `stream_vectors` sends the vector snapshot. That send is
  **unused** (`#[allow(dead_code)]`) and shells out through `bash -c
  "btrfs send … | ssh … 'btrfs receive …'"`. **This send is not brought back.**
  The dump sends the timing snapshot and the vector snapshot as two streams
  through the current native prepared transfer in
  `crates/op-snowball/src/replication.rs`, never through bash/ssh.

## Ground truth captured during authoring (read off these files)

These are facts read from the current repository, not assumptions.

### `crates/op-snowball/src/snowball.rs`
- `SNAPSHOT_LABELS = ["timing", "vectors", "state"]`.
- `create_snapshot` / `create_snapshot_aligned` take one aligned triple: under
  one counter it snapshots all three `SNAPSHOT_LABELS` through
  `commit_snapshot_set_sync`, which calls `btrfs_snapshot_readonly(source,
  staging.join(label))` for each label. The doc comment states this takes "one
  aligned triple and does not do that [vector] send."
- `transfer_snapshot_set` loops `SNAPSHOT_LABELS` and transfers each member via
  `crate::replication::transfer_prepared_snapshot`, with `on_receive: None` on
  every member. So the set transfer sends all three and runs no arrival hook.
- `commit_snapshot_set_sync(snapshot_dir, counter, &sources: &[(String,
  PathBuf); 3])` snapshots into a staging dir then renames it to `SNP-{counter}`
  with a `.committed` marker. It is hard-wired to three labels.
- `retain_snapshots` keeps only the newest `counter` (the send parent). The
  doc comment says so explicitly. The prune path at commit uses
  `prune_snapshot_sets_sync`. A dump's timing snapshot and vector snapshot are
  not `SNP-*` sets, so that prune does not see them.
- `add_event` writes the timing record first, then the vector file only if the
  event already carries a vector; `attach_vector` fills a block's vector later
  and rejects a block with no timing record.
- Block files: `block-{n:012}.json` (timing), `vec-{n:012}.bin` (vectors). On
  disk the vector is raw little-endian f32.

### `crates/op-snowball/src/replication.rs`
- `transfer_prepared_snapshot(PreparedSnapshotTransfer)` is the native prepared
  transfer: native btrfs `send` over a pipe into `ssh … btrfs receive --
  <dir>`. Preflight validates the source is a read-only subvolume root with
  snapshot/received provenance.
- Incremental send uses the original snapshot pair on the **same filesystem**
  (`validate_parent` requires `source.filesystem_uuid == parent.filesystem_uuid`
  and a distinct stream identity). The native `send` ioctl checks the actual
  incremental relationship.
- Receive creates a **new read-only subvolume** at the destination; the parent
  is found by UUID on the destination filesystem.
- `on_receive` is an explicitly requested remote executable invoked with the
  received snapshot's absolute path and stream UUID; it is never a shell
  fragment. It is `None` on the current snowball set-transfer path.

### `crates/op-cognitive-mcp/src/chain_vectors.rs`
- `ingest_received(from_block)` is the **replica** path: after `btrfs receive`
  it reads the vector files present and upserts them into the Qdrant collection
  `snowball_footprints` (`DEFAULT_COLLECTION`). No Voyage, no re-embedding
  (`open_replica` builds with `voyage: None`).
- `ingest_received` reads `self.chain`, a `StreamingSnowball` opened in `build`
  at `OPDBUS_SNOWBALL_PATH` (default `/var/lib/opdbus/snowball`). It reads the
  index's **own** chain path — **not** the subvolume `btrfs receive` just
  created. So firing ingest after a receive requires opening the received
  subvolume as the chain the ingest reads; a bare `ingest_received` on the
  default-opened index would not see the received data.
- `upsert_block` builds the Qdrant point. Payload is **allow-listed provenance
  only**, and every value comes from the **timing record** the block was parsed
  from: `block_num`, `event_hash` (the block's `hash`), `input_patch_hash` (from
  `data.payload.input_patch_hash`), `plugin_id` (the block `category`),
  `action`, `timestamp`, and when present from `data.metadata.*`: `actor_id`,
  `session_id`, `sealed_id_digest`, `capability_id`, `decision`, plus `text` and
  policy `payload_fields`. None of these come from `vec-{n}.bin`. The comment
  says: "never the session genesis." `session_genesis` is not placed on the
  point.
- `point_id(block_num) = uuid::Uuid::new_v5(BLOCK_POINT_NAMESPACE,
  "block:{n}")`. The namespace is fixed forever. This UUID is the Qdrant point
  id — the anchor every link references.
- `on_receive` is `None` on the snowball transfer path, so **receive does not
  call `ingest_received` today**.
- `FlattenAll` is the default `ProjectionPolicy`; the index is built with it in
  `build`. A replica has no sealed catalog, so it stays on `FlattenAll` and
  cannot produce `subid` itself — it can link `subid` only when the dump already
  carried it.

### `crates/op-grpc-bridge/src/chain_vector_projector.rs` and `crates/op-state-store/src/schema_projection.rs`
- `SchemaProjectionPolicy` is the **live origin bridge** projector's noise
  filter. It embeds only schema-described fields of the mutation
  (`described_fields`): `x-secret` and `x-opdbus-private` fields are dropped,
  undeclared keys are ignored. It is the live footprint path.
- Because of that filter, `session_genesis` is not placed on the Qdrant point by
  this path either.
- `subid` is produced **on the origin** by `SchemaProjectionPolicy`
  (`payload_fields` / the described field's subid), read from the sealed plugin
  schema. The replica has no catalog and stays on `FlattenAll`, so it cannot
  produce `subid` and can link it only when the dump carried it on the timing
  record.
- `described_fields(schema, value)` walks the schema, emitting described scalar
  leaves and dropping redacted subtrees and undeclared keys.
- The **operator CLI** `op-chain-vectors project` uses `FlattenAll` (the index
  default); this spec does not change that, and does not switch the live bridge
  path off `SchemaProjectionPolicy`.

### `crates/op-state-store/src/event_chain.rs`
- `ChainEvent.conversation_id` is the chat session id ("the conversation (chat
  session) this event belongs to"). `turn_id` is carried on the chat payload
  when present, not a column on `ChainEvent`. There is **no `chat_id`** column.
- Identity/provenance fields on `ChainEvent`: `event_hash`, `input_patch_hash`,
  `plugin_id`, `actor_id`, `session_id`, `sealed_id_digest`, `capability_id`,
  and `session_genesis` (metadata only, "never used as an identity, grant key,
  or payload digest").

### `crates/op-cozo-store/src/system_graph.rs`
- `system_graph_edges` is a **replaced inventory snapshot** relation
  (`replace_system_graph` swaps one namespace's `system_graph_nodes` /
  `system_graph_edges` in a checked transaction). Chain vectors do **not** go
  into it, and the chain relation this spec adds is **not** `system_graph_edges`
  and **not** `graph_edge`. The system graph may name the chain as one inventory
  node only.

## Terminology

- **Dump** — snapshot `timing`, then snapshot live `vectors`, then send those
  two snapshots as separate streams into a new writable chain root
  (`<dest>/timing` and `<dest>/vectors`) so the replica can read the link ids
  from the received timing records.
- **Vector member** — the `vectors` snapshot alone, sent as its own btrfs
  stream.
- **Chain relation** — a new Cozo relation written on receive beside the Qdrant
  upsert, keyed on the Qdrant point id, carrying the block's identity links. Not
  `system_graph_edges`, not `graph_edge`.
- **Link** — an identity/provenance id carried on the vector, recorded in the
  chain relation as pointing from the point id to that id.
- **Point id** — `uuid::Uuid::new_v5(BLOCK_POINT_NAMESPACE, "block:{n}")` from
  `chain_vectors.rs`; the anchor of every link.

## Requirements

### R1 — The dump snapshots timing first, then live vectors

**User story:** As the operator running a dump, I want timing frozen before the
live vectors are snapshotted, so a block that exists is always captured even
when its vector file is still being written.

#### Acceptance criteria
1. The dump SHALL snapshot the `timing` subvolume first, producing a read-only
   snapshot, before it snapshots `vectors`.
2. The dump SHALL then snapshot the live `vectors` subvolume, capturing whatever
   vector files have landed at that moment.
3. The dump SHALL NOT require the two snapshots to be an aligned triple with
   `state`. The dump is a timing+vectors operation; `state` is not its subject.
4. A block whose timing record is in the timing snapshot but whose `vec-{n}.bin`
   is absent from the vector snapshot SHALL be accounted for on the **source**
   timing snapshot. The vector stream does not contain such a block, so the
   replica never receives it; the gap is detectable on the source side, not on
   the replica. The block SHALL NOT be silently dropped from the source account.
5. Each snapshot SHALL be created on the live `vectors`/`timing` filesystem (a
   btrfs snapshot cannot cross volumes). The dump SHALL NOT snapshot into an
   empty subvolume placed on another filesystem.

### R2 — Both members are sent through the existing prepared transfer

**User story:** As the operator, I want the timing snapshot and the vector
snapshot sent by the current native prepared transfer, each as its own stream,
into a new chain root, not by the retired bash/ssh send.

#### Acceptance criteria
1. The dump SHALL send the vector snapshot as its own btrfs stream through
   `crates/op-snowball/src/replication.rs`
   (`transfer_prepared_snapshot` / the prepared-transfer entry), never through
   `bash -c "btrfs send … | ssh … btrfs receive"`.
2. The retired `stream_vectors` send from `operation-dbus-proto`
   `streaming_blockchain.rs` SHALL NOT be reintroduced in any form.
3. Incremental send SHALL use the original snapshot pair on the same filesystem,
   as `replication.rs` already enforces; the received parent must still exist on
   the destination filesystem or the transfer is a full send.
4. The vector snapshot SHALL be sent as a standalone stream. A snapshot placed
   under another subvolume is nested and is not included by a send of the outer
   subvolume; the vector dump is its own stream.
5. The dump SHALL also send the timing snapshot as its own btrfs stream through
   the same prepared transfer, into the same new chain root as the vector
   stream. `btrfs receive` creates `remote_directory/<snapshot name>`, so the
   snapshot basenames SHALL be `timing` and `vectors` and the two SHALL land as
   `<dest>/timing` and `<dest>/vectors`. A received vector snapshot is the
   `vectors` tree itself (`vec-{n}.bin` at its root, no `timing/` child), so the
   timing records cannot ride inside it; the link ids live on `block-{n}.json`,
   and the timing stream is how they reach the replica. The timing snapshot and
   the vector snapshot each keep their own incremental parent (same original
   pair, same filesystem).
6. Each dump SHALL create a new chain root `<dest>/`. Receive creates
   `<dest>/timing` and `<dest>/vectors` there. The dump SHALL NOT receive a
   later pair into a chain root that already contains those names. The previous
   chain root stays on the destination filesystem; its received `timing` and
   `vectors` are the incremental parents, found by UUID. After the next
   incremental send has succeeded, a chain root older than that parent MAY be
   removed. When a parent is already gone, that member's next transfer is a
   full send.
7. The dump SHALL send the timing snapshot first with `on_receive` unset, then
   send the vector snapshot with `on_receive` set. The hook runs at the end of
   that second transfer, after `<dest>/timing` already exists. It is invoked
   with the received vector subvolume's absolute path and stream UUID, and it
   opens the parent of that path as `<dest>`. The dump SHALL NOT add a second,
   parallel transport or control plane for ingest.
8. On the source, the dump's timing snapshot and vector snapshot are not
   `SNP-*` sets, so `retain_snapshots` does not see them. The send uses the
   previous snapshot of that member as its parent. After both sends succeed,
   the dump keeps the timing snapshot and the vector snapshot just sent, and
   removes older dump snapshots of those two members. Those two kept snapshots
   are the next incremental parents. This does not change `retain_snapshots`.

### R3 — Receive ingests the vector into Qdrant and writes a chain relation in the same pass

**User story:** As the replica, I want each received vector upserted into
`snowball_footprints` and its chain links recorded in one pass, so the index and
the relation never diverge.

#### Acceptance criteria
1. The dump SHALL send the timing snapshot for the blocks it sends (R2.5), so
   the ids the chain relation needs arrive on the replica. The receive-side
   ingest SHALL read those ids from the received timing record under
   `<dest>/timing`, and SHALL NOT re-derive, recompute, or synthesize them on
   the replica.
2. The arrival hook SHALL open the parent of the received path it was given.
   That parent is the writable chain root `<dest>`, and its `timing/` and
   `vectors/` children are the two received read-only subvolumes. The ingest
   SHALL read that root through a read-only open of those existing children.
   The hook SHALL NOT call `StreamingSnowball::new` on `<dest>` or on either
   received subvolume. `new` creates a `state` subvolume, a `snapshots/`
   directory, and a `.writer.lock`, including when the root is writable. The
   index watermark SHALL be `<dest>/.vector-index-block`, outside the two
   received subvolumes.
3. After both receives, the ingest SHALL upsert the received vectors into the
   Qdrant collection `snowball_footprints` exactly as today (replica path, no
   Voyage, no re-embedding).
4. In the **same pass**, for each vector ingested, the ingest SHALL write a
   chain Cozo relation recording the vector's links.
5. The chain relation SHALL NOT be `system_graph_edges` and SHALL NOT be
   `graph_edge`. It is a distinct relation.
6. Every link SHALL be recorded from the Qdrant point id
   (`uuid::Uuid::new_v5(BLOCK_POINT_NAMESPACE, "block:{n}")`, i.e.
   `chain_vectors.rs` `point_id`) to the linked id.
7. The ingest SHALL remain idempotent: re-receiving or re-ingesting the same
   block SHALL overwrite the same point id and the same relation rows in place,
   not duplicate them (consistent with the point id being keyed on block number
   and fixed forever).

### R4 — The link set is exactly the declared identity set, read from the carried timing record, and session_genesis is excluded

**User story:** As an auditor, I want each vector linked to the identity and
provenance ids that let me trace it, drawn from the timing record the dump
carried, with the session genesis kept off the links.

#### Acceptance criteria
1. The ingest SHALL link, when the carried timing record has them:
   `conversation_id`, `turn_id` (only when the chat payload has it),
   `event_hash`, `input_patch_hash`, `plugin_id`, `actor_id`, `session_id`,
   `sealed_id_digest`, `capability_id`, and the schema `subid`.
2. Every link id SHALL be read from the timing record (`block-{n}.json`) carried
   by the dump — the vector file `vec-{n}.bin` is only the raw floats and carries
   no ids. The replica SHALL NOT re-hash, re-mint, or synthesize any id.
3. `subid` is produced on the origin by `SchemaProjectionPolicy` and the replica
   has no catalog, so the ingest SHALL link `subid` only when the dump already
   carried it on the timing record; the replica SHALL NOT derive `subid` itself.
4. The ingest SHALL NOT link `session_genesis`. This matches the Qdrant payload
   rule in `chain_vectors.rs` ("never the session genesis") and the event-chain
   note that genesis is arrival-proof metadata, never an identity.
5. `conversation_id` SHALL be treated as the chat session id and `turn_id` as a
   per-turn id present only on chat payloads; the spec SHALL NOT invent a
   `chat_id` column, since `ChainEvent` has none.
6. A link SHALL be written only when its id is actually present on the carried
   timing record; absent ids SHALL be omitted, not recorded as empty links.

### R5 — The live and operator projection paths are not changed by this spec

**User story:** As the maintainer, I want this spec to add the dump and the
receive-side relation without disturbing how the live bridge and the operator
CLI already project.

#### Acceptance criteria
1. The operator CLI `op-chain-vectors project` SHALL continue to use
   `FlattenAll`.
2. The spec SHALL NOT switch the live bridge footprint path off
   `SchemaProjectionPolicy`.
3. The replica ingest path's existing policy (`FlattenAll`, no catalog on the
   replica) SHALL be unchanged; the chain relation is written beside the upsert,
   not by changing the embedding policy.

### R6 — Prune behavior is left exactly as it is

**User story:** As the maintainer, I want the existing prune behavior untouched
by this spec.

#### Acceptance criteria
1. The spec SHALL leave `retain_snapshots` keeping only the newest counter (the
   send parent); it SHALL NOT change that prune rule.
2. No task in this spec SHALL change `retain_snapshots`. The dump pair is not
   an `SNP-*` set. After a successful send the dump keeps only the timing
   snapshot and the vector snapshot just sent (R2.8).

## Out of scope — do not spec these

- Disaster-recovery JSON, PackageKit, or systemfs inventory.
- `qemu-img` seed provisioning.
- Deleting live snapshots, releases, or `target/release`.
- Restarting services.
- Putting chain blocks into the system graph. The system graph may name the
  chain as one inventory node only; the per-block links do not go there.
- Reintroducing the retired bash/ssh `stream_vectors` send.
- Changing `retain_snapshots`. The prune rule stays "keep the newest counter."
  The dump keeps the timing snapshot and the vector snapshot just sent (R2.8).
- Changing the live bridge `SchemaProjectionPolicy` or the operator CLI's
  `FlattenAll`.
- Adding a `chat_id` column to `ChainEvent`.
