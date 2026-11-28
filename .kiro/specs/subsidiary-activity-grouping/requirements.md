# Requirements Document — `subsidiary-activity-grouping`: all subsidiary activity grouped per subsidiary container

> **Owner correction (2026-11-23).** A subsidiary uses the full 3tched
> backend, including reads, mutations and execution, as one isolated unit.
> Resolve its namespace from the admitted identity, never from a requested
> tenant or a caller-supplied prefix. Collections use
> `<subsidiary>::<collection>`; its main chain is
> `<subsidiary>::mainchain`. Backend data, generated resources, execution
> targets, receipts and derived activity must belong to that unit. All list,
> lookup, create, update and delete paths must enforce the same boundary.
> Grant the full sealed capability set to that exact principal, not a group.
> The older shared-vector decision, no-execution ceiling and consumables-only
> restrictions below are superseded where they conflict. Full capabilities
> do not confer host or sibling authority; namespacing must be implemented
> before those capabilities can safely be materialized.

> **Hybrid storage decision (2026-11-20).** A subsidiary keeps its own
> private Cozo database and the chain ID `<subsidiary>::mainchain`. Subsidiary
> vectors share a Qdrant collection using the immutable, verified subsidiary ID
> as the tenant key. This supersedes per-subsidiary collection requirements
> below. Reads and writes use a tenant scope from admission or verified
> chain attribution. Caller arguments do not select tenant scopes.

Status: OWNER-AUTHORIZED IMPLEMENTATION, INCOMPLETE. Current owner corrections
above define the full-backend subsidiary namespace. Historical revisions below
are context only where they do not conflict. No end-to-end isolation, grant
materialization or deployment is claimed from this document.

Spec id: `subsidiary-activity-grouping`
Spec home: `.kiro/specs/subsidiary-activity-grouping/`
Revision 2 (owner decisions of 2026-11-19, after the first draft): keep the
single shared main ledger; each subsidiary chain has the id
`<subsidiaryname>::mainchain` and is vectorized into its **own** Qdrant
collection; Cozo edges for subsidiary activity are dual-written to the main
ledger Cozo and the subsidiary's own Cozo DB.

Revision 3 (owner decisions of 2026-11-19): chain edges are written in the
same pass as the Qdrant upsert, into a new Cozo relation
`subsidiary_chain_edge` (no relation for them exists today); a topology graph
per subsidiary holds a full inventory (installed packages, binaries, services,
sealed plugins, configs and their dependencies) of the container and of the
host side it depends on, with every node's owner (unix and principal), the
grant/capability that authorizes changing it, and its domain
(`host|container`); drift against the sealed release is detected.

Revision 4 (owner decisions of 2026-11-19): each subsidiary has an allowed
resources list covering consumables only (agents, workflows, notebooks, chat
modes, UI generators, tools, models, skills, personas, and further kinds),
never infrastructure; it is stored as data in Cozo and the topology graph and
enforced through the existing grant/capability gate. Package inventory
supports dpkg (antix1 is Debian) and pacman where present.

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
`crates/op-oscal-subid/src/lib.rs`, `crates/op-cozo-store/src/lib.rs`
(`graph_node`, `graph_edge`, `chain_vector_link`),
`crates/op-cozo-store/src/system_graph.rs` (`GraphSnapshot`, `GraphNode`,
`GraphEdge`, `GraphEvidence`, `GraphProvenanceKind`, namespaces),
`crates/op-cozo-store/src/bin/inventory/mod.rs` (prepared-release inventory).

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

Out of scope: enforcing anything from the inventory (ownership, domain and
drift are recorded as data; nothing is blocked on them); running provisioning; LXD/OVN installation; container
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

