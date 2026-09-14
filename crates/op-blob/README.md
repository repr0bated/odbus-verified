# op-blob

Self-contained, buildable implementation of the architecture defined by
`.kiro/specs/schemars-to-reflection-plugin-pipeline/`,
`schema-coupled-plugin-blob-reflection-whitepaper.md`, and
`BLOB_ARCHITECTURE_SYNTHESIS.md`.

**The plugin is the schema pipeline. The blob is its sealed projection.**

```text
Rust structs (#[derive(schemars::JsonSchema)] + x-oscal-subid extends)
  -> schemars JSON Schema 2020-12                       src/demo.rs (Layer 0)
  -> adapter::plugin_schema_from_json                   src/adapter.rs (Layer 1)
  -> PluginSchema  == SINGLE SOURCE OF TRUTH ==         src/schema.rs (Layer 2)
  -> blob::blobify[_with_identity]                      src/blob.rs
  -> PluginObjectBlob (sealed, zero-copy, hash-addressed)
       section 1  canonical PluginSchema JSON (sha256 = blob identity)
       section 2  manifest: D-Bus identity + gRPC identity + method/OSCAL
                  metadata + WG-keypair account identity
       section 3  protobuf FileDescriptorSet, typed per-method messages
       section 4  compliance metadata + rendered JSON Schema
  -> ActiveReflectionCatalog                            src/catalog.rs
       /dev/shm/opdbus/plugin-blobs/<id>.<hash16>.blob  (replaces monolithic
       live-schema.json; advertises only ACTIVE blobs; serves descriptor
       bytes as borrowed slices of the sealed image)
  -> btrfs filesystem IN the blob artifact              src/btrfs.rs
       mkfs.btrfs --rootdir --shrink --subvol (no root, no mount needed)
       one subvolume per plugin + blob-index.json; loop-mount ro to consume
```

## Directives encoded

- **Zero copy** — `BlobRef` verifies magic + schema hash once, then returns
  borrowed slices; descriptor bytes served to reflection are subslices of the
  sealed image. Sealing is deterministic: same schema ⇒ same bytes ⇒ same hash.
- **One source of truth** — every artifact (D-Bus path, gRPC services,
  reflection descriptors, manifest, compliance metadata) derives from
  `PluginSchema`; nothing is hand-authored downstream.
- **Blob encapsulates schema and metadata** — the whitepaper's
  `PluginObjectBlob` data model, self-describing at the blob boundary.
- **Btrfs filesystem in blob** — the outermost deployment artifact is a
  btrfs image containing the sealed blobs (snapshot/send/receive-able,
  mountable read-only so consumers see the surface as native local).
- **WireGuard keypair is the identity** — `identity::WgKeypair` (X25519,
  `wg genkey`-compatible base64); only the public half enters the manifest.
- **Session lifespan is persistent for the life of the account** —
  `SessionBinding` derives deterministically from the public key; no expiry,
  no rotation; rotating the account key is the only way a session ends.

## Use

```sh
cargo test                                  # 20 tests, includes btrfs seal
cargo run --bin opblob -- demo-seal /tmp/blobs
cargo run --bin opblob -- inspect /tmp/blobs/wireguard.*.blob
cargo run --bin opblob -- btrfs-seal /tmp/opdbus-blobs.btrfs /tmp/blobs
mount -o loop,ro /tmp/opdbus-blobs.btrfs /mnt/opdbus-blob   # root, to consume
```

## Spec compliance map

| Requirement | Where |
|---|---|
| REQ-1/2 struct-derived schema, adapter translation | `adapter.rs`, `demo.rs` |
| REQ-3 OSCAL subids from struct annotations | `adapter.rs`, `subid.rs` |
| REQ-4 typed MethodDecl w/ output (`AckOutput`) | `adapter.rs::method_decl_from_schemars_with_output` |
| REQ-6 typed reflection descriptors (no `Struct`) | `descriptor.rs` (hand-encoded FileDescriptorSet) |
| REQ-7 D-Bus identity `/org/opdbus/v1/plugins/<name>` | `blob.rs::blobify` |
| REQ-8 shm runtime read-path (per-object, tmpfs) | `catalog.rs` |
| NFR-1 zero-copy | `blob.rs::BlobRef`, `catalog.rs` slice serving |
| NFR-2 deterministic builds | canonical JSON (BTreeMap maps), sorted method files |
| Whitepaper "Required Tests" | `tests/pipeline.rs`, module tests |

Note: this repo's `op-plugins/` references workspace crates that are not part
of this repo, so it does not compile here; `op-blob` ports the exact shapes it
needs (`PluginSchema`, the schemars adapter) and stands alone. When merged back
into the full workspace, `op-blob`'s blob/catalog/btrfs layers slot behind
`op-state-store`'s `PluginSchema` unchanged.
