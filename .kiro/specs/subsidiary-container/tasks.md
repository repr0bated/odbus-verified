# Implementation Plan

Spec only. Not approved for implementation; waits for the golden build and
the owner's go-ahead, and for Open Questions 1–4.

- [ ] 1. Test harness: extend `deploy/provisioning/tests/` with a fake `plugins/lxd/operations/` tree generated from the live operation list and a forbidden `lxc`/`lxd` stub
  - _Requirements: 1.2, 9.4_
- [ ] 2. Plan builder for `lxd.*` calls (payload validated against live `input.schema.json`, capability from live `metadata.json`, gap = stop)
  - [ ] 2.1 Implement in `deploy/provisioning/lib-plan.sh`
  - [ ] 2.2 bats: missing operation stops with a gap, uncertain outcome is not retried
  - _Requirements: 1.1, 1.3, 1.4, 1.5_
- [ ] 3. Edge network and NIC plan (`opdbus-edge`, `type: ovn`, one NIC, no forwards/LBs/peers)
  - [ ] 3.1 Implement the network and NIC steps
  - [ ] 3.2 bats: exactly one NIC, never `ovsbr0`/bridge/macvlan/physical, no inbound objects
  - _Requirements: 2.1, 2.2, 2.3, 2.4_
- [ ] 4. Backend deny ACL and its hash label
  - [ ] 4.1 Implement `opdbus-subsidiary-deny` rules from design "Backend deny set" as data
  - [ ] 4.2 bats: rule set complete and ordered; start refused on hash mismatch
  - _Requirements: 3.1, 3.2, 3.4_
- [ ] 5. Host-firewall table `opdbus_subsidiary` rendering and check (no change to `opdbus_door`)
  - _Requirements: 3.3, 3.4_
- [ ] 6. Backend channel device (child projection only) and forbidden-device refusal
  - _Requirements: 4.1, 4.2, 4.4_
- [ ] 7. Profile `opdbus-subsidiary` (exact key set, refuse-if-different) and limits defaults
  - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5_
- [ ] 8. Read-only root and overlay volume, stage-1 overlay mounts, reset via `base-empty`
  - _Requirements: 6.1, 6.2, 6.3, 6.4_
- [ ] 9. Post-provision instance + overlay snapshots and rollback plan
  - _Requirements: 7.1, 7.2, 7.3, 7.4_
- [ ] 10. runit service list, `down` files in the container root, `log/run` for every enabled service
  - _Requirements: 8.1, 8.2, 8.3, 8.4_
- [ ] 11. `40-verify.sh` read-only checks for Req 2–8 including in-container reachability probes, and the golden dry-run file
  - _Requirements: 3.5, 4.5, 9.1, 9.2, 9.3_