1. THE SYSTEM SHALL key the full sealed backend capability set, including execution and mutations, by the subsidiary's exact child `principal_id`, without wildcard grants or changes to another principal.
2. THE SYSTEM SHALL resolve the subsidiary namespace from its verified identity and enforce that namespace on every resource selector, mutation, listing and execution target.
3. THE SYSTEM SHALL allow a subsidiary to execute inside its own admitted container and SHALL refuse host or sibling-container targets.
4. THE SYSTEM SHALL derive collections as `<subsidiary>::<collection>` and its main chain as `<subsidiary>::mainchain`; caller arguments SHALL NOT select another subsidiary.
5. THE SYSTEM SHALL materialize owner-authorized exact-principal grants through the existing validated durable grant owner only after affected backends enforce the subsidiary boundary.
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
5. THE SYSTEM SHALL NOT introspect the running container for routing or schemas, SHALL NOT execute anything inside it, and SHALL NOT read its tenant working directory while it is attached; the only container inspection SHALL be the read-only inventory collection of Requirement 15.

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

1. WHEN an edge for subsidiary activity is written (subsidiary link, checkpoint, anchor record, chain edge, topology snapshot) THEN THE SYSTEM SHALL write it first to the main Cozo and then to the subsidiary's own Cozo DB.
2. THE SYSTEM SHALL treat the main ledger Cozo as authoritative and the subsidiary's own Cozo DB as a replica, and SHALL answer every rollup, proof and verification from the main ledger Cozo.
3. THE SYSTEM SHALL make every replica write idempotent, keyed by chain id and link sequence (or point id for chain-vector links), and SHALL record the replica's applied sequence in the replica in the same transaction as the edges.
4. IF the replica write fails THEN THE SYSTEM SHALL keep the main write, SHALL NOT fail or retry the originating call, SHALL mark the replica `lagging` with its applied sequence and an error code, and SHALL record the lag as a ledger event at most once per lag episode.
5. WHILE a replica is `lagging` THE SYSTEM SHALL re-apply edges from its applied sequence plus one in order, with bounded back-off, until it reaches the main ledger head, and SHALL then mark it `current`.
6. WHEN the bridge opens THEN THE SYSTEM SHALL reconcile every non-retired replica against the main ledger Cozo before marking it `current`.
7. IF a replica edge differs from the main ledger edge with the same key, or the replica cannot be opened or verified, THEN THE SYSTEM SHALL mark it `diverged`, stop writing to it, rebuild a new replica from the main ledger alone, swap it in atomically, keep the diverged copy in the host-only directory, and record both digests as a ledger event.
8. THE SYSTEM SHALL refuse a move, export bundle or retirement snapshot while the replica is not `current`.
9. THE SYSTEM SHALL be able to rebuild any subsidiary's own Cozo DB edges from the main ledger alone, with an identical result to the incrementally written replica.

### Requirement 13 — Chain edges written in the same pass as the Qdrant upsert

**User Story:** As the owner, I want every vectorized subsidiary event to have
typed Cozo edges written in the same pass as its Qdrant point, so that the
graph and the vector index never disagree about what was indexed.

#### Acceptance Criteria

1. THE SYSTEM SHALL store chain edges in a new Cozo relation `subsidiary_chain_edge` keyed by chain id, source node, relation and destination node, and SHALL store per-point index state in a new relation `subsidiary_chain_point`.
2. THE SYSTEM SHALL derive each subsidiary point id deterministically from the event hash with the existing `ledger_point_id` scheme, so that re-ingesting an event yields the same point id and the same edge keys.
3. WHEN the vector pass processes a subsidiary block THEN THE SYSTEM SHALL, in one pass and in this order: write the block's edges and a `pending` point row to the main Cozo in one transaction; upsert the point to the subsidiary's collection and wait for it; mark the point row `indexed` in the main Cozo; then apply the same edges and point row to the subsidiary's own Cozo DB.
4. IF the main Cozo transaction fails THEN THE SYSTEM SHALL NOT upsert the point and SHALL NOT advance the vector watermark past that block.
5. IF the Qdrant upsert fails THEN THE SYSTEM SHALL keep the edges and the `pending` point row, SHALL NOT advance the watermark past that block, and SHALL retry the upsert on the next pass.
6. IF marking the point `indexed` fails after a successful upsert THEN THE SYSTEM SHALL resolve the row from Qdrant on the next pass (point present → `indexed`) without re-embedding.
7. IF the subsidiary Cozo write fails THEN THE SYSTEM SHALL handle it as a lagging replica (Requirement 12.4–12.5) and SHALL NOT fail the pass.
8. THE SYSTEM SHALL use only the closed relation vocabulary of the design for chain edges and SHALL refuse any other relation.

