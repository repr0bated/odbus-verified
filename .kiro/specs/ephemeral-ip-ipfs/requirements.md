# Requirements — Ephemeral IP addresses over the 3tchedFS IPFS exchange

Status: SPECIFICATION ONLY. Owner request of 2026-11-18 ("give ephemeral ip
addresses a shot with ipfs, it is built into the fusefs, spec only"). This
document authorizes no implementation, seal, reseal, activation, grant change,
network change, or service restart. Nothing on the live host changes.

Spec id: `ephemeral-ip-ipfs`
Spec home: `/srv/git/odbus-verified/.kiro/specs/ephemeral-ip-ipfs/`
Tracks: `WISHLIST.md` OD-127 (ephemeral egress addresses for identity
containers). This spec picks OD-127 option (3), IPFS/libp2p identity, as the
publication and naming layer, and keeps options (1) and (2) as the address
sources underneath it.

Related specs, which this one does not override:
`.kiro/specs/standalone-network/` (one OVS bridge, addresses on the bridge,
container has no IP, no second tunnel),
`.kiro/specs/3tchedfs-threetchedfs/` and
`.kiro/specs/3tched-fs-mount-consolidation/` (mount names, surfaces, call
files), `.kiro/specs/identity-bound-fuse-application-migration/` (identity on
the mount). The bridge-over-FUSE work in progress (Muse, `op-grpc-bridge`) is
a dependency, not a deliverable of this spec.

Naming is deliberate and is not changed here: `3tched` / `3tched_fs` is our
code surface; `threetched` / `threetched_fs` / `THREETCHED_*` is the
ZeroClaw-derived binary surface. This feature belongs to the `3tched` surface.

## 0. Vocabulary

- **Ephemeral address** — a short-lived network source address held on behalf
  of exactly one lease. It is never a container NIC address. The container has
  no IP (standalone-network design, "Addresses live on the OVS bridge").
- **Lease** — the record that binds one ephemeral address and one ephemeral
  peer ID to one owner for a bounded time. A lease has a scope: `connection`,
  `session`, or `agent`.
- **Owner** — the identity the lease is issued to: the mount identity of the
  3tchedFS mount the request was written through (`MountIdentity` in
  `crates/_3tched-fs/src/namespace.rs`), resolved to one identity sled
  (`ContainerIdentitySled` in
  `crates/op-plugins/src/state_plugins/identity_sled.rs`).
- **Address source** — where the underlay address comes from. Either a routed
  IPv6 prefix on the host (OD-127 option 1) or a WARP address on the existing
  decoy carrier (OD-127 option 2). Neither is a new tunnel.
- **Ephemeral peer ID** — a libp2p peer ID derived from a fresh Ed25519
  keypair generated per lease. It names the lease inside the private swarm.
  It is not the identity root and is not the WireGuard peer key.
- **Private swarm** — the existing 3tchedFS IPFS exchange
  (`crates/_3tched-fs/src/ipfs.rs`): rust-ipfs with libp2p pnet swarm key,
  TCP + noise only, bitswap/identify/ping, no DHT, autonat, pubsub, mDNS,
  relay, or upnp, gated by `/etc/op-dbus/3tchedfs-ipfs.enabled`.
- **Lease record** — the canonical JSON of one lease, published as a raw
  BLAKE3-256 block (CID from `ipfs::cid_for`).
- **Lease view** — a 3tchedFS `ViewManifest` (`crates/_3tched-fs/src/store.rs`)
  whose objects name the current lease-table object. Each generation is a new
  view with `parent_view` set to the previous one.
- **Lease plugin** — the sealed plugin that owns leases. Working name
  `ephemeral_address`. ASSUMED: no such plugin exists in
  `crates/op-plugins/src/state_plugins/` today; the name is a proposal.

## 1. Goals

1. Give each identity container, session, or agent run a source address that
   lives only as long as its lease and is not reused.
2. Make lease, renew, release, and resolve plain file I/O on the identity's
   3tchedFS mount, so it works through the bridge-over-FUSE path unchanged.
3. Use the IPFS exchange that already exists in 3tchedFS for publication and
   resolution, with no public IPFS participation.
4. Add no interface to the container, no second tunnel, and no change to the
   path MTU.

## 2. Non-goals

1. Giving a container a NIC, a MAC, an IP, DHCP, or a macvlan/ipvlan device.
2. Carrying IP packets over libp2p (no IP-over-IPFS, no TUN on top of the
   swarm, no WireGuard inside WARP inside anything).
3. Inbound reachability on an ephemeral address. The door stays the single
   inbound path (`op-tunnel-mux`, `10.0.0.2:8090`).
4. Joining, announcing to, or resolving through the public IPFS network, a
   public DHT, a public IPNS record, or a public gateway.
5. Replacing SID1, OIA1, or the identity sled as the identity root.
6. Choosing the obfuscation or VPN-detection cover (deferred by
   standalone-network Requirement 4.6).
7. Changing `op-grpc-bridge` or the 3tchedFS engine in this spec.

## Requirements

### Requirement 1: Leases are file I/O on the identity's mount

**User Story:** As an identity container, I want to lease and release an
address by writing a file on my 3tchedFS mount, so that I need no network
client, socket, or credential of my own.

#### Acceptance Criteria

1. WHERE the lease plugin is sealed THE SYSTEM SHALL project it on the `3tched`
   surface as `plugins/ephemeral_address/` with `operations/lease/`,
   `operations/renew/`, `operations/release/`, `operations/resolve/`, and
   `operations/status/`, each with `input.schema.json`, `output.schema.json`,
   `metadata.json`, and, on a lungs mount, a `call` file.
2. WHEN an identity writes a complete JSON request to a lease plugin `call`
   file and closes or flushes it THE SYSTEM SHALL validate the request against
   the sealed method schema and dispatch it as the mount identity through
   `org.opdbus.v1.PluginV1.CallWithOperationId`.
3. WHEN that dispatch completes THE SYSTEM SHALL make the method output
   readable on the same open file handle, as the existing `CallFile` contract
   does.
4. IF a request fails schema validation THEN THE SYSTEM SHALL return the
   validation error on that handle AND SHALL NOT allocate an address.
5. WHERE a mount has no lungs identity (catalog-only or view mount) THE SYSTEM
   SHALL expose no `call` file for the lease plugin.
6. THE SYSTEM SHALL NOT project the lease plugin on the `threetched` surface.

### Requirement 2: The owner is the mount identity

**User Story:** As the operator, I want every lease bound to the identity that
asked for it, so that an address always traces back to one sled.

#### Acceptance Criteria

1. WHEN a lease is requested THE SYSTEM SHALL take the owner from the mount
   identity that carried the call, never from a field in the request.
2. IF the mount identity is `Unbound` THEN THE SYSTEM SHALL refuse the lease.
3. WHEN a lease is issued THE SYSTEM SHALL bind it to exactly one identity
   sled `session_id`.
4. IF the owner's sled is not `active` or its `expires_at` has passed THEN THE
   SYSTEM SHALL refuse the lease.
5. WHILE a human principal's session is admitted by an OIA1 assertion THE
   SYSTEM SHALL NOT issue a lease whose expiry is later than that assertion's
   `expires_at`.
6. WHEN an identity reads lease plugin state THE SYSTEM SHALL show only that
   identity's own leases.

### Requirement 3: Allocation

**User Story:** As an identity, I want a fresh address and peer ID per lease,
so that two leases cannot be linked by their addresses.

#### Acceptance Criteria

1. WHEN a lease is issued THE SYSTEM SHALL generate a fresh Ed25519 keypair
   and derive the lease's peer ID from it.
2. WHEN a lease is issued from a routed IPv6 prefix THE SYSTEM SHALL choose a
   uniformly random interface identifier inside that prefix.
3. IF the chosen address has been leased before THEN THE SYSTEM SHALL choose
   again AND SHALL NOT reissue it.
4. WHEN a lease is issued from the WARP source THE SYSTEM SHALL use an address
   already held on the existing decoy WARP carrier, AND SHALL NOT create a new
   tunnel or interface to obtain it.
5. IF no address source is available THEN THE SYSTEM SHALL refuse the lease
   with a typed error AND SHALL NOT fall back to the host's public address.
6. THE SYSTEM SHALL keep the ephemeral private key on the host side of the
   egress path AND SHALL NOT write it to any FUSE file, plugin state, lease
   record, or log.
7. WHERE a per-owner lease limit is configured THE SYSTEM SHALL refuse a lease
   that would exceed it.

### Requirement 4: Lease lifetime and expiry

**User Story:** As the operator, I want every lease to end on its own, so that
no address outlives what it was for.

#### Acceptance Criteria

1. WHEN a lease is issued THE SYSTEM SHALL set `expires_at` to the requested
   TTL capped by the configured maximum TTL, the owner sled's `expires_at`,
   and any governing OIA1 `expires_at`, whichever is earliest.
2. WHEN `expires_at` passes without a renew THE SYSTEM SHALL mark the lease
   `expired`, withdraw its address from the egress path, and drop its key.
3. WHEN the owner writes `renew` before expiry THE SYSTEM SHALL extend the
   lease under the same caps AND SHALL keep the same address and peer ID.
4. IF a renew arrives after expiry THEN THE SYSTEM SHALL refuse it.
5. WHEN the owner writes `release` THE SYSTEM SHALL end the lease immediately.
6. WHEN a `connection`-scoped lease's connection closes THE SYSTEM SHALL end
   the lease.
7. WHEN the owner's session ends, the container stops, or the sled becomes
   inactive THE SYSTEM SHALL end every lease of that owner.
8. WHEN a lease ends for any reason THE SYSTEM SHALL keep a tombstone of its
   address so Requirement 3.3 holds.

### Requirement 5: Publication and resolution through the private swarm

**User Story:** As a trusted peer (host or decoy), I want to resolve a lease's
address and peer ID to its status, so that egress can be attributed without
asking the container.

#### Acceptance Criteria

1. WHEN the lease table changes THE SYSTEM SHALL write the table as a typed
   3tchedFS object and commit a new lease view whose `parent_view` is the
   previous lease view.
2. WHILE `/etc/op-dbus/3tchedfs-ipfs.enabled` is present and valid THE SYSTEM
   SHALL publish the new lease view and its objects as pinned raw BLAKE3
   blocks to the private swarm, using the existing `publish_view` path.
3. IF the IPFS switch is absent THEN THE SYSTEM SHALL still issue and enforce
   leases locally AND SHALL report `published: false` on each lease.
4. WHEN a trusted peer resolves a lease THE SYSTEM SHALL serve it by view id
   and verify every block against its CID and typed schema before use, as
   `fetch_view` does.
5. THE SYSTEM SHALL NOT publish a raw `session_id`, SID1, OIA1, WireGuard key,
   container name, or uid in a lease record.
6. THE SYSTEM SHALL NOT start a DHT, pubsub, mDNS, relay, autonat, or public
   bootstrap to publish or resolve a lease.
7. IF a fetched lease view does not verify THEN THE SYSTEM SHALL discard it
   AND SHALL keep the last verified lease view.

### Requirement 6: Host network fit without nested tunnels

**User Story:** As the owner, I want ephemeral egress to use the one bridge
and the one existing carrier, so that MTU stays what it is.

#### Acceptance Criteria

1. THE SYSTEM SHALL NOT add an interface, NIC, address, or route inside an
   identity container to realize a lease.
2. WHERE the routed IPv6 source is used THE SYSTEM SHALL source the lease's
   egress from the host bridge on the native uplink path, with no added
   encapsulation.
3. WHERE the WARP source is used THE SYSTEM SHALL carry the lease's egress on
   the existing decoy WARP carrier only, with no tunnel inside or around it.
4. THE SYSTEM SHALL NOT carry lease egress through the private swarm.
5. THE SYSTEM SHALL NOT change the MTU of `eth0`, `ovsbr0`, `wgcf-ingress`,
   or `rtly-lungs` to realize a lease.
6. THE SYSTEM SHALL NOT use macvlan, ipvlan, OVN, an Incus proxy device, or a
   second OVS bridge to realize a lease.
7. WHERE an ephemeral address is held on the host THE SYSTEM SHALL hold it on
   the bridge or as a local route on the host, never on a bridge member port.

### Requirement 7: Egress is bound to the lease

**User Story:** As an identity, I want my outbound connection to leave from my
lease's address, so that the address means what the lease says.

#### Acceptance Criteria

1. WHEN an identity opens egress with a lease id THE SYSTEM SHALL check that
   the lease belongs to the identity on that egress socket's peer credential.
2. IF the lease is not owned by that identity, is not active, or has expired
   THEN THE SYSTEM SHALL refuse the connection AND SHALL NOT use another
   address.
3. WHEN the connection is allowed THE SYSTEM SHALL bind its source to the
   lease's address.
4. WHILE the egress filter in `deploy/runit/ct-egress-proxy/ct_egress.nft` is
   loaded THE SYSTEM SHALL keep rejecting private, loopback, multicast, and
   host-local destinations for lease egress, for both address families.
5. WHERE an identity opens egress without a lease id THE SYSTEM SHALL keep the
   current ct-egress behavior.

### Requirement 8: Bridge-over-FUSE compatibility

**User Story:** As the owner, I want this to ride the bridge-over-FUSE work,
so that the control path is the same file calls as everything else.

#### Acceptance Criteria

1. WHEN the bridge carries calls as FUSE I/O THE SYSTEM SHALL carry lease
   methods as the same call-file writes and reads, with no lease-specific
   transport.
2. THE SYSTEM SHALL NOT pass egress bytes through FUSE files or plugin state.
3. WHERE the bridge-over-FUSE work is not yet merged THE SYSTEM SHALL NOT
   require changes to `op-grpc-bridge` beyond registering the lease plugin's
   backend.

### Requirement 9: Observability and audit

**User Story:** As the operator, I want to see leases and their history
without exposing who owns them publicly.

#### Acceptance Criteria

1. WHEN a lease changes state THE SYSTEM SHALL record one event with lease id,
   state, address, peer ID, and timestamps.
2. WHEN host root reads lease plugin state THE SYSTEM SHALL show every lease
   with its owner session.
3. THE SYSTEM SHALL log through the service's runit log service AND SHALL NOT
   remove or disable that log service.
