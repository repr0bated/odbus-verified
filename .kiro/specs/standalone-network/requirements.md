# Requirements — Standalone network

Status: draft. Owner decisions of 2026-11-11. The host bridge is already
deployed from this spec (`/srv/git/network`, `ovsbr0`, `eth0` enslaved, both
addresses on the bridge, `10.0.0.2:8090` unbound). Decoy WireGuard, OIA1, and
live WARP are not done. This revision authorizes no further deploy. Chat and
the gate are not this spec; see `.kiro/specs/container-chat/` and
`.kiro/specs/3tched-core/`.

This spec covers two jobs only: establishing identity, and transporting that
identity. `10.0.0.2:8090` is a dumb port. It accepts nothing and interprets
nothing until the bridge, or another authoritative service, attaches and
becomes the door.

The network is a separate repository that deploys on its own. The repository
exists at `/srv/git/network`. The host bridge from this spec is deployed.
Decoy WireGuard, OIA1, and live WARP are not. The copy of this spec in that
repository can predate the 2026-11-11 ownership correction; this directory
is the source.

Spec id: `standalone-network`
Spec home: `/srv/git/odbus-verified/.kiro/specs/standalone-network/`

## Vocabulary

- **Decoy** — where WireGuard terminates. Tunnel address `10.0.0.1`, interface
  `wg-opdbus`.
- **Peer key** — the WireGuard public key. It is the cryptographic root of an
  identity. It is not a NIC, a MAC, or a host.
- **Peer proof** — the decoy's signed assertion for that key. On the wire this
  is OIA1 (`x-oracle-identity-assertion-bin`).
- **WARP** — `wgcf-ingress`. It transports the peer proof from the decoy to the
  host. It is not a listener and it is not the door.
- **Dumb TCP port** — `10.0.0.2:8090` with no listener. The network delivers the
  proof to that socket and does not speak a protocol on it.
- **Dumb subsidiary port** — an unaddressed OVS member of this same bridge.
  Nothing is attached until a container uses it as its network. It is not a
  second bridge and it holds no address.
- **Authoritative service** — the process that later binds the dumb port and
  becomes the door. The bridge is the expected one. Another service may attach
  instead. The network does not choose or start it.

## Requirements

### Requirement 1: The network deploys alone

**User Story:** As an operator, I want the identity path and its transport up
before any door exists, so that a service can attach later.

#### Acceptance Criteria

1. WHEN the network is deployed AND no authoritative service is attached THEN
   the network SHALL bring up WireGuard termination, peer-proof minting, and
   WARP transport.
2. WHEN no authoritative service is attached THEN `10.0.0.2:8090` SHALL be a
   dumb port: unbound, with no tonic listener and no protocol spoken by the
   network.
3. WHEN an authoritative service later binds `10.0.0.2:8090` THEN the network
   SHALL deliver to that socket without adding a second transport.
4. WHERE the network repository is built the repository SHALL contain no
   bridge crate, plugin catalog, container provisioner, door implementation,
   or 3tchedFS implementation.
5. IF a network deploy fails THEN the network SHALL leave any other repository
   and any already-attached service unchanged.

### Requirement 2: WireGuard establishes identity only

**User Story:** As a peer, I want WireGuard to prove who I am, so that the
identity is not the hardware I arrived on.

#### Acceptance Criteria

1. WHERE WireGuard is configured the network SHALL use it only to admit a peer
   key.
2. WHEN a peer connects THEN the network SHALL terminate that WireGuard session
   at the decoy.
3. WHEN a peer session is terminated at the decoy THEN the network SHALL keep
   that session up with the decoy as the peer.
4. IF the host is inspected THEN the network SHALL present no WireGuard
   interface and no WireGuard peer on the host.
5. WHEN an arrival has no admitted peer key THEN the network SHALL NOT forward
   an identity.
6. WHEN the same peer key arrives from different hardware THEN the network
   SHALL mint the proof for that key AND SHALL NOT bind the proof to a NIC, a
   MAC, or a host address.

### Requirement 3: The decoy seals the peer proof

**User Story:** As the service that will attach later, I want a signed proof
of the WireGuard peer waiting at the dumb port.

#### Acceptance Criteria

1. WHEN the decoy admits a peer key THEN the decoy SHALL mint one OIA1 peer
   proof for that key.
2. WHEN an OIA1 proof is minted THEN the proof SHALL identify that peer key and
   SHALL be signed by the decoy.
3. IF an arrival has no OIA1 proof THEN the network SHALL NOT forward it to
   `10.0.0.2:8090`.
4. WHERE a session credential is required the attached service SHALL create it,
   AND the network SHALL NOT mint SID1 or any session credential.
5. WHEN the decoy is unreachable THEN the network SHALL NOT substitute a proof
   minted on the host.

### Requirement 4: WARP transports the proof to the dumb port

**User Story:** As an operator, I want the sealed proof carried to one socket,
so that the host can receive the identity without running WireGuard.

#### Acceptance Criteria

1. WHEN an OIA1 proof has been minted THEN WARP SHALL carry that proof from the
   decoy to the host.
2. WHEN WARP delivers a proof THEN the network SHALL forward it only to
   `10.0.0.2` port `8090`.
3. WHERE WARP is configured the network SHALL use it only as that transport.
4. WHEN nothing is bound to `10.0.0.2:8090` THEN the forward SHALL fail closed
   AND the network SHALL NOT choose another address or port.
5. IF WARP is down THEN the network SHALL NOT open a second tunnel, a host
   WireGuard interface, a placeholder listener, or a public listener to
   replace it.
6. WHEN this spec is in force THEN the network SHALL NOT require an obfuscation
   or VPN-detection cover. That cover is a later specification.

## Out of scope

These belong to the service that attaches, or to a later spec. The network
does not do them.

- Becoming the door. Binding `10.0.0.2:8090`, speaking tonic, and admitting or
  refusing a proof.
- Minting SID1, anchoring a session, creating a container, or mounting
  3tchedFS. When the attached service is the bridge, the container is the
  identity. That is the bridge's job.
- Obfuscation and covering WireGuard detection.
- A Cloudflare Tunnel, a Mesh CIDR route, a public TLS listener on 443, or a
  second WireGuard hop on the host.
- EMQX. It is not part of identity establishment or transport.
- gRPC and tonic. They begin only when an authoritative service binds the dumb
  port. The bridge is that service. The network does not introduce them.
- OpenFlow on `10.200.0.1`, schema-tag flows, `lungs.sock`, and the Xray
  egress socket.
- Implementing 3tchedFS. The protocol is already decided: a container's own
  network is an unaddressed port on this OVS bridge. Calls to lungs and to
  the 3tched backend are disk I/O through the shared 3tchedFS. Not OVN. Not
  an Incus proxy. Not a second bridge. Not the hackathon gate `etched0`.
- A second internal port such as `pub0`, and DHCP on the enslaved NIC.

## Uplink

Every OVS bridge holds its addresses on the bridge. A member port stays
unaddressed. `eth0` is the member of this bridge. The public address
`188.68.58.237/22` and the dumb-port address `10.0.0.2/32` are set statically
on the bridge. The bridge MAC is `eth0`'s MAC, and the default route stays
`via 188.68.56.1` onlink on that bridge. DHCP is not used.

## Open questions

1. How the onlink default route is persisted on the Incus bridge. Incus's
   single `ipv4.address` does not by itself express a gateway outside the /22.
