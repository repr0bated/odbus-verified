# Requirements Document — `subsidiary-activity-grouping`: all subsidiary activity grouped per subsidiary container

Status: SPEC ONLY, NOT APPROVED FOR IMPLEMENTATION. Owner request of
2026-11-19: prepare 3tched to group all subsidiary activity per subsidiary
container. Nothing in this spec is implemented, deployed or provisioned until
the owner approves it and answers the open questions in `design.md`.

Spec id: `subsidiary-activity-grouping`
Spec home: `.kiro/specs/subsidiary-activity-grouping/`
Revision 2 (owner decisions of 2026-11-19, after the first draft): keep the
single shared main ledger; each subsidiary chain has the id
`<subsidiaryname>::mainchain` and is vectorized into its **own** Qdrant
collection; Cozo edges for subsidiary activity are dual-written to the main
ledger Cozo and the subsidiary's own Cozo DB.

Source material: a fresh read of `origin/dev` `879e7d7`:
`crates/op-state-store/src/event_chain.rs` (`ChainEvent`, `EventChain`,
`EventBatch`, `MerkleProof`), `crates/op-state-store/src/method_receipt.rs`
(`MethodExecutionReceipt`), `crates/op-grpc-bridge/src/core.rs` (`StateScope`),
`crates/op-grpc-bridge/src/core_persistence.rs` (the bridge's single Cozo
append ledger `op-grpc-bridge-ledger-v1`, `LedgerRecord`, `LedgerAction`,
per-scope `heads`), `crates/op-grpc-bridge/src/core_operations.rs`,
`crates/op-grpc-bridge/src/grant_document.rs`,
`crates/op-grpc-bridge/src/task_elevation.rs`,
`crates/op-plugins/src/state_plugins/elevation.rs`,
`crates/op-mcp-door/src/{audit,grants,fusecall,identity}.rs`,
`crates/op-execution-tracker/src/{execution_context,record}.rs`,
`crates/op-cognitive/src/chain_vectors.rs`,
`crates/op-grpc-bridge/src/chain_vector_projector.rs`,
`crates/op-grpc-bridge/src/runtime_sled_ledger.rs`,
`crates/op-oscal-subid/src/lib.rs`, `crates/op-cozo-store/src/lib.rs`.

