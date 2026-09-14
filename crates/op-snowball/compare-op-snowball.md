# op-snowball implementation comparison

The compiled crate matches the current payload-carrier design:

| Requirement | Implementation |
|---|---|
| Payload-only footprint | `src/footprint.rs::PluginFootprint` |
| No hash-of-hash conversion | `PluginFootprint::to_block_event` copies `event_hash` |
| Timing/vector/state layout | `src/snowball.rs::StreamingSnowball` |
| Btrfs delta replication | `src/btrfs_delta.rs` |
| NUMA/cache wrapper | `src/btrfs_numa_integration.rs` |
| Snapshot/retention policy | `src/snapshot.rs`, `src/retention.rs` |

The former `plugin_footprint.rs` and `streaming_snowball.rs` modules are retired and
must not be restored. Chain hashing remains in `op-state-store::EventChain`; identity
sealing remains outside this crate.
