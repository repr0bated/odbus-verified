# Requirements Document — `subsidiary-container`: the Subsidiary container contract

Status: SPEC ONLY, NOT APPROVED FOR IMPLEMENTATION. Owner request of
2026-11-19: document the container contract for the Kiroween "3tched
Subsidiary" project (public repo `repr0bated/subsidiary`). Provisioning is
pre-existing 3tched tooling; this spec fixes what the provisioned container
must be. Nothing is provisioned until after the current golden build and the
owner's go-ahead.

Owner decisions, 2026-11-22 (Kiroween hackathon; new development). These
take precedence over the requirements below where they differ:

- **LXD:** a subsidiary is remote or isolated, which is why it runs under LXD
  through the sealed `lxd` plugin. LXD 5.0.2 was
  installed on this host on 2026-11-23 at the owner's request (runit `lxd`,
  pool `lxd-pool`, no network or listener).
- **Privileges and grants:** a subsidiary is the same as root within its own
  space. It holds root-equivalent grants and capabilities scoped to itself,
  with no execution methods.
- **Not the chatbot model:** the chatbot is the only account with both a
  human and a machine identity. It has no execution, and root access needs
  two humans. A subsidiary does not copy that model.
- **Backend channel:** FUSE call files only, as in Requirement 4. No lungs
  socket and no bus.
- **Network:** no NIC. The OVN edge NIC in Requirement 2 is withdrawn. BGP
  stays possible without a NIC: a BGP session is TCP 179 over a socket, so a
  BGP speaker in the container can peer through a forwarded socket (an LXD
  `proxy` device) instead of an interface. That socket is a second channel
  besides the FUSE call files and conflicts with Requirement 4.2 (no host
  Unix socket) unless the owner exempts it or it is carried through the
  bridge. Undecided. Effects otherwise go through FUSE call files and the
  bridge's tracked egress.
- **Root filesystem:** the owner accepts the earlier Ubuntu seed, which has
  FUSE: `/var/lib/opdbus/containers/3tchedChatbot/seed.img`, Btrfs label
  `3tched-seed`, SEEDING flag set, subvolumes `@` and `@seed-ro`, Ubuntu
  26.04.1 LTS, `fusermount3` present. It boots systemd. The owner chose
  systemd inside containers (2026-11-22), so Requirement 8 (runit) is
  withdrawn for subsidiaries. Imported and sealed as
  `3tched-pool/images/@ubuntu-subsidiary` (no in-container bridge or door)
  and received read-only into the LXD ublk at
  `/mnt/cozo-btrfs-lxd/seeds/@ubuntu-subsidiary`.
- **Subsidiary marker:** "subsidiary" is the OSCAL-tagged
  `ContainerIdentitySled.org_relation` (`kind`, `name`, `parent`;
  `src.service.identity-sled.org-relation@v1`), not a label or the container
  name. Its `parent` may point anywhere and may be moved later.

Spec id: `subsidiary-container`
Spec home: `.kiro/specs/subsidiary-container/`
Source material: a fresh read of `origin/dev` `009e035`
(`crates/op-plugins/src/state_plugins/lxd.rs` — 319 typed LXD methods with
`cap.lxd.*@v1` capabilities; `deploy/runit/ovsbr0-persist/ovsbr0-persist.conf`
and `door.nft` — backend addresses; `deploy/incus-hooks/ovs-port-hook`;
`deploy/provisioning/*`) and a read-only listing of the live
`/run/mounts/3tchedFS/plugins/lxd/operations/` (319 operations) on antix1.

