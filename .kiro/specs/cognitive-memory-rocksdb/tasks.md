# Implementation Plan — cognitive memory on the identity's own RocksDB

Spec id: `cognitive-memory-rocksdb`. Design: `design.md`. Requirements:
`requirements.md` 1–6. One task = one commit on `dev`, pushed fast-forward
after a fresh fetch/rebase. Cargo: `-p op-cognitive` (and `-p op-plugins` for
task 1) only, one process at a time, private target dir
(`.kiro/steering/subagent-cargo.md`). No task touches the bridge, 3tchedFS,
FUSE, `op-cozo-store`, live services, pools, grants or sealed blobs.

- [ ] 1. Identity volume locator and per-identity store cache
  - `identity_memory/{mod,error,locator,stores}.rs` in `op-cognitive`.
  - Canonical-UUID check, `session_id` match, 0700 `memory/`, best-effort
    nocow, `new_persistent_private` open, per-session handle reuse, duplicate
    path refusal, global-guard refusal.
  - Make `memory_methods::owner_document_key` public (needed by task 4).
  - Unit tests with a temp custom root: refusals, reuse, persistence across
    reopen, two-session isolation of soul, namespaces and plugin documents.
  - _Requirements: 1.1–1.7, 2.1, 2.3, 3.2, 3.4_

- [ ] 2. Local vector index and embedder
  - `identity_memory/vectors.rs`: `identity_memory_vector` relation, upsert,
    delete, exact cosine search with namespace filter, model/dim skip,
    scan limit; `Embedder` trait and `VoyageEmbedder`.
  - Unit tests with fixed vectors.
  - _Requirements: 4.1, 4.2, 4.4, 4.5, 4.6_

- [ ] 3. Router: one dispatch entry point for memory and cognitive memory
  - `identity_memory/router.rs`: `VerifiedMemoryCaller`, `supports`,
    `dispatch` for the 10 plugin methods and `cognitive_memory` / the seven
    `memory_*` methods over a per-identity `MemoryTool`, semantic handling,
    `soul()`.
  - Unit tests: each method, ensure_soul isolation, semantic store/query with
    a fake embedder, no-embedder results, argument session ignored, unknown
    method refused.
  - _Requirements: 2.2, 3.1–3.3, 3.5, 4.1–4.4, 6.1, 6.2_

- [ ] 4. Offline migration and admin command
  - `identity_memory/migrate.rs`: selection by session prefix, soul rows,
    plugin document; dry run; put-if-absent; identical skip; conflict report;
    read-back count check; source never written.
  - `op-cog-admin identity-memory migrate` (dry run by default, `--apply`).
  - Unit tests over temp source and target stores.
  - _Requirements: 5.1–5.5_

- [ ] 5. Exposure check and handoff
  - Test in `op-cognitive` that every routed method is a memory method the
    door classifies as never public (name-rule mirror of
    `public_forbidden`), and that `supports` covers exactly the bridge's
    memory method lists.
  - Mark tasks done; record blockers and the bridge wiring patch in
    `design.md` and `HANDOFF.md`.
  - _Requirements: 6.3_
