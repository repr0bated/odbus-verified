# Requirements Document — `subsidiary-activity-grouping`: all subsidiary activity grouped per subsidiary container

Status: SPEC ONLY, NOT APPROVED FOR IMPLEMENTATION. Owner request of
2026-11-19: prepare 3tched to group all subsidiary activity per subsidiary
container. Nothing in this spec is implemented, deployed or provisioned until
the owner approves it and answers the open questions in `design.md`.

Spec id: `subsidiary-activity-grouping`
Spec home: `.kiro/specs/subsidiary-activity-grouping/`
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
3. Storage: host ext4 volumes hold RocksDB directories that are bind-mounted
   into the container. Cozo defines the boundary: one Cozo DB per subsidiary,
   which the host can snapshot, move or revoke as a unit. Ownership follows the
   unprivileged container's idmap.
4. Only hashes go on-chain. The on-chain anchor (owner's EtchedAnchor contract)
   anchors hash-chain heads. A receipt is verifiable only by its holder; the
   local hash protects the IP.
5. Each subsidiary's schemas are pre-generated; the backend performs no routing
   and no introspection of the container.
6. The service/hire contract is the owner's; this spec only fixes the interface
   fields grouping needs (the subsidiary id and link in receipts, and the anchor
   payload).

In scope: a stable subsidiary id bound to container identity; tagging every
chain event, receipt, grant use, ticket, execution record and audit record;
a per-subsidiary chain head for anchoring; per-subsidiary grant scope; a
per-subsidiary FUSE view; the per-subsidiary Cozo DB lifecycle; rollup
queries and receipt export; per-subsidiary vector partitioning; teardown and
retention; clean integration with existing 3tched.

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

### Requirement 3 — One authoritative chain, one derived head per subsidiary

**User Story:** As the owner, I want each subsidiary to have its own
hash-linked head that can be anchored, so that its activity can be proven
without exposing or depending on any other activity.

#### Acceptance Criteria

1. THE SYSTEM SHALL keep the bridge's single Cozo append ledger (`op-grpc-bridge-ledger-v1`) as the only authoritative record of activity and SHALL NOT create a second authoritative chain per subsidiary.
2. WHEN an event tagged with a subsidiary is accepted into the ledger THEN THE SYSTEM SHALL append one link to that subsidiary's derived link chain, binding the subsidiary id, the link sequence number, the previous link hash and the ledger event id and hash.
3. THE SYSTEM SHALL compute link hashes with blake3 and SHALL start every subsidiary's link chain at a genesis derived from its `subsidiary_id`, so that no two subsidiaries share a genesis.
4. WHEN the bridge reopens THEN THE SYSTEM SHALL be able to rebuild every subsidiary's link chain from the ledger alone and SHALL fail closed if a stored link differs from the rebuilt one.
5. WHEN a subsidiary checkpoint is taken THEN THE SYSTEM SHALL append a ledger event recording the subsidiary's link head (sequence and hash), so that the derived head is itself committed in the authoritative chain.
6. WHEN verification of a subsidiary is requested THEN THE SYSTEM SHALL check its link chain's linkage, every link's binding to the ledger event it names, and every recorded checkpoint, and SHALL report each failure by sequence number.

### Requirement 4 — Hash-only anchoring and holder-only receipt proofs

**User Story:** As a receipt holder, I want to prove my receipt is included in
an anchored head, and as the owner I want nothing but hashes to leave the host,
so that the proof works without disclosing any IP.

#### Acceptance Criteria

1. WHEN an anchor is prepared for a subsidiary THEN THE SYSTEM SHALL produce an anchor payload containing only hashes and integers: an anchor tag, the Merkle root over the anchored links, the first and last link sequence, the link head hash and the previous anchor root.
2. THE SYSTEM SHALL NOT put a subsidiary id, principal id, method name, argument, result or schema in plaintext into an anchor payload.
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
5. THE SYSTEM SHALL NOT introspect the container, route on its schemas, or read its tenant Cozo DB while it is attached.

### Requirement 7 — One Cozo DB per subsidiary on host ext4

**User Story:** As the owner, I want each subsidiary's data to be one Cozo DB
on host storage, so that I can snapshot, move or revoke a subsidiary as one
unit and keep it across container rebuilds.

#### Acceptance Criteria

1. WHEN a subsidiary's storage is provisioned THEN THE SYSTEM SHALL create one dedicated host ext4 volume for it, holding exactly one tenant Cozo (RocksDB) directory and one host-only directory.
2. THE SYSTEM SHALL bind-mount only the tenant Cozo directory into the container, read-write, through the sealed `lxd` plugin, and SHALL never mount the host-only directory into the container.
3. THE SYSTEM SHALL make the tenant directory writable by the container's unprivileged mapped user and by no other unprivileged user, using the instance's idmap.
4. THE SYSTEM SHALL move the tenant Cozo DB through the states `provisioned → attached → detached → revoked → retired` (with `detached → attached` allowed) and SHALL record every transition as a ledger event tagged with the subsidiary.
5. WHEN a snapshot is requested THEN THE SYSTEM SHALL take it only while the container does not hold the DB open, SHALL record the snapshot's blake3 digest and size as a ledger event, and SHALL keep the snapshot in the subsidiary's host-only directory.
6. WHEN a move is requested THEN THE SYSTEM SHALL produce one bundle containing the tenant DB snapshot, the subsidiary's receipt export and its registry record, with a manifest digest recorded as a ledger event.
7. WHEN revocation is requested THEN THE SYSTEM SHALL detach the tenant DB and the backend channel from the container, keep the data intact, and leave the container stopped.
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

### Requirement 9 — Per-subsidiary vector partitioning

**User Story:** As the owner, I want a subsidiary's vectors kept in its own
partition, so that a semantic search never returns another subsidiary's activity.

#### Acceptance Criteria

1. WHEN a vector is written for an event tagged with a subsidiary THEN THE SYSTEM SHALL write `subsidiary_id` into the point payload.
2. THE SYSTEM SHALL keep a keyword payload index on `subsidiary_id` in each collection that receives subsidiary points.
3. WHEN a subsidiary caller searches events THEN THE SYSTEM SHALL filter on its own `subsidiary_id` taken from admission, in addition to the existing actor and session filters.
4. WHEN a search runs for a caller that is not a subsidiary and no subsidiary filter is requested THEN THE SYSTEM SHALL exclude subsidiary points unless the caller is the owner.
5. WHEN a subsidiary is retired THEN THE SYSTEM SHALL delete its points by filter and SHALL skip it in every later reindex or replica ingest.

### Requirement 10 — Teardown and retention

**User Story:** As the owner, I want a defined end of life for a subsidiary,
so that removing it is deliberate, provable and leaves the audit intact.

#### Acceptance Criteria

1. THE SYSTEM SHALL treat revocation and retirement as protected tasks that need an elevation ticket.
2. WHEN a subsidiary is retired THEN THE SYSTEM SHALL keep its ledger events, links, checkpoints and anchor records, and SHALL mark it retired in the registry.
3. THE SYSTEM SHALL destroy a retired subsidiary's tenant DB and host-only directory only after its retention period has elapsed and only on an explicit owner request, and SHALL record the destruction with the final snapshot digest.
4. WHEN a retired subsidiary's container is still present THEN THE SYSTEM SHALL refuse retirement until the container is stopped and has neither the tenant DB nor the backend channel attached.
5. THE SYSTEM SHALL never delete, rewrite or compact ledger events of a subsidiary.

### Requirement 11 — Clean integration with 3tched

**User Story:** As a maintainer, I want this feature to be additive, so that
existing chains, receipts, tests and clients keep working unchanged.

#### Acceptance Criteria

1. THE SYSTEM SHALL verify every ledger record and event written before this feature with unchanged results.
2. THE SYSTEM SHALL add every new field as optional and omitted when absent, in every serialized form.
3. THE SYSTEM SHALL add no new admission path: admission SHALL remain `identity ∧ grants ∧ sealed` in the bridge.
4. THE SYSTEM SHALL give every new method, schema and event kind a valid OSCAL subid from `op-oscal-subid` and a capability of the form `cap.subsidiary.<method>@v1`.
5. WHEN a subsidiary is retired THEN its activity SHALL remain queryable by the owner from the main ledger with no data migration.
