# Tasks — vector dump and chain links

Order is fixed and follows the design spine (A → B → C): the dump snapshots
timing first then live vectors, sends the timing snapshot and then the vector
snapshot into a new chain root, receive ingests into Qdrant, and the chain
relation is written beside the upsert — then tests. No task changes
`retain_snapshots`,
changes the live bridge `SchemaProjectionPolicy`, or changes the operator CLI's
`FlattenAll`.

The dump reuses `btrfs_snapshot_readonly` and
`replication.rs::transfer_prepared_snapshot`; it does not reuse the three-label
hard-wiring of `commit_snapshot_set_sync` as its snapshot step, and it does not
restore the retired bash/ssh `stream_vectors` send.

Cargo/build/test execution is delegated to the repo's Cargo worker per policy;
the primary agent owns source, decisions, and the report.

---

## Phase 1 — The dump: snapshot timing first, then live vectors

- [ ] 1.1 Snapshot `timing` first on the live filesystem
  - Add a dump entry in `op-snowball` that, under the snowball writer
    serialization, snapshots the `timing` subvolume to a read-only snapshot via
    `btrfs_snapshot_readonly` before touching `vectors`. The snapshot is created
    on the live `timing`/`vectors` filesystem (a btrfs snapshot cannot cross
    volumes); never snapshot into an empty subvolume on another filesystem.
  - Do NOT fold this into `create_snapshot_aligned`'s three-label triple; the
    dump's subject is timing+vectors, not `state`.
  - _Requirements: R1.1, R1.3, R1.5_

- [ ] 1.2 Snapshot the live `vectors` next, capturing the as-is set
  - After the timing snapshot, snapshot the live `vectors` subvolume read-only,
    capturing whatever `vec-{n:012}.bin` files have landed — including the
    absence of a vector whose timing record is already frozen in 1.1.
  - Place the vector snapshot as a standalone subvolume (not nested under another
    subvolume) so it can be sent as its own stream in Phase 2.
  - The "timing present, vector absent" block is accounted for on the source
    timing snapshot from 1.1; it is not in the vector snapshot and is not an
    error.
  - _Requirements: R1.2, R1.4, R2.4_

## Phase 2 — Send both members into a new writable chain root

- [ ] 2.1 Send the timing snapshot first, into a new chain root
  - Create a new `<dest>/` for this dump. Snapshot basename `timing`. Send it
    through `replication.rs::transfer_prepared_snapshot` with `on_receive`
    unset. The incremental parent is the previous timing snapshot of the same
    original pair on the same filesystem, which is the `timing` child of the
    previous chain root, found by UUID. Receive creates `<dest>/timing`.
  - Do NOT receive into a chain root that already contains `timing` or
    `vectors`. Do NOT reintroduce the retired `stream_vectors` bash/ssh send.
  - _Requirements: R2.1, R2.2, R2.3, R2.5, R2.6_

- [ ] 2.2 Send the vector snapshot second, into that same new chain root
  - Snapshot basename `vectors`. Send it through the same prepared transfer.
    The incremental parent is the previous vector snapshot of the same original
    pair. Receive creates `<dest>/vectors`, a `vectors` tree with `vec-{n}.bin`
    at its root. Leave the two streams separate.
  - Set `on_receive` on this transfer only. The hook runs after this receive,
    when `<dest>/timing` is already present. Its arguments are the received
    vector path and the stream UUID. Surface a hook failure through
    `arrival_hook_error`, without undoing the completed transfers.
  - _Requirements: R2.4, R2.5, R2.7, R4.2_

- [ ] 2.3 Keep the send parents after both sends succeed
  - The dump snapshots are not `SNP-*` sets, so `retain_snapshots` does not see
    them. After both sends succeed, keep the timing snapshot and the vector
    snapshot just sent and remove older dump snapshots of those two members.
    On the destination, keep this chain root and the previous one. A root older
    than the parent may be removed only after the next incremental send has
    succeeded. Do NOT change `retain_snapshots`.
  - _Requirements: R2.6, R2.8, R6.1, R6.2_

## Phase 3 — Ingest on receive, chain relation beside the Qdrant upsert

- [ ] 3.1 Open the parent of the hook path, then fire `ingest_received`
  - The hook is given `<dest>/vectors`. Open the parent of that path. Read the
    existing `timing/` and `vectors/` children through a read-only open. Do NOT
    call `StreamingSnowball::new` on `<dest>` or on either child: `new` creates
    a `state` subvolume, a `snapshots/` directory, and a `.writer.lock` even on
    a writable root. Do NOT create a `state` subvolume. Write the index
    watermark at `<dest>/.vector-index-block`.
  - Then run `ingest_received`, upserting the received vectors into
    `snowball_footprints` exactly as today — replica path, no Voyage, no
    re-embedding, idempotent by `point_id`.
  - _Requirements: R3.1, R3.2, R3.3, R3.7, R5.3_

