# op-snowball specification

`op-snowball` is the Btrfs-backed delivery and projection layer for the authoritative
mutation event stream.

## Compiled modules

```text
src/lib.rs
src/footprint.rs              BlockEvent, payload-only PluginFootprint
src/snowball.rs               timing/vector/state storage and Btrfs replication
src/btrfs_delta.rs            send/receive delta discovery
src/btrfs_numa_integration.rs cache/NUMA wrapper
src/retention.rs              snapshot retention policy
src/snapshot.rs               snapshot interval policy
```

There is no compiled `plugin_footprint` or `streaming_snowball` compatibility module.

## Public footprint contract

```rust
pub struct PluginFootprint {
    pub plugin_id: String,
    pub operation: String,
    pub timestamp: u64,
    pub payload: simd_json::OwnedValue,
    pub metadata: HashMap<String, simd_json::OwnedValue>,
    pub vector_features: Vec<f32>,
}
```

The footprint has no digest fields. For MutationEngine events, `payload.event_hash` is
the already-authoritative chain hash and is copied unchanged into `BlockEvent.hash`.

## Storage contract

`StreamingSnowball::add_footprint` converts the payload envelope to a `BlockEvent` and
appends it to `timing_subvol/block-{N:012}.json`. Optional vectors are stored separately
and can be attached/rebuilt by block number. State and the timing/vector streams are
snapshotted with a shared aligned counter and replicated through Btrfs send/receive.

## Integration contract

- `op-state-store::EventChain` owns canonical event hashing and verification.
- `op-grpc-bridge::MutationEngine` converts a complete `ChainEvent` into the footprint
  payload and persists it.
- `op-cognitive-mcp::chain_vectors` embeds semantic payload fields and records exact
  hash metadata without rehashing.
- Identity and grants never consume footprint/hash values.

See [REQUIREMENTS.md](REQUIREMENTS.md) for normative invariants and
[DESIGN.md](DESIGN.md) for data flow and ownership.
