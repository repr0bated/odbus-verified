# Appendix: Blob Definition, Lifecycle, and Current-State Analysis

> Extracted from *BLOB_ARCHITECTURE_SYNTHESIS.md* (2026) and scoped to the present repo.
> This appendix complements the main whitepaper. It keeps the blob definition, lifecycle,
> and current-state analysis, and defers the btrfs-as-deployment-unit packaging details to
> the stale-excerpts file.

## Definition of the Blob Architecture

The blob architecture evolved from the need to move from a monolithic live schema catalog
to self-contained, sealed, schema-derived runtime artifacts.

Core questions that shaped the design:

> "is there a way to make schema like blobs so a proto could be a blob ?"  
> "can the dbus object + reflection proto be a blob ?"  
> "we are going to be blobbing everything. the sled, compliance blobs"  
> "no more monolithic live schema ... individual blobs"  
> "blob as sealed projection artifact" + "shm_path with hash in filename" + "ActiveReflectionCatalog tied to blob lifetime"  
> "blobify_plugin_schema in bridge, not plugin"

A **plugin object blob** is a self-contained, sealed, schema-derived, zero-copy/loadable
runtime artifact. The current implementation is in `crates/op-blob/src/blob.rs` and carries:

- `schema_json` + `schema_hash` (authoritative contract)
- `dbus_identity` + path
- `grpc_file_descriptor_set` (for dynamic reflection)
- `blob_method_manifest` + richer metadata (footprint policy, subids, OSCAL, etc.)
- (later) state snapshots, model pointers, providers config

Serialization uses `postcard` today. Blobs are written to `/dev/shm/opdbus/plugin-blobs/<plugin_id>.<schema_hash16>.blob`.

### Lifecycle

1. The plugin declares `schema()` and state from `PluginSchema`.
2. `op_blob::blobify_plugin_schema()` (or `blobify_plugin_schema_with_identity()`) seals the blob.
3. The blob is persisted to `/dev/shm/opdbus/plugin-blobs/` and registered in `ActiveReflectionCatalog`.
4. The D-Bus + gRPC surface appears for clients natively.

This replaces the old monolithic `/dev/shm/live-schema.json` or whole-projections model.

### Scope note on packaging

The source document also discusses btrfs subvolumes as a future packaging layer for complete
deployment units. In this repo, the zero-btrfs-overhead identity rule (`AGENTS.md`) limits
btrfs to vectorized footprint transport (e.g., `op-snowball`) and forbids btrfs overhead on
identity/Xray paths. Those btrfs-as-deployment-unit details are therefore excluded from this
appendix and captured in the stale-excerpts file.

## Current State (as of July 2026)

### Achieved foundations

- **Blob types are implemented.** `PluginObjectBlob`, `blobify_plugin_schema()`, and `ActiveReflectionCatalog` live in `crates/op-blob` and are consumed by `op-grpc-bridge` (`src/plugin_object_blob.rs`, `src/dynamic_reflection.rs`, `src/grpc_server.rs`, `src/mutation_engine.rs`).
- Zeroclaw is registered as a full `StatePlugin`:
  - `crates/op-plugins/src/state_plugins/zeroclaw.rs` sets `LLM_PROVIDER=ollama` with a default local model of `gemma3:4b` (`DEFAULT_LOCAL_MODEL`). `gemma4` is available as a provider alias / route, not the default.
  - Router config classifies inputs and emits tags/hints.
  - Providers include factory, ollama, openrouter, gemini, antigravity, opencode/cli, etc., plus OSCAL policy.
  - Schema is dynamically generated via `schema_from_state` from live `ZeroclawState` (schema-as-code is correct).
- SchemaEngine + `/dev/shm` projections are authoritative; D-Bus objects are live from schema only.
- The zeroclaw gRPC path was consolidated into the main `op-grpc-bridge` binary on port 8090. The separate `op-grpc-bridge-zeroclaw` binary has been retired.
- BTRFS primitives are used in the snowball footprint: `op-snowball` implements timed/vector/state subvolumes; `deploy/btrfs-layout.sh` provides module subvolumes and s6 services.
- `ZeroclawPlugin` `apply_state`/`calculate_diff` are still stubs (schema-declared only, no real mutation yet).
- Ollama is expected to be available locally; there is no `ollama-srv` s6 service in the canonical installer (`install/3tched-artix-s6-install.sh`).

### Not yet implemented: full blob deployment

- The foundational blob types are implemented, but the **btrfs-backed "zeroclaw-gemma-blob" deployment unit** is not wired.
- No orchestration "completes" a blob containing model + config so that external D-Bus/gRPC clients view it exactly as a native plugin.
- Incus + Xray + ghostbridge `container.sock` are transport pieces, not yet blob-packaged.
- BTRFS deploy tooling for complete runnable artifacts is partial (modules are not the sealed reflection+LLM blobs).
- No explicit "blob as the deployment" path for zeroclaw; the current path is still normal s6 services plus local ollama.

### Why full blob deployment is not yet implemented

1. **Staging priorities** (rollouts coded in `.codex/memories`):
   - Sled identity + zero-copy SHM source-of-truth.
   - Plugin refactor to explicit `schema()` + SchemaEngine projections.
   - Unix socket ownership canonicalization.
   - Zeroclaw absorbing LLM / `op-llm` retirement + 3-layer boundary clarification.
   - `grpc-bridge-zeroclaw` + `projection_server` deployment (script fixes ongoing).
   - Incus/qdrant shuttle wiring.
   - D-Bus first everywhere, no bypasses.
2. The blob vision surfaced late and is conversation-heavy. Directives ("read codex about blobs", "gemma up first") came *after* much runtime work.
3. **Engineering caution** (`AGENTS.md`): avoid unintended btrfs mutation overhead in hot paths. Subvolumes are proven only in the snowball footprint first.
4. **Deploy fragility observed**: symlinked s6 dirs caused `cp` issues in scripts; release builds are heavy, so single-job builds were needed.
5. The "complete blob" requires solving packaging + mount semantics + reflection reload (rethinking the monolithic model) — no longer gated on core interop, but a follow-on effort.
6. The model is present and services are normal s6, rather than a "blob subvolume activated instance."

<!-- Extracted from BLOB_ARCHITECTURE_SYNTHESIS.md on 2026-07-20 -->
