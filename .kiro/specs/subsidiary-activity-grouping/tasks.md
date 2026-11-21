# Implementation Plan

> **Storage decision (2026-11-20).** The Qdrant tasks below are superseded.
> Provision the HNSW relation in each subsidiary's private Cozo DB for
> `<subsidiary>::mainchain`. Route only
> records with verified subsidiary attribution to that DB. Commit the vector and
> its local edges atomically, keep indexing progress per database, and test
> isolated receive replay, failed writes, and caller-scoped search. Registry,
> event attribution, and admission are prerequisites.

Spec only. Not approved for implementation; waits for the owner's go-ahead
and answers to the design's Open Questions (at least 1, 2, 3, 4, 9, 11, 12,
14, 15, 17, 18, 19). Tasks that change `op-grpc-bridge` or the `_3tched-fs` receipt
contract need explicit owner approval at that point, because both are shared,
running components. Order matters: readers before writers (design "Error
Handling").

- [ ] 1. Tag types and hashing in `op-state-store`
  - [ ] 1.1 Add `SubsidiaryRole` and optional `subsidiary_id` / `subsidiary_role` to `ChainEvent` and `CanonicalEventPayload` with `skip_serializing_if = "Option::is_none"` (same pattern as `sealed_id_digest`); leave `LegacyCanonicalEventPayload` unchanged
  - [ ] 1.2 Tests: untagged events from fixed fixtures hash byte-identically to today; a tagged event's hash changes and `verify()` passes; serialized untagged events contain no new keys
  - _Requirements: 2.3, 11.1, 11.2_
- [ ] 2. Names, chain ids and the link chain in `op-state-store/src/subsidiary_chain.rs` (pure, no IO)
  - [ ] 2.1 `SubsidiaryName` validator (regex, no `--`, reserved names) and `ChainId` = `<name>::mainchain` with `collection_name()` (`::` → `__`) and its reverse
  - [ ] 2.2 `SubsidiaryLink` with `chain_id`, genesis from the chain id, `link_hash`, salted `receipt_leaf`, keyed `anchor_tag` over the chain id, blake3 Merkle root and proof
  - [ ] 2.3 Anchor payload type `op-subsidiary/anchor@1` with a constructor that only accepts hashes and integers
  - [ ] 2.4 Tests: name accept/refuse table, chain id and collection name round-trip, distinct genesis per chain id, linkage break detected by seq, proof round-trip, a flipped byte fails, payload contains no name, chain id or id in plaintext
  - _Requirements: 1.8, 1.11, 3.2, 3.3, 4.1, 4.2, 4.3, 4.6, 9.1_
- [ ] 3. `SubsidiaryReceiptRef` (with `chain_id`) on `MethodExecutionReceipt` and `MethodReceiptRecovery` (optional, omitted when absent), with tests that existing receipts serialize unchanged
  - _Requirements: 2.4, 4.4, 11.2_
- [ ] 4. Execution tracker fields: optional `executor_subsidiary_id` / `requestor_subsidiary_id` on `ExecutionContext` and `DelegationAttribution`; tests for old JSON still parsing and new JSON round-trip
  - _Requirements: 2.4, 11.2_
- [ ] 5. Offline verifier bin `op-subsidiary-verify` (receipt + proof + anchor payload → ok/fail, no network), with a golden test
  - _Requirements: 4.7_
- [ ] 6. Registry relations in `op-cozo-store` (main ledger Cozo): `subsidiary`, `subsidiary_by_name`, `subsidiary_by_principal`, `subsidiary_by_instance`, `subsidiary_link`, `subsidiary_anchor`, `subsidiary_replica`
  - [ ] 6.1 Create/read/update functions with uniqueness checks on name (forever, including retired and destroyed), principal and instance UUID; no rename function
  - [ ] 6.2 Tests on an in-memory `CozoGraphShuttle`: mint format, name refusal after retirement, refusal of a second binding, rebind keeps id and name, retired ids listed
  - _Requirements: 1.1, 1.2, 1.6, 1.7, 1.9, 1.10, 9.8_
- [ ] 7. Subsidiary replica store in `op-cozo-store`: open a subsidiary's own Cozo DB with `new_persistent_private`, relations `subsidiary_link`, `subsidiary_anchor`, `chain_vector_link`, `replica_meta`; idempotent `apply(edges, seq)` in one transaction; `rebuild_from(main)` using the same edge encoder
  - [ ] 7.1 Tests: re-applying is a no-op, `applied_seq` advances atomically with edges, rebuild and incremental apply give byte-equal dumps, `ensure_distinct_from` refuses a shared handle
  - _Requirements: 7.1, 12.3, 12.9_
- [ ] 8. Bridge resolution, tagging and main edges in `PersistentCore` (owner approval required)
  - [ ] 8.1 `SubsidiaryResolver::for_scope` called in `append` before `compute_hash`; `Public → None`; resolver error fails the append closed
  - [ ] 8.2 Link index written to the main ledger Cozo in `accept_metadata`; rebuilt in `walk_records` on open and compared; mismatch latches `failure`
  - [ ] 8.3 Write `/run/op-dbus/subsidiary-principals.json` and `subsidiary-ceilings.json` atomically on registry change
  - [ ] 8.4 Tests: tagged append, untagged public append, tampered stored link latches failure, resolver failure writes nothing
  - _Requirements: 1.4, 2.1, 2.5, 2.6, 3.1, 3.4, 12.1, 12.2_
- [ ] 9. Replica dual-write and reconciliation in the bridge (owner approval required)
  - [ ] 9.1 After the main edges commit, apply the same edges to the subsidiary's replica; on failure keep the main write, mark `lagging`, emit one lag event per episode, never fail the call
  - [ ] 9.2 Per-subsidiary reconciler: catch up from `applied_seq + 1` with bounded back-off, then `current`; reconcile all non-retired replicas on open
  - [ ] 9.3 Divergence: mark `diverged`, stop writes, rebuild into a new directory from the main ledger, rename-swap, quarantine the old copy under `host/diverged/`, record both digests
  - [ ] 9.4 Tests with a fault-injecting replica: failed write → call succeeds and `lagging`; reconciler → `current`; tampered edge → `diverged` → rebuilt equal to main
  - _Requirements: 12.1, 12.4, 12.5, 12.6, 12.7_
- [ ] 10. Capability ceiling check for subsidiary callers in bridge admission, and refusal of protected tasks for subsidiary principals in `task_elevation`; tests that the check only narrows
  - _Requirements: 5.2, 5.4, 11.3_
- [ ] 11. `subsidiary` sealed plugin skeleton in `op-plugins/src/state_plugins/subsidiary.rs`
  - [ ] 11.1 Schema, methods (including `reconcile_replica`, `rebuild_replica`), capabilities and subids from the design table; test all subids with `is_valid_subid`
  - [ ] 11.2 Scope rule: self vs owner (by id or name), cross-subsidiary "not found", owner-only methods refused for subsidiaries
  - [ ] 11.3 Add `org.opdbus.v1.subsidiary.{revoke,retire,destroy-data}` to `elevation::protected_actions()` with tests
  - _Requirements: 6.2, 6.3, 6.4, 10.1, 11.4_
- [ ] 12. Lifecycle methods: `register` (name validation, chain id, own Cozo DB, Qdrant collection `pending`/`ready`), `rebind_container`, `checkpoint`, `prepare_anchor`, `record_anchor` (events tagged `subject`), with tests against a fake `lxd` operation tree for label and `volatile.uuid` checks and a fake Qdrant client
  - _Requirements: 1.1, 1.3, 1.5, 1.6, 1.8, 1.9, 1.10, 1.11, 2.2, 3.5, 4.5, 7.8, 9.2_
- [ ] 13. Storage lifecycle methods: `provision_storage` (volume with `cozo/`, `tenant/`, `host/`), `attach`, `detach`, `snapshot`, `export_bundle`, `revoke` as `lxd.update_instance` disk-device plans (`tenant` with `shift: true`, idmap fallback), state machine enforcement, blake3 digests; tests that `cozo/` and `host/` are never in a device payload, that the only devices are the child projection and `tenant`, that `tenant/` snapshots (and any host open of `tenant/`) refuse while attached, and that move/export refuse while the replica is not `current`
  - _Requirements: 6.1, 6.5, 7.1, 7.2, 7.3, 7.4, 7.5, 7.6, 7.7, 7.9, 5.6, 12.8_
- [ ] 14. Read methods: `get_identity`, `get_head`, `list_activity`, `rollup`, `rollup_parent`, `export_receipts`, `prove_receipt`, `verify`, all answered from the main ledger Cozo, with paging bounds; tests for salt inclusion only in own/owner exports, no other subsidiary's data, and no replica reads
  - _Requirements: 3.6, 4.4, 4.6, 8.1, 8.2, 8.3, 8.4, 8.5, 12.2_
- [ ] 15. Door tagging and ceiling in `op-mcp-door`: optional `subsidiary_id` on `AuditRecord`; principal map and ceiling file readers (mtime-cached like `DurableGrants`); refusal in grant/ticket consumption outside the ceiling; tests
  - _Requirements: 2.4, 5.3_
- [ ] 16. Revocation of grants and tickets: revoke every ephemeral grant and unconsumed ticket for the principal and write a removal grant-delta proposal; tests that no grant document is written
  - _Requirements: 5.1, 5.5, 5.6_
- [ ] 17. Per-subsidiary Qdrant collections in `op-cognitive/src/chain_vectors.rs`
  - [ ] 17.1 `ensure_subsidiary_collection(name)` reusing `ensure_collection` (1024, cosine, idempotent, collision refusal)
  - [ ] 17.2 Per-block routing in `ingest_received`, `reindex_from_chain` and the manual origin path (tagged → `<name>__mainchain`, untagged → shared), one watermark, gap probe per routed collection; retired subsidiaries skipped
  - [ ] 17.3 Subsidiary points use `ledger_point_id(event_hash)`; `chain_vector_link` rows gain `collection` and `chain_id` kinds; `upsert_session_ledger` routes subsidiary principals the same way
  - [ ] 17.4 `search_session_events` selects the collection from admission (subsidiary → own only; others → shared; owner → by name)
  - [ ] 17.5 Tests with a fake Qdrant client: routing table, no subsidiary point in the shared collection, `pending` → `ready`, read-only after retire, skip on reindex
  - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5, 9.6, 9.8, 12.1, 13.2_
- [ ] 18. Footprint metadata key `subsidiary_id` in `event_to_footprint` and routing in `chain_vector_projector.rs` (inert until both are linked; Open Question 10)
  - _Requirements: 2.4, 9.3_
- [ ] 19. Retirement and destruction: `retire` (stopped container, both devices detached, replica `current`, collection `read_only`), `destroy_data` after retention (volume and collection deleted, final digest and point-count event, main ledger edges untouched); tests that ledger events and main Cozo edges are never removed and the name stays reserved
  - _Requirements: 9.7, 10.2, 10.3, 10.4, 10.5, 11.5_
- [ ] 20. FUSE terminal receipt carries optional `subsidiary` (shared FUSE contract; owner approval and Open Question 14); door `terminal_receipt` test that the extra key is accepted
  - _Requirements: 2.4, 4.4_
- [ ] 21. Chain edge relations in `op-cozo-store`: `subsidiary_chain_edge` and `subsidiary_chain_point` schemas, closed relation vocabulary and typed node-id constructors, `replace_chain_edges(chain_id, event_hash, rows, point_row)` as one transaction (rm by `(chain_id, src)` + put), `mark_point_indexed`, `pending_points`; same API on the subsidiary replica
  - [ ] 21.1 Tests: unknown relation refused, re-ingest byte-equal, no argument/result text accepted as a value
  - _Requirements: 13.1, 13.2, 13.8_
- [ ] 22. Same-pass ordering in `op-cognitive/src/chain_vectors.rs`: per subsidiary block, main edges + `pending` row → Qdrant upsert (wait) → `indexed` → replica apply; watermark only past `indexed`; `pending` rows resolved from Qdrant by id on the next pass
  - [ ] 22.1 Tests with fault injection at each step (fake Qdrant, in-memory Cozo)
  - _Requirements: 13.3, 13.4, 13.5, 13.6, 13.7, 12.1_
- [ ] 23. Extract the shared inventory code from `op-cozo-store/src/bin/inventory/mod.rs` into library module `op-cozo-store/src/inventory/` (Builder, hashing, secret handling, runit/shell scanner, consumers, catalog reader, edge merge) with no behaviour change; golden test that `system_graph_inventory --prepared` output is byte-equal
  - _Requirements: 15.1, 15.3_
- [ ] 24. Node attribute contract and ownership: `owner`, `change_authority`, `domain`, `container` builders, the section 14 resolution table, ownership edges (`owned_by`, `unix_owner`, `unix_group`, `change_authorized_by`); tests that every node produced by every collector carries them
  - _Requirements: 16.1, 16.2, 16.3, 16.4, 16.5_
- [ ] 25. Inventory collectors (read-only, bounded, limitations on gaps)
  - [ ] 25.1 packages (package-manager detection per root; dpkg `status`/`info/*.{list,md5sums,conffiles}` default, pacman local DB where present, limitation if neither), binaries (ELF sha256, build-id, DT_NEEDED → shared libraries), crates (`.dep-v0` or release `source:` graph)
  - [ ] 25.2 runit services and log services, configs and consumers, secret references by path only, live sealed catalog and blobs
  - [ ] 25.3 host collector producing `inventory:host` (`ObservedRuntime` provenance) and container collector reading the base rootfs and overlay volume from the host, never `tenant/`, never executing in the container
  - [ ] 25.4 Fixture tests for each collector, including an assertion that secret files are never opened
  - _Requirements: 15.1, 15.2, 15.3, 15.4, 15.5, 15.7, 15.8, 6.5_
- [ ] 26. Topology graph: registry, lxd, identity/grants, FUSE view and volume collectors; assemble `topology:subsidiary:<name>` with the host-inventory dependency closure; write with `replace_system_graph` to main then replica with equal receipts; record revision/digest as a tagged ledger event; lifecycle, release/reseal and periodic refresh triggers with one retry on revision conflict; `baseline:subsidiary:<name>` frozen at provisioning
  - [ ] 26.1 `subsidiary.topology_*` and `refresh_inventory` read/refresh methods over the fixed system-graph queries, scoped per caller
  - [ ] 26.2 Tests: bounds pass, receipts equal, replica mismatch rebuilt from main, lifecycle event → new revision, subsidiary caller limited to its namespace
  - _Requirements: 14.1, 14.2, 14.3, 14.4, 14.5, 15.5, 15.6, 12.1, 12.7_
- [ ] 27. Drift: compare `inventory:host` with the active `release:<id>` namespace and the live catalog manifest, and each container inventory with its baseline; write `drift:*` namespaces with `drifts_from` edges and kinds; tagged ledger event with counts and digest; `subsidiary.drift` read method; fixture tests for every drift kind and that nothing is blocked
  - _Requirements: 17.1, 17.2, 17.3, 17.4_
- [ ] 28. Allowed resources data and validation in `op-cozo-store` + `subsidiary` plugin
  - [ ] 28.1 `subsidiary_resource_kind` (seeded with agent, persona, workflow, notebook, chat_mode, ui_generator, tool, model, skill bound as in design section 16) and `subsidiary_allowed_resource` relations in the main Cozo, replicated via the task 9 path
  - [ ] 28.2 Validation: infra set refusal (kind and entry), ceiling check, `excluded_capabilities`, id existence via each kind's `id_source` read
  - [ ] 28.3 Methods `list_resource_kinds`, `list_allowed_resources`, `propose_allowed_resources`, `add_resource_kind` (protected); every change a tagged ledger event
  - [ ] 28.4 Grant-delta generation: capability union + merged `ArgsConstraint` `enum` per argument key, add/remove proposal only; `proposed` → `active` when the materialized grants carry it
  - [ ] 28.5 Tests: seeded kinds, infra/ceiling refusals, golden grant-delta, no grant document written, subsidiary caller sees only its own list
  - _Requirements: 18.1, 18.2, 18.3, 18.4, 18.5, 18.7, 18.10_
- [ ] 29. Enforcement through the existing gate (owner approval required, Open Question 25)
  - [ ] 29.1 Optional `args` on durable grant entries (door `ArgsConstraint`/`PropertyConstraint` shape, moved to a shared crate or mirrored) evaluated in bridge admission and door `DurableGrants`; absent = unchanged behaviour
  - [ ] 29.2 Tests: old grant documents admit identically; `args` present refuses out-of-set `workflow_id`/`notebook_id`/`mode`/`model`; no new check outside the gate
  - _Requirements: 18.6, 18.10_
- [ ] 30. Allowed resources in topology and drift: `<kind>:<id>` nodes with owner/domain/change authority, `allowed_<kind>` and `requires` edges in `topology:subsidiary:<name>`; `allowlist_grant_mismatch` drift kind; fixture tests
  - _Requirements: 18.8, 18.9, 14.2, 16.1, 17.2_

