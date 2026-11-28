# Implementation Plan — Standalone network

The network repository does not exist yet. Do not create it, and do not copy
this spec, until these documents are approved. Tasks below are the work inside
that future repository. None of them bind a door, mint SID1, or create a
container.

- [x] 1. Create the network repository and copy this spec
  - Create the repository only after approval. Copy
    `.kiro/specs/standalone-network/` in unchanged.
  - The tree contains identity establishment and transport only. No bridge
    crate, catalog, container provisioner, tonic door, or 3tchedFS.
  - _Requirements: 1.4_

- [ ] 2. Encode the decoy WireGuard boundary
  - [ ] 2.1 Configure termination on `wg-opdbus` at `10.0.0.1`
    - The peer session stays at the decoy after termination.
    - The host configuration has no WireGuard interface and no WireGuard peer.
    - _Requirements: 2.1, 2.2, 2.3, 2.4_
  - [ ] 2.2 Test that an unadmitted arrival is dropped
    - No peer key means no proof and no forward.
    - _Requirements: 2.5_

- [ ] 3. Mint one OIA1 proof per admitted peer key
  - [ ] 3.1 Sign the proof on the decoy and bind it to the peer key
    - The same key from different hardware yields the same proof identity.
    - The proof carries no NIC, MAC, or host address.
    - _Requirements: 2.6, 3.1, 3.2_
  - [ ] 3.2 Refuse a host-minted substitute
    - If the decoy cannot sign, mint nothing and forward nothing.
    - Do not write SID1.
    - _Requirements: 3.3, 3.4, 3.5_

- [ ] 4. Transport the proof to the dumb port
  - [ ] 4.1 Carry the proof on `wgcf-ingress` and forward only to `10.0.0.2:8090`
    - WARP is transport. It is not a listener and not a front.
    - _Requirements: 4.1, 4.2, 4.3_
  - [ ] 4.2 Fail closed when the port is dumb or WARP is down
    - An unbound `10.0.0.2:8090` does not get a placeholder listener or another
      target.
    - A down WARP does not grow a second tunnel or a host WireGuard interface.
    - _Requirements: 1.2, 4.4, 4.5_

- [ ] 5. Prove the network comes up with nothing attached
  - Deploy the identity path and the transport in a fixture with no process
    bound to `10.0.0.2:8090`.
  - Assert the three components are up and the port has no listener.
  - Assert a later bind of that socket receives the existing forward and adds
    no second transport.
  - _Requirements: 1.1, 1.2, 1.3, 1.5_

- [x] 6. Enslave `eth0` and place both addresses on the Incus bridge
  - `eth0` becomes an unaddressed member. Static `188.68.58.237/22` and
    `10.0.0.2/32` land on the bridge. Bridge MAC is `eth0`'s MAC. DHCP and
    NAT stay off. No `pub0`.
  - Persist `default via 188.68.56.1` onlink on the bridge. Do not perform
    the live move until that route is in the same step as the address move.
  - Port `8090` stays unbound.
  - _Requirements: 4.2_