### Requirement 14 — Topology graph per subsidiary

**User Story:** As the owner, I want one graph per subsidiary linking it to
its container, identity, grants, FUSE view, storage, DB, collection, chain and
anchors, so that I can see and query everything a subsidiary is made of.

#### Acceptance Criteria

1. THE SYSTEM SHALL keep a topology graph per subsidiary as a system-graph namespace `topology:subsidiary:<subsidiary_name>`, using the existing `GraphSnapshot` node and edge model.
2. THE SYSTEM SHALL include at least these nodes and the edges between them: subsidiary, LXD instance (name and UUID), child principal, parent session, durable and ephemeral grants and their capabilities, FUSE mount and namespace view with its exposed operations, ext4 volume and its directories, the subsidiary's own Cozo DB, its Qdrant collection, its chain id and current head, and its anchors.
3. THE SYSTEM SHALL write the topology graph to the main Cozo first and then replicate the same revision and digest to the subsidiary's own Cozo DB, under the rules of Requirement 12.
4. WHEN a lifecycle event of the subsidiary is recorded (register, rebind, provision, attach, detach, snapshot, revoke, retire, destroy, checkpoint, anchor, collection state change, grant-delta proposal) THEN THE SYSTEM SHALL refresh the topology graph and record the new revision and digest as a ledger event tagged with the subsidiary.
5. THE SYSTEM SHALL answer topology queries (neighbours, paths, traversal, by kind) only through the existing bounded, fixed system-graph queries, and SHALL scope a subsidiary caller to its own namespace.

### Requirement 15 — Full inventory in the topology graph

**User Story:** As the owner, I want the topology graph to hold a complete
inventory of what is installed and configured in the container and on the
host side it depends on, with dependencies, so that I know exactly what a
subsidiary runs on.

#### Acceptance Criteria

1. THE SYSTEM SHALL inventory, for each subsidiary container and for the host side it depends on: installed packages with versions and their package manager; binaries with SHA-256; runit services and their log services; sealed plugins and sealed blobs with schema hashes; configuration files with paths, SHA-256 and consuming services.
2. THE SYSTEM SHALL record dependencies as edges: package to package, binary to shared library, service to service, service to configuration, service to mount or FUSE view, and deployed binary to crate.
3. THE SYSTEM SHALL record a secret only as a reference to its path and SHALL never read, hash or store a secret value.
4. THE SYSTEM SHALL collect container inventory read-only from the container's storage on the host, SHALL never execute anything in the container, and SHALL never read the tenant working directory.
5. THE SYSTEM SHALL keep the host-side inventory in a namespace `inventory:host` and SHALL copy into each subsidiary's topology graph the part of the host inventory that the subsidiary depends on.
6. WHEN a refresh runs THEN THE SYSTEM SHALL refresh on every lifecycle event of Requirement 14.4, on every release activation or catalog reseal, and periodically (full every 6 hours, registry/LXD/grants every hour by default).
7. IF a collector cannot read an item or a bound is reached THEN THE SYSTEM SHALL record it in the snapshot's limitations and SHALL still write the rest.
8. THE SYSTEM SHALL detect the package manager of each inspected root and SHALL read dpkg (`var/lib/dpkg`) and pacman (`var/lib/pacman/local`) databases where present, recording the manager on each package node and a limitation when neither is found.

### Requirement 16 — Owner and domain of every inventory node

**User Story:** As the owner, I want every inventory and topology node to say
who owns it, which domain it belongs to, and what authorizes changing it, so
that responsibility for each item is explicit.

#### Acceptance Criteria