- [ ] 3.2 Write the chain Cozo relation in the same pass
  - For each vector ingested, write rows into a new chain Cozo relation in the
    same pass as the upsert. The relation is NOT `system_graph_edges` and NOT
    `graph_edge`. Chain vectors do not enter the system graph; the system graph
    may name the chain as one inventory node only.
  - Each row is a link from the Qdrant point id
    (`uuid::Uuid::new_v5(BLOCK_POINT_NAMESPACE, "block:{n}")`, i.e.
    `chain_vectors.rs` `point_id`) to the linked id. Re-ingest overwrites the
    same rows in place.
  - _Requirements: R3.4, R3.5, R3.6, R3.7_

- [ ] 3.3 Record exactly the declared link set from the carried timing record; exclude `session_genesis`
  - Read every link id from the carried timing record (`block-{n}.json`), never
    from `vec-{n}.bin` and never re-derived on the replica. Link, when the timing
    record has them: `conversation_id`, `turn_id` (only when the chat payload
    carries it), `event_hash`, `input_patch_hash`, `plugin_id`, `actor_id`,
    `session_id`, `sealed_id_digest`, `capability_id`, and the schema `subid`.
  - Link `subid` only when the dump carried it: it is an origin product of
    `SchemaProjectionPolicy`, and the replica on `FlattenAll` with no catalog
    cannot derive it.
  - Do NOT link `session_genesis`. Treat `conversation_id` as the chat session id
    and `turn_id` as a per-turn id on chat payloads; do NOT invent a `chat_id`
    column (`ChainEvent` has none). Write a link only when its id is present on
    the carried timing record.
  - _Requirements: R4.1, R4.2, R4.3, R4.4, R4.5, R4.6_

## Phase 4 — Tests

- [ ] 4.1 Test the dump order and the standalone vector stream
  - Assert `timing` is snapshotted before `vectors`, and the vector snapshot is a
    standalone (non-nested) subvolume sent as its own stream through the prepared
    transfer.
  - _Requirements: R1.1, R1.2, R2.1, R2.4_

- [ ] 4.2 Test a new chain root per dump and the second-transfer hook
  - Assert timing is sent first with `on_receive` unset and vectors second with
    `on_receive` set. Assert the basenames are `timing` and `vectors`, they
    arrive as `<dest>/timing` and `<dest>/vectors`, and a second dump uses a
    different root while the first root stays as the parents. Assert the hook
    is given the received vector path and opens its parent. Assert the open is
    read-only, `StreamingSnowball::new` is not called on `<dest>` or on either
    child, no `state` subvolume is created, and the watermark is
    `<dest>/.vector-index-block`. Assert that after both sends succeed the
    source still has the two snapshots just sent and older dump snapshots of
    those members are gone, and that `retain_snapshots` was not changed.
  - _Requirements: R2.5, R2.6, R2.7, R2.8, R3.2, R6.2_

- [ ] 4.3 Test a vector file missing from the vector snapshot while its timing record is present
  - Construct a block with a timing record but no `vec-{n}.bin` in the vector
    snapshot. Assert the block is accounted for on the **source** timing snapshot
    (it records the block and its absent vector) and that it is not in the vector
    stream. The test checks the source snapshot, not a block on the replica — the
    replica never receives such a block.
  - _Requirements: R1.4_

- [ ] 4.4 Test the link set, the timing-record source, and the excluded genesis
  - Assert every link id is read from the received timing record under
    `<dest>/timing` and that `vec-{n}.bin` alone yields no ids. Assert the chain
    relation carries exactly `conversation_id`, `turn_id` (only when the chat
    payload has it), `event_hash`, `input_patch_hash`, `plugin_id`, `actor_id`,
    `session_id`, `sealed_id_digest`, `capability_id`, and `subid` (only when the
    dump carried it), each anchored on the point id. Assert `session_genesis` is
    absent and no `chat_id` exists. Assert the relation is neither
    `system_graph_edges` nor `graph_edge`.
  - _Requirements: R3.5, R3.6, R4.1, R4.2, R4.3, R4.4, R4.5_

- [ ] 4.5 Verify the unchanged paths and run the gate
  - Confirm the operator CLI still uses `FlattenAll`, the live bridge still uses
    `SchemaProjectionPolicy`, and `retain_snapshots` still keeps only the newest
    counter (unchanged prune rule). Run `/verify` via the Cargo dispatch.
  - _Requirements: R5.1, R5.2, R6.1, R6.2_

---

### Out of scope (restated for the executor)
Disaster-recovery JSON, PackageKit, systemfs inventory; `qemu-img` seed
provisioning; deleting live snapshots, releases, or `target/release`; restarting
services; putting chain blocks into the system graph (one inventory node is the
most it may name); reintroducing the retired bash/ssh `stream_vectors` send;
changing `retain_snapshots`; the dump keeps the two snapshots just sent;
changing the live
bridge `SchemaProjectionPolicy` or the operator CLI's `FlattenAll`; adding a
`chat_id` column to `ChainEvent`. The tasks above are the implementation.
