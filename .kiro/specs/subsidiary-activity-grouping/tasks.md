# Implementation Plan

Spec only. Not approved for implementation; waits for the owner's go-ahead
and answers to the design's Open Questions (at least 1, 2, 3, 4, 9, 12, 14).
Tasks that change `op-grpc-bridge` or the `_3tched-fs` receipt contract need
explicit owner approval at that point, because both are shared, running
components. Order matters: readers before writers (design "Error Handling").

- [ ] 1. Tag types and hashing in `op-state-store`
  - [ ] 1.1 Add `SubsidiaryRole` and optional `subsidiary_id` / `subsidiary_role` to `ChainEvent` and `CanonicalEventPayload` with `skip_serializing_if = "Option::is_none"` (same pattern as `sealed_id_digest`); leave `LegacyCanonicalEventPayload` unchanged
  - [ ] 1.2 Tests: untagged events from fixed fixtures hash byte-identically to today; a tagged event's hash changes and `verify()` passes; serialized untagged events contain no new keys
  - _Requirements: 2.3, 11.1, 11.2_
- [ ] 2. Link chain module `op-state-store/src/subsidiary_chain.rs` (pure, no IO)
  - [ ] 2.1 `SubsidiaryLink`, per-id genesis, `link_hash`, salted `receipt_leaf`, keyed `anchor_tag`, blake3 Merkle root and proof
  - [ ] 2.2 Anchor payload type `op-subsidiary/anchor@1` with a constructor that only accepts hashes and integers
  - [ ] 2.3 Tests: distinct genesis per id, linkage break detected by seq, proof round-trip, a flipped byte fails, payload contains no plaintext id
  - _Requirements: 3.2, 3.3, 4.1, 4.2, 4.3, 4.6_
- [ ] 3. `SubsidiaryReceiptRef` on `MethodExecutionReceipt` and `MethodReceiptRecovery` (optional, omitted when absent), with tests that existing receipts serialize unchanged
  - _Requirements: 2.4, 4.4, 11.2_
- [ ] 4. Execution tracker fields: optional `executor_subsidiary_id` / `requestor_subsidiary_id` on `ExecutionContext` and `DelegationAttribution`; tests for old JSON still parsing and new JSON round-trip
  - _Requirements: 2.4, 11.2_
- [ ] 5. Offline verifier bin `op-subsidiary-verify` (receipt + proof + anchor payload → ok/fail, no network), with a golden test
  - _Requirements: 4.7_
- [ ] 6. Registry relations in `op-cozo-store` (`subsidiary`, `subsidiary_by_principal`, `subsidiary_by_instance`, `subsidiary_link`, `subsidiary_anchor`)
  - [ ] 6.1 Create/read/update functions with uniqueness checks on principal and instance UUID
  - [ ] 6.2 Tests on an in-memory `CozoGraphShuttle`: mint format, refusal of a second binding, rebind keeps the id, retired ids listed
  - _Requirements: 1.1, 1.2, 1.6, 1.7, 9.5_
- [ ] 7. Bridge resolution and tagging in `PersistentCore` (owner approval required)
  - [ ] 7.1 `SubsidiaryResolver::for_scope` called in `append` before `compute_hash`; `Public → None`; resolver error fails the append closed
  - [ ] 7.2 Link index written in `accept_metadata`; rebuilt in `walk_records` on open and compared; mismatch latches `failure`
  - [ ] 7.3 Write `/run/op-dbus/subsidiary-principals.json` and `subsidiary-ceilings.json` atomically on registry change
  - [ ] 7.4 Tests: tagged append, untagged public append, tampered stored link latches failure, resolver failure writes nothing
  - _Requirements: 1.4, 2.1, 2.5, 2.6, 3.1, 3.4_
- [ ] 8. Capability ceiling check for subsidiary callers in bridge admission, and refusal of protected tasks for subsidiary principals in `task_elevation`; tests that the check only narrows
  - _Requirements: 5.2, 5.4, 11.3_
- [ ] 9. `subsidiary` sealed plugin skeleton in `op-plugins/src/state_plugins/subsidiary.rs`
  - [ ] 9.1 Schema, methods, capabilities and subids from the design table; test all subids with `is_valid_subid`
  - [ ] 9.2 Scope rule: self vs owner, cross-subsidiary "not found", owner-only methods refused for subsidiaries
  - [ ] 9.3 Add `org.opdbus.v1.subsidiary.{revoke,retire,destroy-data}` to `elevation::protected_actions()` with tests
  - _Requirements: 6.2, 6.3, 6.4, 10.1, 11.4_
- [ ] 10. Lifecycle methods: `register`, `rebind_container`, `checkpoint`, `prepare_anchor`, `record_anchor` (events tagged `subject`), with tests against a fake `lxd` operation tree for label and `volatile.uuid` checks
  - _Requirements: 1.3, 1.5, 1.6, 2.2, 3.5, 4.5, 7.8_
- [ ] 11. Storage lifecycle methods: `provision_storage`, `attach`, `detach`, `snapshot`, `export_bundle`, `revoke` as `lxd.update_instance` disk-device plans (`shift: true`, idmap fallback), state machine enforcement, blake3 digests; tests that the host-only directory is never in a device payload, that the only devices are the child projection and `tenant-cozo`, and that snapshots (and any host open of the tenant DB) refuse while attached
  - _Requirements: 6.1, 6.5, 7.1, 7.2, 7.3, 7.4, 7.5, 7.6, 7.7, 7.9, 5.6_
- [ ] 12. Read methods: `get_identity`, `get_head`, `list_activity`, `rollup`, `rollup_parent`, `export_receipts`, `prove_receipt`, `verify` with paging bounds; tests for salt inclusion only in own/owner exports and no other subsidiary's data
  - _Requirements: 3.6, 4.4, 4.6, 8.1, 8.2, 8.3, 8.4, 8.5_
- [ ] 13. Door tagging and ceiling in `op-mcp-door`: optional `subsidiary_id` on `AuditRecord`; principal map and ceiling file readers (mtime-cached like `DurableGrants`); refusal in grant/ticket consumption outside the ceiling; tests
  - _Requirements: 2.4, 5.3_
- [ ] 14. Revocation of grants and tickets: revoke every ephemeral grant and unconsumed ticket for the principal and write a removal grant-delta proposal; tests that no grant document is written
  - _Requirements: 5.1, 5.5, 5.6_
- [ ] 15. Vector partitioning in `op-cognitive/src/chain_vectors.rs`: payload key and `chain_links` entry, `LedgerPoint.subsidiary_id`, keyword index in `ensure_collection`, caller-typed filters in `search_session_events`, delete-by-filter and retired-id skip in reindex/ingest; tests for filter construction
  - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5_
- [ ] 16. Footprint metadata key `subsidiary_id` in `event_to_footprint` (inert until `mutation_engine.rs` is linked; Open Question 10)
  - _Requirements: 2.4_
- [ ] 17. Retirement and destruction: `retire` (stopped container, both devices detached), `destroy_data` after retention with final digest event; tests that ledger events are never removed
  - _Requirements: 10.2, 10.3, 10.4, 10.5, 11.5_
- [ ] 18. FUSE terminal receipt carries optional `subsidiary` (shared FUSE contract; owner approval and Open Question 14); door `terminal_receipt` test that the extra key is accepted
  - _Requirements: 2.4, 4.4_
