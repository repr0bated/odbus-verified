# Implementation Plan — Ephemeral IP addresses over the 3tchedFS IPFS exchange

Status: SPECIFICATION ONLY. Dependency-ordered. Nothing in this list is
implemented, built, tested, sealed, or deployed by approval of the spec.
Tests, fmt, and Clippy run only after an explicit owner request, one cargo
process at a time (`.kiro/steering/subagent-cargo.md`). Reseal and
`./activate-golden.sh` are not tasks here. No task touches `op-grpc-bridge`
before the bridge-over-FUSE work lands, except task 4.

Spec id: `ephemeral-ip-ipfs`
Design: `design.md`. Requirements: `requirements.md` Requirements 1–9.

Each task: `implemented: no | built: no | deployed: no | tested: no`

## Phase 0. Owner decisions (blocks everything)

- [ ] 0.1 Answer the open questions in `design.md`
  - At minimum: address source (Q1), meaning of "ephemeral IP" (Q2), egress
    owner (Q3), and tombstone home (Q5).
  - Confirm or replace the working plugin name `ephemeral_address`.
  - _Requirements: 3.2, 3.4, 4.8, 7.1_

## Phase 1. Leases as file I/O, no network effect

- [ ] 1. Define the sealed lease plugin schema
  - [ ] 1.1 Add `crates/op-plugins/src/state_plugins/ephemeral_address.rs`
    - `EphemeralAddressState`, `LeaseRequest`, `Lease`, `LeaseRecord`, and
      the enums in `design.md`, styled like `tunnel.rs`
      (`deny_unknown_fields`, schemars, `x-oscal-subid`
      `sch.software.plugin.ephemeral-address.schema@v1`, category `network`).
    - Methods `lease`, `renew`, `release`, `resolve`, `status` with input and
      output schemas.
    - _Requirements: 1.1, 2.1, 5.5_
  - [ ] 1.2 Register the subid and its mutation subids
    - Mutation records carry `actor_id` and `capability_id`
      (`docs/subid-taxonomy.md` rules).
    - _Requirements: 9.1_

- [ ] 2. Project the plugin on the `3tched` surface only
  - Confirm `Namespace::build_lungs` in `crates/_3tched-fs/src/namespace.rs`
    gives the five `call` files with no engine change.
  - Confirm a view mount and a `threetched` mount do not.
  - _Requirements: 1.1, 1.5, 1.6_

- [ ] 3. Lease logic, in memory, no address applied
  - [ ] 3.1 Owner from mount identity, sled checks
    - Refuse `Unbound`, inactive, and expired sleds.
    - Show only the caller's leases; host root sees all.
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.6, 9.2_
  - [ ] 3.2 TTL caps, renew, release, expiry sweep
    - Expiry is the earliest of request, max TTL, sled `expires_at`, and OIA1
      `expires_at`.
    - _Requirements: 2.5, 4.1, 4.2, 4.3, 4.4, 4.5, 4.7_
  - [ ] 3.3 Fresh Ed25519 keypair and peer ID per lease
    - The private key stays in the host backend; nothing serializes it.
    - _Requirements: 3.1, 3.6_
  - [ ] 3.4 Allocation against a fixture prefix and tombstones
    - Random interface identifier, never reused, typed `no_address_source`.
    - The host public IPv4 is never returned.
    - _Requirements: 3.2, 3.3, 3.5, 3.7, 4.8_

- [ ] 4. Bridge backend registration, after bridge-over-FUSE lands
  - One module like `crates/op-grpc-bridge/src/runtime_tunnel.rs`
    (`supports(plugin, method)`), registered once.
  - No lease-specific transport. No egress bytes in state.
  - _Requirements: 1.2, 1.3, 8.1, 8.2, 8.3_

- [ ] 5. Phase 1 tests (only on owner request)
  - Unit tests in `design.md` Testing Strategy for lease logic, record
    privacy, and namespace projection.
  - _Requirements: 1.4, 2.6, 4.3, 5.5_

## Phase 2. Private IPFS publication

- [ ] 6. Lease table as a 3tchedFS view chain
  - Each change writes one typed object and a `ViewManifest` with
    `parent_view` set to the previous lease view.
  - _Requirements: 5.1_

- [ ] 7. Publish and resolve with the existing exchange
  - [ ] 7.1 Publish with `ipfs::publish_view`, unchanged
    - Gated by `ipfs::ensure_enabled` and `ipfs::read_swarm_key`.
    - Without the switch, leases still work and report `published: false`.
    - _Requirements: 5.2, 5.3, 5.6_
  - [ ] 7.2 Resolve with `ipfs::fetch_view`, unchanged
    - A view that fails CID or schema checks is discarded; the last verified
      view stays.
    - _Requirements: 5.4, 5.7_
  - [ ] 7.3 Long-lived publisher service (only after owner answers Q6)
    - runit `ephemeral-address-publish` in the mesh-only namespace, with a
      `log/run` using `_runit-log` and `/var/log/runit/`.
    - _Requirements: 5.2, 9.3_

- [ ] 8. Phase 2 tests (only on owner request)
  - Two in-memory private nodes with a test swarm key exchange a lease
    view; a tampered block is refused.
  - _Requirements: 5.4, 5.6, 5.7_

## Phase 3. Egress bound to the lease

- [ ] 9. Address source, per owner answer to Q1
  - [ ] 9.1 Routed IPv6 prefix as a host local route (AnyIP), not per-lease
    aliases and not on a bridge member port
    - No change to `eth0`, `ovsbr0`, `wgcf-ingress`, or `rtly-lungs` MTU.
    - _Requirements: 6.1, 6.2, 6.5, 6.7_
  - [ ] 9.2 WARP fallback on the existing decoy carrier only
    - No new tunnel, no per-lease WARP registration in this phase.
    - _Requirements: 3.4, 6.3, 6.4_

- [ ] 10. ct-egress honours leases (or the owner's chosen egress, Q3)
  - [ ] 10.1 Root-owned binding file from the lease plugin to the egress
    service; stale bindings fail closed
    - _Requirements: 4.2, 7.1, 7.2, 7.3_
  - [ ] 10.2 Extend `deploy/runit/ct-egress-proxy/ct_egress.nft` with an
    IPv6 block set matching `blocked4`, for lease-bound traffic only
    - Unleased egress keeps today's behavior.
    - _Requirements: 7.4, 7.5_

- [ ] 11. Phase 3 tests (only on owner request)
  - Isolated netns fixture in the style of
    `deploy/decoy-warp-ssh/test-namespace.sh`: source address matches the
    lease; unowned lease refused; private IPv6 destinations rejected; no
    interface, tunnel, or MTU change appears.
  - _Requirements: 6.1, 6.5, 6.6, 7.2, 7.3, 7.4_

## Phase 4. Live rollout (separate owner approval)

- [ ] 12. Seal, release, and enable
  - Only after phases 1–3 pass and the owner approves a deploy.
  - Not part of approving this spec.
  - _Requirements: 1.1, 5.2, 7.3_