Related specs, which this one does not override:
`.kiro/specs/provisioning-system-subsidiary/` (the `--subsidiary` mode:
parent preflight, identity-stripped export, import into LXD through the
`lxd` plugin, child identity and grant-delta proposal — this spec builds on
it and adds the network, channel and hardening contract),
`.kiro/specs/identity-bound-fuse-application-migration/` (identity on the
mount; Wonder's shared FUSE contracts), `.kiro/specs/mcp-door-adapter/`
(tool classes and deny-list).

## Introduction

The Subsidiary is a child identity container that runs as its own Web3
instance. Its contract has three parts:

1. **Network**: one NIC, an OVN logical switch port on an edge/egress
   logical network with its own internet egress (OVN gateway router + SNAT).
   OVN ACLs deny all traffic to backend addresses and ports. There is **no**
   network path from the container to the backend — not to `ovsbr0`, the
   10.x backend addresses, the door, the bridge, EMQX, or the host's public
   address.
2. **Backend channel**: the **only** channel to the backend is a FUSE mount
   of the child identity's sealed call files
   (`operations/<plugin>/<method>/{call,metadata.json,input.schema.json,output.schema.json}`).
   Grants and capabilities are enforced backend-side by the existing bridge;
   the container adds no authority.
3. **Hardening**: unprivileged, no nesting, CPU/memory/process limits,
   read-only base with a writable overlay, a post-provision snapshot for
   rollback, runit with a log service for every service.

Every LXD step goes through the sealed `lxd` plugin's FUSE operations, never
raw `lxc`/`lxd`.

Out of scope: running provisioning; installing an LXD daemon or OVN; changing
the bridge, 3tchedFS / `threetched-fs`, FUSE code, `ovsbr0`, OpenFlow rules,
pools, the seed, the overlay, sealed blobs or grants; minting identities;
the Subsidiary GUI and Web3 code (in `repr0bated/subsidiary`).

## Requirements

### Requirement 1 — LXD only through the sealed plugin

**User Story:** As the owner, I want every LXD step to go through the sealed
`lxd` plugin, so that provisioning the container is itself governed and
receipted.

#### Acceptance Criteria

1. THE SYSTEM SHALL perform every LXD step (profile, network, ACL, instance, devices, snapshot, state) through `plugins/lxd/operations/<method>/call`.
2. THE SYSTEM SHALL NOT invoke `lxc` or `lxd` directly.
3. WHEN a needed LXD operation is absent from the sealed catalog THEN THE SYSTEM SHALL stop and report the gap, and SHALL NOT fall back to a native command.
4. WHEN a plan is produced THEN THE SYSTEM SHALL list each call as `lxd.<method>` with its JSON payload and the operation's `required_capability` read from its live `metadata.json`.
5. IF a call's outcome is uncertain THEN THE SYSTEM SHALL stop, record the operation id, and SHALL NOT retry.

### Requirement 2 — OVN edge port with independent egress

**User Story:** As the owner, I want the container's only NIC to be an OVN
port on an edge/egress logical network, so that it has internet access that
is independent of the backend network.

#### Acceptance Criteria

1. THE SYSTEM SHALL give the container exactly one NIC, attached to a dedicated LXD network of type `ovn` (an edge/egress logical switch) and no other network.
2. THE SYSTEM SHALL NOT attach the container to `ovsbr0`, any `lxdbr*`/`incusbr*` bridge, a macvlan, a physical NIC, or any OVS internal port created by `ovs-port-hook`.
3. WHEN the edge network is created THEN THE SYSTEM SHALL give it internet egress through the OVN gateway with SNAT, and SHALL NOT route it through the door, the decoy or WARP.
4. THE SYSTEM SHALL configure no inbound forwards, load balancers or peers on the edge network.
5. WHEN the container is running THEN its default route SHALL be the edge network's OVN router and SHALL be the only route.

### Requirement 3 — No network path to the backend

**User Story:** As the owner, I want the container to have zero network
reach to the backend, so that compromise of the container cannot touch the
control plane over the network.

#### Acceptance Criteria

1. THE SYSTEM SHALL attach an OVN ACL to the container's port that drops egress to every backend address and range in design "Backend deny set", on all ports and protocols.
2. THE SYSTEM SHALL drop ingress to the container except replies to connections it opened.
3. THE SYSTEM SHALL keep a host-firewall deny for traffic from the edge network's external (SNAT) address to the host and backend addresses, as defence in depth.
4. IF the ACL or the host-firewall deny is missing or differs from the design THEN THE SYSTEM SHALL refuse to start the container.
5. WHEN verification runs THEN THE SYSTEM SHALL show that connections from inside the container to each backend address and to the door/bridge/EMQX ports fail, and that a public internet address succeeds.

### Requirement 4 — FUSE call files are the only backend channel

**User Story:** As the owner, I want the identity's sealed call files to be
the container's only way to the backend, so that every action passes the
bridge's identity + grants + sealed check.

#### Acceptance Criteria

1. THE SYSTEM SHALL expose to the container exactly one backend channel: the child identity's 3tchedFS projection of sealed call files.
2. THE SYSTEM SHALL NOT attach the host `lungs` socket, the D-Bus session bus socket, the door tunnel, `/run/opdbus/ct/<name>`, the sealed catalog directory, the decoy trust store, or any other host Unix socket.
3. THE SYSTEM SHALL NOT grant the container any capability locally; admission SHALL remain `identity ∧ grants ∧ sealed` in the existing bridge.
4. IF the container holds no admitted child identity THEN THE SYSTEM SHALL attach no backend channel and SHALL leave the container stopped (`admission=pending`, as `provisioning-system-subsidiary` Req 8.3).
5. WHEN a method is not granted to the child identity THEN a call through its call file SHALL be refused by the bridge with the missing capability named.

### Requirement 5 — Unprivileged and confined

**User Story:** As the owner, I want the container locked down by default,
so that a compromised Subsidiary cannot escalate on the host.

#### Acceptance Criteria

1. THE SYSTEM SHALL set `security.privileged=false`, `security.nesting=false`, `security.idmap.isolated=true` and `boot.autostart=false`.
2. THE SYSTEM SHALL keep the default syscall deny list, deny compat syscalls, disable syscall interception and disable the guest API.
3. THE SYSTEM SHALL drop the Linux capabilities listed in design "Instance profile".
4. THE SYSTEM SHALL set `limits.cpu`, `limits.memory` (hard) and `limits.processes`, with design defaults when not given.
5. THE SYSTEM SHALL attach no device beyond the root disk, the overlay volume, the one NIC (Req 2) and the backend channel (Req 4).

### Requirement 6 — Read-only base, writable overlay

**User Story:** As the owner, I want the base system read-only with a
separate writable overlay, so that the base cannot drift and can be compared
to its source.

#### Acceptance Criteria

1. THE SYSTEM SHALL mount the container's base root read-only.
2. THE SYSTEM SHALL provide writes through a dedicated LXD custom volume used as the overlay upper layer for the paths listed in design "Overlay paths".
3. WHEN the overlay is reset THEN THE SYSTEM SHALL return the container to its base state without changing the base.
4. IF the base is found writable at start THEN THE SYSTEM SHALL refuse to start.

### Requirement 7 — Post-provision snapshot and rollback

**User Story:** As the owner, I want a known-good snapshot right after
provisioning, so that I can roll back cleanly.

#### Acceptance Criteria

1. WHEN provisioning completes and verification passes THEN THE SYSTEM SHALL create an instance snapshot named `provisioned-<stamp>` through `lxd.create_instance_snapshot`, with no expiry.
2. THE SYSTEM SHALL snapshot the overlay volume at the same point.
3. WHEN rollback is requested THEN THE SYSTEM SHALL restore both to the named snapshot through the `lxd` plugin while the container is stopped.
4. THE SYSTEM SHALL NOT delete the `provisioned-*` snapshot automatically.

### Requirement 8 — runit with log services

**User Story:** As the owner, I want runit as init with a log service for
every service, so that the container is supervised and every service's
output is kept.

#### Acceptance Criteria

1. THE SYSTEM SHALL use runit as the container's init (`lxc.init.cmd = /sbin/runit-init`, as identity containers today).
2. THE SYSTEM SHALL give every enabled service a `log/run` service writing with `svlogd` to the overlay.
3. IF an enabled service has no log service THEN verification SHALL fail.
4. THE SYSTEM SHALL disable in the container's own root (never in the seed) every seed service not in the Subsidiary service list, including the 3tchedFS, bridge, door, door-tls-init and nextdns services.

### Requirement 9 — Verification and dry-run

**User Story:** As the operator, I want the whole contract checked by a
dry-run and a read-only verifier, so that I can review it before anything
changes and audit it afterwards.

#### Acceptance Criteria

1. WHEN `--dry-run` is given THEN THE SYSTEM SHALL print every planned `lxd.*` call with its payload and capability and SHALL execute nothing.
2. WHEN verification runs THEN THE SYSTEM SHALL check Requirements 2–8 read-only and report each as pass/fail.
3. THE SYSTEM SHALL print no secret material in any output.
4. THE SYSTEM SHALL have offline tests with a fake operation tree that fail if `lxc`/`lxd` is invoked, a forbidden device is attached, or the ACL deny set is incomplete.
