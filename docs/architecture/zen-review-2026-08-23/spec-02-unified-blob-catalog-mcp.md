# Spec 02: `unified-blob-catalog-mcp`

**Spec Path**: [`.kiro/specs/unified-blob-catalog-mcp/requirements.md`](file:///srv/git/odbus/.kiro/specs/unified-blob-catalog-mcp/requirements.md)  
**Domain**: MCP, Catalog & Vector Embeddings  
**Status**: **PASS (Verified & Hardened)**

---

## Requirements Verification Matrix

| Requirement | Requirement Statement | Code Implementation | Status |
|---|---|---|:---:|
| **R1 (Collection)** | Dedicated Qdrant collection named `blob_vectors` (overridable via `COGNITIVE_MCP_BLOB_VECTORS_COLLECTION`) holds one point per active plugin blob. | [`crates/op-cognitive-mcp/src/qdrant_shuttle.rs:21,55`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/qdrant_shuttle.rs#L21-L55): `DEFAULT_BLOB_VECTORS_COLLECTION = "blob_vectors"`. | **PASS** |
| **R1 (Vectors)** | Vector format: `voyage-4` 1024-dim embedding of `render_schema_embedding_text(schema)`. | [`crates/op-cognitive-mcp/src/qdrant_shuttle.rs:51-52`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/qdrant_shuttle.rs#L51-L52). | **PASS** |
| **R1 (Payload)** | Payload: `{ plugin_id: string, text: string }` storing rendered text alongside vector. | Stored in Qdrant point payload during vectorization. | **PASS** |
| **R1 (Point ID)** | Point ID: derived deterministically via UUIDv5 from `plugin_id`. | [`crates/op-cognitive-mcp/src/qdrant_shuttle.rs:621-623`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/qdrant_shuttle.rs#L621-L623): `uuid::Uuid::new_v5(&BLOB_VECTORS_NAMESPACE, plugin_id.as_bytes())`. | **PASS** |
| **R2 (Renderer)** | `render_schema_embedding_text(schema)` renders name, version, tags, immutable_paths, and fields into deterministic text. | [`crates/op-cognitive-mcp/src/qdrant_shuttle.rs:498-540`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/qdrant_shuttle.rs#L498-L540). | **PASS** |
| **R3 (Rebuild)** | Explicit user-triggered rebuild command (`RebuildBlobVectors`) rather than automatic background reindexing. | [`crates/op-cognitive-mcp/src/grpc_service.rs:70-95`](file:///srv/git/odbus/crates/op-cognitive-mcp/src/grpc_service.rs#L70-L95). | **PASS** |
| **R4 (Graph)** | Dependency graph traversal pulls adjacent plugin schemas into context using `PluginSchema.dependencies`. | [`crates/op-plugins/src/state_plugins/mod.rs`](file:///srv/git/odbus/crates/op-plugins/src/state_plugins/mod.rs): `dependencies` resolved during vector enrichment. | **PASS** |
| **R5 (Zero-Copy)**| Zero-copy schema access via `BlobRef` with eager UTF-8 checks. | [`crates/op-blob/src/blob.rs:346-385`](file:///srv/git/odbus/crates/op-blob/src/blob.rs#L346-L385). | **PASS** |
