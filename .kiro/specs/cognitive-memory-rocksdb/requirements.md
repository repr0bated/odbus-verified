# Requirements Document — cognitive memory on the identity's own RocksDB

Status: SPEC ONLY: implementation deferred until after the build. Owner request of 2026-11-19 ("did you
map the soul and namespace memories to the rocksdb in container ... you
decide"). Implementation authorizes source and tests in `crates/op-cognitive`
and one visibility change in `crates/op-plugins` (`memory_methods.rs`). It
authorizes no edit of the bridge (`crates/op-grpc-bridge`), 3tchedFS /
`threetched-fs`, FUSE, `op-cozo-store` (claimed by OD-126), `cozo-ublk`,
pools, the seed, the overlay, ZeroClaw or sealed blobs, and no service start,
container creation or live data migration.

Spec id: `cognitive-memory-rocksdb`
Spec home: `.kiro/specs/cognitive-memory-rocksdb/`
Source material: `/workspace/cognitive-memory-audit.md` (2026-11-19) and a
fresh read of `origin/dev` `ff85735` (see `design.md` "What exists today").

Related specs, which this one does not override:
`.kiro/specs/mcp-door-adapter/` (memory tools are never public),
`.kiro/specs/cockpit-container/` (identity sled, Cozo/RocksDB store),
`.kiro/specs/identity-bound-fuse-application-migration/` (identity on the
mount; Wonder's shared FUSE contracts for identity binding, capability
checks, toolset projection and receipts).

## Introduction

"The container IS the identity" (`runtime_sled_ledger.rs`). Each identity
container has a sled: the pool volume `identity-<session>`, mounted read-only
in the container at `/agent/user`, written only by the host bridge.

Today the cognitive memory of every identity lives in shared, host-global
stores and is separated only by a `session/<session>/` name prefix:

- the soul (`soul_memories`, `agent_namespace_bindings`, and the
  `soul:user-container:<ns>` namespace written by `cognitive_memory
  ensure_soul`) and the namespace memories (`memory_namespaces`,
  `memory_entries`) of `op-cognitive` live in one Cozo-on-RocksDB database at
  `COGNITIVE_DB_PATH` (default `/var/lib/op-dbus/cognitive-memory.db`), with
  vectors in Qdrant;
- the `memory` plugin's namespace/entry CRUD lives as one runtime document per
  (principal, session) in the shared plugin-runtime Cozo store
  (`/var/lib/op-dbus/plugin-runtime-cozo`).

This feature gives every identity its own Cozo/RocksDB memory store inside its
identity volume, and moves soul memory, namespace memory, memory-plugin CRUD
and the memory vectors there. Calls still arrive only as FUSE call files; the
bridge stays the only writer and the only caller-authorization point.

In scope: a per-identity store resolver; per-identity soul, namespace and
memory-plugin stores; a local vector index with a pluggable embedder (Qdrant
is down); a single dispatch entry point for the bridge; an offline, dry-run
first migration tool; tests.

Out of scope (blockers, see `design.md`): wiring the entry point into
`runtime_cognitive.rs` (bridge code), running the migration on antix1,
starting Qdrant, any FUSE/3tchedFS change, grants changes.

## Requirements

### Requirement 1 — One memory store per identity, inside its identity volume

**User Story:** As Jeremy, I want each identity container's memories stored in
that identity's own RocksDB, so that one identity's memory can never be read
or damaged through another identity's store and leaves with the identity.

#### Acceptance Criteria

1. WHEN a memory operation is dispatched for a session THEN THE SYSTEM SHALL
   resolve that session's identity volume and open the Cozo/RocksDB store at
   `<identity volume>/memory/cozo`.
2. IF the session id is not a canonical UUID THEN THE SYSTEM SHALL refuse the
   operation before touching the filesystem.
3. IF the identity volume does not exist, or its `session_id` file does not
   equal the session THEN THE SYSTEM SHALL refuse the operation and SHALL NOT
   fall back to any shared or host-global store.
4. WHEN the store directory is created THEN THE SYSTEM SHALL create it with
   mode 0700 and request no-copy-on-write before any RocksDB file exists.
5. WHILE a store for a session is open THE SYSTEM SHALL reuse the same handle
   for every later operation of that session in the process.
6. IF two different sessions resolve to the same store directory THEN THE
   SYSTEM SHALL refuse the second open.
7. IF the per-identity store would share a handle with a host-global store
   THEN THE SYSTEM SHALL refuse to serve memory from it.

### Requirement 2 — Soul memory per identity

**User Story:** As an identity, I want my soul (identity, personality, traits,
namespace binding) kept in my own store, so that it survives sessions and is
mine alone.

#### Acceptance Criteria

1. WHEN the soul of an identity is upserted THEN THE SYSTEM SHALL write it to
   that identity's store and increment its version.
2. WHEN `cognitive_memory ensure_soul` runs for an identity THEN THE SYSTEM
   SHALL create the soul namespace and its `_identity` entry in that
   identity's store only.
3. WHEN another identity reads souls THEN THE SYSTEM SHALL NOT return the
   first identity's soul.

### Requirement 3 — Namespace memories per identity

**User Story:** As an identity, I want my memory namespaces and entries
(cognitive `memory_*` and the `memory` plugin CRUD) in my own store, so that
listing, querying and deleting only ever sees my data.

#### Acceptance Criteria

1. WHEN `memory_store`, `memory_recall`, `memory_retrieve`, `memory_query`,
   `memory_delete` or `memory_list_namespaces` runs for an identity THEN THE
   SYSTEM SHALL execute it against that identity's store.
2. WHEN a `memory` plugin method (`create_namespace`, `delete_namespace`,
   `query_namespace`, `insert_entry`, `update_entry`, `get_entry`,
   `delete_entry`, `clear_namespace`, `get_stats`, `compact`) runs for an
   identity THEN THE SYSTEM SHALL execute it against that identity's store.
3. WHEN `memory_list_namespaces` or `get_stats` runs THEN THE SYSTEM SHALL
   report only namespaces of that identity's store.
4. WHEN the process restarts THEN THE SYSTEM SHALL return the same entries for
   the identity as before the restart.
5. IF a method name is not a memory method THEN THE SYSTEM SHALL refuse it.

### Requirement 4 — Vectors without Qdrant

**User Story:** As Jeremy, I want semantic memory to work while Qdrant is
down, so that memory is not blocked on a service.

#### Acceptance Criteria

1. WHEN an entry is stored with `semantic: true` and an embedder is configured
   THEN THE SYSTEM SHALL store its vector in the identity's own store.
2. WHEN `memory_semantic_query` runs and an embedder is configured THEN THE
   SYSTEM SHALL rank only that identity's vectors by cosine similarity and
   return the current entries with their scores, marked `backend: "local"`.
3. IF no embedder is configured THEN THE SYSTEM SHALL store the entry without
   a vector, say so in the result (`semantic_mirrored: false`), and fail
   `memory_semantic_query` with an explicit "semantic memory unavailable"
   error rather than a non-semantic result.
4. WHEN an entry is deleted or overwritten THEN THE SYSTEM SHALL delete or
   replace its vector in the same store.
5. IF a vector's dimension or model differs from the query embedding THEN THE
   SYSTEM SHALL skip it.
6. IF the identity has more vectors than the configured scan limit THEN THE
   SYSTEM SHALL fail the query rather than return a partial ranking.

### Requirement 5 — Migration from the shared stores

**User Story:** As Jeremy, I want existing memories moved into each
identity's store without loss, so that nothing is forgotten in the switch.

#### Acceptance Criteria

1. WHEN migration runs without `--apply` THEN THE SYSTEM SHALL report what it
   would copy and SHALL write nothing.
2. WHEN migration runs with `--apply` for a session THEN THE SYSTEM SHALL copy
   that session's namespaces (`session/<session>/…` and
   `soul:user-container:session/<session>/…`), their entries, the soul and
   namespace binding of that session, and the session's `memory` plugin
   document into the identity's store.
3. WHEN a record already exists in the target with identical content THEN THE
   SYSTEM SHALL skip it; IF it exists with different content THEN THE SYSTEM
   SHALL report a conflict and leave the target record unchanged.
4. WHEN migration finishes THEN THE SYSTEM SHALL verify the copied counts by
   reading the target back and SHALL never delete from the source.
5. IF a source store cannot be opened (for example because a running process
   holds its lock) THEN THE SYSTEM SHALL stop with that error.

### Requirement 6 — Grants and exposure

**User Story:** As Jeremy, I want memory to stay grant-gated, so that only an
identity holding the memory capability reaches its own memory.

#### Acceptance Criteria

1. WHEN the dispatch entry point is called THEN THE SYSTEM SHALL require a
   verified caller (principal and session) supplied by the caller-authorizing
   layer and SHALL use only that session to select the store.
2. IF arguments name a different session, identity or container than the
   verified caller THEN THE SYSTEM SHALL ignore them for store selection.
3. WHERE memory tools are exposed to external clients THE SYSTEM SHALL keep
   them out of the public class (already enforced by
   `op-mcp-door::classify::public_forbidden`).
