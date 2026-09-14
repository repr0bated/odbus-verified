# op-snowball design

## Ownership

The production mutation path has one author for each kind of data:

```text
MutationEngine / EventChain
  canonical direct mutation payload
  prev_hash + payload -> event_hash (once)
              |
              v
PluginFootprint
  unchanged payload + routing metadata
              |
              v
StreamingSnowball
  timing BlockEvent with the same event_hash
  optional vector/state projections
              |
              v
Btrfs snapshot triple -> send/receive -> vector ingestion
```

`PluginFootprint` is deliberately an envelope, despite its historical name. Hashing
belongs to `EventChain`; identity sealing belongs to the MutationEngine/SID1 path;
authorization belongs to exact principal grants.

## Data structures

`PluginFootprint` carries `plugin_id`, `operation`, `timestamp`, `payload`, `metadata`,
and `vector_features`. Its production payload is the lossless serialized `ChainEvent`,
including the direct `input_payload`, `prev_hash`, and authoritative `event_hash`.

`BlockEvent` carries `timestamp`, `category`, `action`, `data`, `hash`, and `vector`.
When produced from a chain footprint, `hash` is assigned directly from
`payload.event_hash`. The fallback constructor hashes direct data once for callers
that do not supply a chain event.

## Persistence layout

`StreamingSnowball` owns:

- `timing/`: numbered JSON block events and the authoritative replay payload;
- `vectors/`: optional raw little-endian `f32` projections;
- `state/`: recovery state; and
- `snapshots/`: aligned read-only timing/vector/state triples.

Snapshots are published and deployed through Btrfs send/receive. Timing is written
before optional vector attachment. A missing vector can be rebuilt from timing data.

## Hash behavior

For current events, `EventChain::compute_hash` canonicalizes the direct event fields and
`input_payload`, with `prev_hash` providing chain linkage. `input_patch_hash` is a
separate payload-integrity compatibility value and is excluded from the current event
hash input. This prevents hashing a digest instead of the payload and prevents a
hash-of-a-hash chain.

On replay, the stored `event_hash` and `prev_hash` are verified rather than replaced.
The footprint-to-Snowball conversion likewise preserves the stored `event_hash`.

## Vector projection

The vector worker reads semantic text/fields from `BlockEvent.data.payload`. Qdrant
metadata copies the exact `event_hash` for correlation and copies the distinct
`input_patch_hash` only when that field exists. Neither is transformed into another
digest. The embedding vector is not an authority input.

## Security boundary

Footprints are audit/vector delivery records, not credentials. The sealed SID1 value
lives in the identity sled, is protected by local file permissions, and exact-matches
the authoritative active session during MCP authentication. Snowball data cannot be
used as a principal, grant selector, or identity header.

## Compatibility

The reader may accept legacy chain events that lack `input_payload` and verify them
with their historical canonical shape. New writes always use the direct-payload shape.
Deleted legacy source modules are not part of the compiled crate.