1. THE SYSTEM SHALL record on every node (package, binary, service, configuration, mount, volume, directory, DB, collection, chain and every other topology node) an `owner` attribute with the unix owner, group and mode where the node is a file or directory, and the owning principal of kind `host_owner`, `subsidiary_principal`, `service_account` or `none`.
2. THE SYSTEM SHALL record on every node a `change_authority` attribute naming the grant capability, protected action, release or owner procedure that authorizes changes to it.
3. THE SYSTEM SHALL record on every node a `domain` attribute of `host` or `container`, and for `container` the subsidiary id, subsidiary name, LXD instance and instance UUID.
4. THE SYSTEM SHALL add ownership edges from each node to its owning principal, its unix user and group, and the capability or protected action that authorizes changes.
5. THE SYSTEM SHALL treat owner, change authority and domain as data only and SHALL NOT block or alter any operation based on them.

### Requirement 17 — Drift against the sealed release

**User Story:** As the owner, I want differences between what is installed and
what was sealed and released to be visible, so that I can spot unplanned
changes.

#### Acceptance Criteria

1. WHEN an inventory refresh completes THEN THE SYSTEM SHALL compare the host inventory with the prepared release graph (`release:<id>`) of the active release and the sealed catalog manifest, and each container inventory with its baseline taken at provisioning.
2. THE SYSTEM SHALL report each difference as missing, extra, hash mismatch, version mismatch, owner or mode mismatch, or unsealed plugin, with the expected and observed values.
3. THE SYSTEM SHALL write drift as a system-graph namespace `drift:host` or `drift:subsidiary:<subsidiary_name>` and record its digest and counts as a ledger event (tagged with the subsidiary for container drift).
4. THE SYSTEM SHALL treat drift as data only and SHALL NOT block or alter any operation based on it.

### Requirement 18 — Allowed resources per subsidiary (consumables only)

**User Story:** As the owner, I want a typed list of the consumable resources
each subsidiary may use, such as specific agents, workflows, notebooks or
chat modes, so that a subsidiary gets exactly the services I chose for it and
nothing else, while infrastructure stays host-managed.

#### Acceptance Criteria

1. THE SYSTEM SHALL keep, per subsidiary, an allowed resources list whose entries are a resource kind and a resource id.
2. THE SYSTEM SHALL support at least the kinds `agent`, `persona`, `workflow`, `notebook`, `chat_mode`, `ui_generator`, `tool`, `model` and `skill`, each bound to an existing catalog concept and its capabilities, and SHALL allow new kinds to be added as data with a binding rule.
3. IF an entry names infrastructure (volumes, devices, mounts, network, host services, storage, containers or any plugin in the design's infrastructure set) THEN THE SYSTEM SHALL refuse the entry.
4. IF an entry needs a capability outside the subsidiary's capability ceiling (Requirement 5.2) THEN THE SYSTEM SHALL refuse the entry.
5. WHEN entries are added or removed THEN THE SYSTEM SHALL write a grant-delta proposal for the subsidiary's principal that adds or removes exactly the capabilities and argument constraints the entries need, and SHALL NOT write or materialize a grant document.
6. THE SYSTEM SHALL enforce the allowed resources only through the existing bridge grant/capability gate, with argument constraints for argument-selected resources (workflow id, notebook id, chat mode, model, UI generator profile), and SHALL add no separate allowlist check.
7. THE SYSTEM SHALL store the list in the main Cozo, replicate it to the subsidiary's own Cozo DB under Requirement 12, and record every change as a ledger event tagged with the subsidiary.
8. THE SYSTEM SHALL represent each entry in the subsidiary's topology graph as an `allowed_<kind>` edge from the subsidiary to the resource node, and the resource node's `requires` edges to its capabilities.
9. WHEN the materialized grants of the subsidiary's principal differ from what its active entries need THEN THE SYSTEM SHALL report the difference as drift, as data only.
10. WHILE a subsidiary has no active entry of a kind THE SYSTEM SHALL grant it no capability of that kind.
