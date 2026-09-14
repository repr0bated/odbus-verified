# op-snowball requirements

## Purpose

`op-snowball` transports mutation payloads from the authoritative sled/event path into
append-only timing storage, Btrfs snapshots, replication, and asynchronous vector
projections. A footprint is a payload carrier. It is not an identity, grant key,
content digest, chain head, or second ledger.

## R1 — Footprint boundary

`PluginFootprint` contains only:

- source `plugin_id`;
- `operation`;
- arrival `timestamp`;
- the complete canonical `payload` needed for replay and embedding;
- non-authoritative `metadata`; and
- optional `vector_features`.

It must not contain or compute `data_hash`, `content_hash`, old/new state hashes,
identity fingerprints, grants, or a second chain hash. Callers must not replace the
payload with a digest.

## R2 — One authoritative hash chain

`op-state-store::EventChain` owns the mutation chain. For a new event it computes one
`event_hash` over the previous event hash and the canonical event fields, including the
direct `input_payload`. It must not place `input_patch_hash`, a footprint digest, or an
already-computed hash into that new hash input.

The MutationEngine puts the full serialized `ChainEvent` in `PluginFootprint.payload`.
`PluginFootprint::to_block_event` copies that event's `event_hash` into
`BlockEvent.hash` unchanged. Snowball must never hash the carried `event_hash` again.

Non-chain callers may use `BlockEvent::new`; its fallback hash is computed once from
the direct event data. That fallback must not be used to replace an authoritative
`ChainEvent` hash.

## R3 — Identity separation

SID1/OIA1 identity envelopes, WireGuard keys, principal IDs, session genesis, and
capability grants are owned by the identity and authorization pipelines. They do not
derive from `PluginFootprint`, and no footprint/hash value may authorize a request.
The SID1 integrity trailer is only an envelope byte-integrity check and is not a
Snowball chain term.

## R4 — Vectorization

Vectorization consumes semantic fields from the direct payload. `event_hash` may be
copied into Qdrant payload metadata as an exact correlation/dedup value, but must not
be embedded, relabeled as `input_patch_hash`, or hashed again. Vector loss must not
affect timing records; vectors are rebuildable projections.

## R5 — Timing and Btrfs delivery

- `timing_subvol` stores the append-only JSON `BlockEvent` sequence.
- `vector_subvol` stores optional little-endian vector projections keyed by block.
- `state_subvol` stores current recovery state.
- Snapshot triples use one aligned counter and are distributed through Btrfs
  send/receive.
- Timing records are authoritative; vector and state projections never rewrite their
  event hashes.

## R6 — Compatibility

Legacy persisted chain records without `input_payload` may verify through the bounded
legacy canonical shape. Compatibility fields are read-only and must not be emitted by
new events or become inputs to the new direct-payload hash path.

## R7 — Acceptance

The implementation is accepted only when tests prove:

1. `PluginFootprint` round-trips its exact payload.
2. A carried `event_hash` becomes `BlockEvent.hash` byte-for-byte.
3. Changing the direct event payload breaks event verification.
4. No new event hash includes `input_patch_hash` or a footprint digest.
5. Chain-vector payload stores `event_hash` and `input_patch_hash` under their correct,
   distinct names without rehashing either.
6. Btrfs timing/vector/state snapshot and replication tests pass.