Related specs, which this one does not override:
`.kiro/specs/subsidiary-container/` (the container contract: unprivileged LXD
instance, FUSE call files as the only backend channel),
`.kiro/specs/provisioning-system-subsidiary/` (`--subsidiary` mode: child
identity derived from a parent, grants ⊆ parent's, `user.opdbus.*` labels),
`.kiro/specs/identity-bound-fuse-application-migration/` (identity on the
mount; shared FUSE contracts), `.kiro/specs/mcp-door-adapter/` (door audit,
grants and tickets).

## Introduction

A **subsidiary** is one child identity running in exactly one LXD container
on antix1. This feature makes every piece of activity that belongs to a
subsidiary attributable to it, queryable as a group, provable to the holder of
a receipt, and movable or removable as a unit, without adding any new
authority to the control plane.

Owner decisions this spec encodes:

1. One subsidiary = one container (LXD on antix1).
2. The container reaches the backend only through FUSE call files; container
   networking is deferred and is not needed by anything in this spec.
3. Storage: each subsidiary has its own Cozo/RocksDB on its own host ext4
   volume; RocksDB directories on that volume are bind-mounted into the
   container. Cozo defines the boundary: the host can snapshot, move or revoke a
   subsidiary's Cozo as a unit. Ownership follows the unprivileged container's
   idmap.
4. One shared main ledger stays the only authority. Each subsidiary has a
   derived chain with the id `<subsidiaryname>::mainchain` (e.g.
   `acme::mainchain`).
5. Each subsidiary chain is vectorized into its own Qdrant collection
   (voyage-4, 1024 dimensions, cosine), not a shared collection partitioned by
   payload.
6. Cozo edges for subsidiary activity are written to both the main ledger
   Cozo and the subsidiary's own Cozo DB; the main ledger is authoritative and
   the subsidiary copy is a rebuildable replica.
7. Only hashes go on-chain. The on-chain anchor (owner's EtchedAnchor contract)
   anchors hash-chain heads. A receipt is verifiable only by its holder; the
   local hash protects the IP.
8. Each subsidiary's schemas are pre-generated; the backend performs no routing
   and no introspection of the container.
9. The service/hire contract is the owner's; this spec only fixes the interface
   fields grouping needs (the subsidiary id and link in receipts, and the anchor
   payload).

In scope: a stable subsidiary id and name bound to container identity; the
`<subsidiaryname>::mainchain` chain id; tagging every
chain event, receipt, grant use, ticket, execution record and audit record;
a per-subsidiary chain head for anchoring; per-subsidiary grant scope; a
per-subsidiary FUSE view; the per-subsidiary Cozo DB lifecycle; dual-written
Cozo edges and their reconciliation; rollup queries and receipt export; one
Qdrant collection per subsidiary; teardown and retention; clean integration
with existing 3tched.

Out of scope: running provisioning; LXD/OVN installation; container
networking; the anchor contract and the service/hire contract themselves; the
application that runs inside a subsidiary; changing the main chain's hash
function; changes to pools, the seed or the overlay; changes to the running
`cozo-ublk`; minting identities; editing grant documents (only proposals).

## Requirements

### Requirement 1 — Stable subsidiary id bound to container identity

**User Story:** As the owner, I want every subsidiary to have one stable id
that is bound to its child identity and its container, so that all its
activity can be grouped under one key that cannot be claimed by anything else.

#### Acceptance Criteria

1. WHEN a subsidiary is registered THEN THE SYSTEM SHALL mint exactly one `subsidiary_id` of the form `sub-<uuid v4, lowercase hyphenated>` and SHALL never change or reuse it.
2. THE SYSTEM SHALL bind each `subsidiary_id` to exactly one child `principal_id`, one LXD instance name, that instance's LXD `volatile.uuid`, and the parent session id, and SHALL refuse a registration that would bind a principal or instance already bound to another live subsidiary.
3. THE SYSTEM SHALL label the instance `user.opdbus.subsidiary_id=<subsidiary_id>` through the sealed `lxd` plugin.
4. WHEN a call is admitted THEN THE SYSTEM SHALL resolve the caller's `subsidiary_id` only from the verified principal of the admitted runtime (`StateScope::Private.principal_id`) through the registry, and SHALL ignore any subsidiary id supplied in call arguments.
5. IF the instance label, the instance `volatile.uuid` or the attached child identity differs from the registry binding THEN THE SYSTEM SHALL refuse to attach the subsidiary's storage and backend channel and SHALL record the mismatch as a denied event.
6. WHEN a subsidiary's container is rebuilt with the same child identity THEN THE SYSTEM SHALL keep the same `subsidiary_id` and SHALL record a rebind event naming the old and new instance UUIDs.
7. WHEN a container is bound to a new child identity THEN THE SYSTEM SHALL treat it as a new subsidiary with a new `subsidiary_id`.
8. WHEN a subsidiary is registered THEN THE SYSTEM SHALL require a `subsidiary_name` matching `^[a-z][a-z0-9-]{1,30}[a-z0-9]$` with no `--`, and SHALL refuse the reserved names `main`, `mainchain`, `host`, `owner`, `public`, `system`, `root`, `snowball`.
9. THE SYSTEM SHALL keep `subsidiary_name` unique across every subsidiary ever registered, including retired ones, and SHALL never rename or reuse it.
10. THE SYSTEM SHALL map each `subsidiary_name` to exactly one `subsidiary_id` and back, and SHALL use `subsidiary_id` as the internal key and `subsidiary_name` only for the chain id and the names derived from it.
11. THE SYSTEM SHALL give each subsidiary the chain id `<subsidiary_name>::mainchain` and SHALL derive its link-chain genesis, its Qdrant collection name and its anchor tag from that chain id only.

### Requirement 2 — Tag every activity record

**User Story:** As an auditor, I want every record of a subsidiary's activity
to carry its subsidiary id, so that nothing it did is missing from its group.

#### Acceptance Criteria

1. WHEN the bridge ledger appends an event whose owner principal is bound to a subsidiary THEN THE SYSTEM SHALL set `subsidiary_id` and `subsidiary_role = actor` on the `ChainEvent`.
2. WHEN a host-side operation targets a registered subsidiary (registration, rebind, storage attach/detach, snapshot, move, revoke, retire, grant-delta proposal for its principal) THEN THE SYSTEM SHALL set `subsidiary_id` and `subsidiary_role = subject` on that event, resolved from the registry and never from free-form arguments.
3. THE SYSTEM SHALL include `subsidiary_id` and `subsidiary_role` in the event's canonical hash payload when they are present, and SHALL leave the canonical payload and hash of every event without them byte-identical to today's.
4. THE SYSTEM SHALL carry the subsidiary reference in every method execution receipt, method recovery record, execution context/record, door audit record, ephemeral-grant consumption, ticket consumption and Snowball footprint metadata produced for a subsidiary.
5. IF a record that should carry a subsidiary id (criteria 1, 2 and 4) cannot resolve one THEN THE SYSTEM SHALL fail the append closed and SHALL NOT write the record untagged.
6. THE SYSTEM SHALL NOT tag any record with a subsidiary id that the registry does not hold.

### Requirement 3 — One shared main ledger, one derived `<name>::mainchain` chain per subsidiary

**User Story:** As the owner, I want each subsidiary to have its own
hash-linked head that can be anchored, so that its activity can be proven
without exposing or depending on any other activity.

#### Acceptance Criteria

1. THE SYSTEM SHALL keep the bridge's single Cozo append ledger (`op-grpc-bridge-ledger-v1`) as the only authoritative record of activity and SHALL NOT create a second authoritative chain per subsidiary.
2. WHEN an event tagged with a subsidiary is accepted into the ledger THEN THE SYSTEM SHALL append one link to that subsidiary's chain `<subsidiary_name>::mainchain`, binding the chain id, the subsidiary id, the link sequence number, the previous link hash and the ledger event id and hash.
3. THE SYSTEM SHALL compute link hashes with blake3 and SHALL start every subsidiary's chain at a genesis derived from its chain id, so that no two subsidiaries share a genesis.
4. WHEN the bridge reopens THEN THE SYSTEM SHALL be able to rebuild every subsidiary's link chain from the ledger alone and SHALL fail closed if a stored link differs from the rebuilt one.
5. WHEN a subsidiary checkpoint is taken THEN THE SYSTEM SHALL append a ledger event recording the subsidiary's link head (sequence and hash), so that the derived head is itself committed in the authoritative chain.
6. WHEN verification of a subsidiary is requested THEN THE SYSTEM SHALL check its link chain's linkage, every link's binding to the ledger event it names, and every recorded checkpoint, and SHALL report each failure by sequence number.

### Requirement 4 — Hash-only anchoring and holder-only receipt proofs

**User Story:** As a receipt holder, I want to prove my receipt is included in
an anchored head, and as the owner I want nothing but hashes to leave the host,
so that the proof works without disclosing any IP.

#### Acceptance Criteria

1. WHEN an anchor is prepared for a subsidiary THEN THE SYSTEM SHALL produce an anchor payload containing only hashes and integers: an anchor tag derived from the chain id with a per-subsidiary secret key, the Merkle root over the anchored links, the first and last link sequence, the link head hash and the previous anchor root.
2. THE SYSTEM SHALL NOT put a subsidiary id, subsidiary name, chain id, principal id, method name, argument, result or schema in plaintext into an anchor payload.
3. THE SYSTEM SHALL compute each receipt's Merkle leaf from a per-receipt salt and the canonical receipt, so that a leaf cannot be recomputed without the receipt holder's copy.
4. WHEN a receipt is issued to a subsidiary caller THEN THE SYSTEM SHALL include the salt, the link reference and the leaf hash in that receipt, and SHALL NOT disclose the salt to any other principal.
5. WHEN an anchor has been submitted THEN THE SYSTEM SHALL record the anchor payload hash and the owner-supplied anchor reference as a ledger event tagged with the subsidiary.
6. WHEN an inclusion proof is requested for a receipt THEN THE SYSTEM SHALL return the Merkle siblings from its leaf to the anchored root, containing hashes only.
7. THE SYSTEM SHALL provide an offline verifier that, given a receipt, its proof and an anchor payload, confirms inclusion without contacting the backend.

### Requirement 5 — Per-subsidiary grant scope

**User Story:** As the owner, I want each subsidiary's authority to be its own
narrow scope, so that a subsidiary can never act beyond what I granted it.

#### Acceptance Criteria

1. THE SYSTEM SHALL key a subsidiary's durable grants by its exact child `principal_id`, using the existing grant-document contract (exact keys, closed `$` groups, no `*`).
2. THE SYSTEM SHALL compute a subsidiary's capability ceiling as the parent's capabilities minus every `shell`, `exec`, `admin` and `secret` class capability, and SHALL refuse admission of any call whose capability is outside the ceiling even when a grant names it.
3. IF an ephemeral grant or ticket names a subsidiary principal and a capability outside its ceiling THEN THE SYSTEM SHALL refuse to consume it and SHALL record the refusal tagged with the subsidiary.
4. WHEN a subsidiary principal requests a protected task (elevation) THEN THE SYSTEM SHALL refuse it.
5. THE SYSTEM SHALL only propose grant changes for a subsidiary (grant-delta files) and SHALL NOT write or materialize a grant document.
6. WHEN a subsidiary is revoked THEN THE SYSTEM SHALL revoke every ephemeral grant and unconsumed ticket naming its principal and SHALL write a grant-delta proposal removing its durable entry.

### Requirement 6 — Per-subsidiary FUSE view

**User Story:** As the owner, I want a subsidiary to see only its own
operations and its own activity through its call files, so that the FUSE
view is its whole world and leaks nothing about others.

#### Acceptance Criteria

1. THE SYSTEM SHALL expose to a subsidiary only the child identity's projection of call files, as in `subsidiary-container` Requirement 4.
2. THE SYSTEM SHALL expose the subsidiary's own read operations (identity, head, activity, rollup, receipt export, receipt proof, verification) as sealed `subsidiary` plugin call files whose scope is the caller's resolved subsidiary.
3. IF a subsidiary caller names another subsidiary in a `subsidiary` read call THEN THE SYSTEM SHALL refuse the call without revealing whether the other subsidiary exists.
4. THE SYSTEM SHALL keep subsidiary registry state out of the public state scope.
5. THE SYSTEM SHALL NOT introspect the container, route on its schemas, or read its tenant working directory while it is attached.

### Requirement 7 — One Cozo DB per subsidiary on host ext4

**User Story:** As the owner, I want each subsidiary's data to be one Cozo DB
on host storage, so that I can snapshot, move or revoke a subsidiary as one
unit and keep it across container rebuilds.

#### Acceptance Criteria

1. WHEN a subsidiary's storage is provisioned THEN THE SYSTEM SHALL create one dedicated host ext4 volume for it, holding the subsidiary's own Cozo DB (RocksDB, bridge-written), the tenant working directory and one host-only directory.
2. THE SYSTEM SHALL bind-mount only the tenant working directory into the container, read-write, through the sealed `lxd` plugin, and SHALL never mount the subsidiary's own Cozo DB or the host-only directory into the container.
3. THE SYSTEM SHALL make the tenant directory writable by the container's unprivileged mapped user and by no other unprivileged user, using the instance's idmap.
4. THE SYSTEM SHALL move the subsidiary's storage through the states `provisioned → attached → detached → revoked → retired` (with `detached → attached` allowed) and SHALL record every transition as a ledger event tagged with the subsidiary.
5. WHEN a snapshot is requested THEN THE SYSTEM SHALL pause the replica writer for the subsidiary's own Cozo DB, SHALL include the tenant working directory only while the container does not hold it open, SHALL record the snapshot's blake3 digest and size as a ledger event, and SHALL keep the snapshot in the subsidiary's host-only directory.
6. WHEN a move is requested THEN THE SYSTEM SHALL first bring the subsidiary's own Cozo DB to the main ledger head (Requirement 12), then produce one bundle containing the volume snapshot, the subsidiary's receipt export and its registry record, with a manifest digest recorded as a ledger event.
7. WHEN revocation is requested THEN THE SYSTEM SHALL detach the tenant working directory and the backend channel from the container, keep the data intact, and leave the container stopped.
8. THE SYSTEM SHALL record the digest of the subsidiary's pre-generated schema bundle at registration and every change to it as a ledger event.
9. THE SYSTEM SHALL NOT place subsidiary volumes on, or change, pools, the seed or the overlay.

### Requirement 8 — Rollups and receipt export

**User Story:** As the owner or a subsidiary, I want to query activity as a
group and export receipts, so that I can review and prove what a subsidiary did.

#### Acceptance Criteria

1. WHEN a rollup is requested for a subsidiary and a time or sequence window THEN THE SYSTEM SHALL return counts grouped by plugin, method, decision and outcome, and the first and last link in the window.
2. WHEN the owner requests a parent rollup THEN THE SYSTEM SHALL return the same rollup for every subsidiary of that parent, and SHALL NOT offer it to a subsidiary caller.
3. WHEN a receipt export is requested THEN THE SYSTEM SHALL produce an `op-subsidiary/receipts@1` bundle containing the subsidiary's links, receipts, inclusion proofs, checkpoints and anchor records for the window, and SHALL include no other subsidiary's data.
4. THE SYSTEM SHALL let a subsidiary caller export only its own bundle, and SHALL include receipt salts only in that caller's own export or an owner export.
5. THE SYSTEM SHALL bound rollup and export output size and SHALL page beyond the bound.

### Requirement 9 — One Qdrant collection per subsidiary chain

**User Story:** As the owner, I want each subsidiary's chain vectorized into
its own Qdrant collection, so that a semantic search over one subsidiary can
never return another's activity and a subsidiary's index can be dropped whole.

#### Acceptance Criteria

1. THE SYSTEM SHALL name each subsidiary's collection by replacing `::` in its chain id with `__` (for `acme::mainchain`, the collection `acme__mainchain`), and SHALL refuse a name that collides with an existing collection that is not that subsidiary's.
2. WHEN a subsidiary is registered THEN THE SYSTEM SHALL create its collection with the same vector parameters as the snowball collection (1024 dimensions, cosine distance, voyage-4 embeddings), and IF Qdrant is unreachable THEN THE SYSTEM SHALL complete registration, mark the collection `pending`, and create it before the first upsert.
3. WHEN a vector is written for a block whose event is tagged with a subsidiary THEN THE SYSTEM SHALL write it only to that subsidiary's collection and SHALL NOT write it to the shared snowball collection.
4. THE SYSTEM SHALL keep the existing origin/replica rule: the automatic path fills subsidiary collections only on the replica after `btrfs receive`, and the origin writes them only under the existing manual exception.
5. WHEN a subsidiary caller searches events THEN THE SYSTEM SHALL search only its own collection, selected from admission, and SHALL ignore any collection named in arguments.
6. WHEN a caller that is not a subsidiary searches without naming a subsidiary THEN THE SYSTEM SHALL search only the shared snowball collection, and SHALL let only the owner search a subsidiary's collection.
7. WHEN a subsidiary is retired THEN THE SYSTEM SHALL stop writing to its collection and keep it read-only for the retention period, and WHEN the retention period has elapsed and the owner requests destruction THEN THE SYSTEM SHALL delete the collection and record the deletion as a ledger event.
8. THE SYSTEM SHALL skip retired subsidiaries in every reindex and replica ingest, and SHALL never reuse a deleted subsidiary's collection name.

### Requirement 10 — Teardown and retention

**User Story:** As the owner, I want a defined end of life for a subsidiary,
so that removing it is deliberate, provable and leaves the audit intact.

#### Acceptance Criteria

1. THE SYSTEM SHALL treat revocation and retirement as protected tasks that need an elevation ticket.
2. WHEN a subsidiary is retired THEN THE SYSTEM SHALL keep its ledger events, links, checkpoints and anchor records, and SHALL mark it retired in the registry.
3. THE SYSTEM SHALL destroy a retired subsidiary's volume (its own Cozo DB, tenant working directory and host-only directory) and its Qdrant collection only after its retention period has elapsed and only on an explicit owner request, and SHALL record the destruction with the final snapshot digest.
4. WHEN a retired subsidiary's container is still present THEN THE SYSTEM SHALL refuse retirement until the container is stopped and has neither the tenant working directory nor the backend channel attached.
5. THE SYSTEM SHALL never delete, rewrite or compact ledger events of a subsidiary; destroying its own Cozo DB SHALL leave the main ledger Cozo edges intact.

### Requirement 11 — Clean integration with 3tched

**User Story:** As a maintainer, I want this feature to be additive, so that
existing chains, receipts, tests and clients keep working unchanged.

#### Acceptance Criteria

1. THE SYSTEM SHALL verify every ledger record and event written before this feature with unchanged results.
2. THE SYSTEM SHALL add every new field as optional and omitted when absent, in every serialized form.
3. THE SYSTEM SHALL add no new admission path: admission SHALL remain `identity ∧ grants ∧ sealed` in the bridge.
4. THE SYSTEM SHALL give every new method, schema and event kind a valid OSCAL subid from `op-oscal-subid` and a capability of the form `cap.subsidiary.<method>@v1`.
5. WHEN a subsidiary is retired THEN its activity SHALL remain queryable by the owner from the main ledger with no data migration.

### Requirement 12 — Cozo edges dual-written; main ledger authoritative

**User Story:** As the owner, I want a subsidiary's activity edges in both the
main ledger Cozo and the subsidiary's own Cozo DB, so that the subsidiary's DB
is self-contained for move and export while the main ledger stays the single
source of truth.

#### Acceptance Criteria

1. WHEN an edge for subsidiary activity is written (subsidiary link, checkpoint, anchor record, chain-vector link) THEN THE SYSTEM SHALL write it first to the main ledger Cozo and then to the subsidiary's own Cozo DB.
2. THE SYSTEM SHALL treat the main ledger Cozo as authoritative and the subsidiary's own Cozo DB as a replica, and SHALL answer every rollup, proof and verification from the main ledger Cozo.
3. THE SYSTEM SHALL make every replica write idempotent, keyed by chain id and link sequence (or point id for chain-vector links), and SHALL record the replica's applied sequence in the replica in the same transaction as the edges.
4. IF the replica write fails THEN THE SYSTEM SHALL keep the main write, SHALL NOT fail or retry the originating call, SHALL mark the replica `lagging` with its applied sequence and an error code, and SHALL record the lag as a ledger event at most once per lag episode.
5. WHILE a replica is `lagging` THE SYSTEM SHALL re-apply edges from its applied sequence plus one in order, with bounded back-off, until it reaches the main ledger head, and SHALL then mark it `current`.
6. WHEN the bridge opens THEN THE SYSTEM SHALL reconcile every non-retired replica against the main ledger Cozo before marking it `current`.
7. IF a replica edge differs from the main ledger edge with the same key, or the replica cannot be opened or verified, THEN THE SYSTEM SHALL mark it `diverged`, stop writing to it, rebuild a new replica from the main ledger alone, swap it in atomically, keep the diverged copy in the host-only directory, and record both digests as a ledger event.
8. THE SYSTEM SHALL refuse a move, export bundle or retirement snapshot while the replica is not `current`.
9. THE SYSTEM SHALL be able to rebuild any subsidiary's own Cozo DB edges from the main ledger alone, with an identical result to the incrementally written replica.
