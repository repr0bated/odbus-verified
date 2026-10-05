# Shared agent files and the chatbot display alias

## Provisioning correction, 2026-10-05

The owner requires one shared read-only physical seed and a private writable
leaf per container, without snapshots or copied container roots. The new
seven-sibling layout and source-only provisioning change are described in
[Shared physical seed and private leaves](shared-seed-provisioning.md).
The delivered contents and snapshot procedures below are historical; they do
not authorize restoring the old seed contents or cloning roots during provisioning.

## Historical delivered workspace, 2026-09-30

The owner narrowed the immediate deliverable to the subvolume layout,
`AGENTS.md`, and already available skills. Content development, runtime loading,
account reset, display-number allocation and container reprovisioning are deferred.

Actual host path: `/var/lib/opdbus-seeds/agent` on `/dev/sdb2`.
Container-root path when this base is used: `/agent`.

| Subvolume | ID | Subvolume property | Current content |
|---|---:|---|---|
| `/agent` | 258 | `ro=false` | `AGENTS.md` symlink to `rules/AGENTS.md` |
| `/agent/soul` | 259 | `ro=true` | Empty, reserved |
| `/agent/rules` | 260 | `ro=true` | `AGENTS.md` |
| `/agent/shared` | 261 | `ro=true` | Four existing skills under `skills/` |
| `/agent/user` | 262 | `ro=false` | Empty, reserved |
| `/agent/memory` | 263 | `ro=false` | Empty, reserved |
| `/agent/skills` | 264 | `ro=false` | Empty, reserved for local skills |

The **whole partition is mounted read-only**, including the subvolumes whose
own property allows future writable leaf use. No Incus device was added for
this layout. Parent snapshots do not carry the contents of the six nested
subvolumes: snapshot and send every required member explicitly for transport.

`AGENTS.md` SHA-256 is
`feed8ca7b302eb44a9f270b59ae884cb3c662b148447c94c520b7a0047c40a48`.
It matches `deploy/config/agents/workspace/rules/AGENTS.md`. Existing skills
were copied unchanged from `deploy/config/zeroclaw-skills/default`: `plugin-schema`,
`mutation-dispatch`, `ovs-network-triage`, and `qdrant-collections`. All four
copies match their source. No hook or script was activated.

Receipts: `/var/tmp/opdbus-agent-subvolumes-20260930/` and
`/var/tmp/opdbus-agent-skills-20260930/`. Read-only recovery snapshots are
`.seed-recovery/pre-agent-layout-20260930` (base root before new layout) and
`.seed-recovery/pre-ready-skills-20260930` (the shared subvolume before skills).

The existing `threetched-fs` executable and disabled vendor service template
remain in the seed at the paths recorded below. The running archived chatbot
does not consume this base yet. Its older manual `/opt/agents` bind is retained;
future provisioning source no longer adds that bind for these static files.

Source inspection confirms the pinned native runtime currently reads host
workspaces under `/var/lib/tched-router/agents/<alias>/workspace`, not `/agent`.
Its normal prompts load workspace Markdown; the separate stateless generation
path omits those files. Skill compact/on-demand loading is supported but not
configured. These are later integration tasks, not claims that placing the
workspace has activated them.

## Live setup, 2026-09-30

The owner-started container
`b0c777e3-0955-cd8b-3d01-ca12998cae28-archived-20260928` is running. Its
display metadata is `user.display_name=3tched-ctl` (updated from
`3tchedChatbot` at the owner's request). Its Incus name was not
changed. The owner is still confirming that this instance is the intended
Control Plane execution environment; the display alias does not establish
that runtime binding.

The requested numbering convention reserves `0` for this chatbot and `1` for
the owner's identity. That allocator/reprovisioning work remains deferred;
the live Incus instance still uses the UUID-based name below. The display-label
change was read back while the instance remained Running, without restart.

The persistent `agents` disk device is:

| Setting | Value |
|---|---|
| Host source | `/var/lib/opdbus-runtime/agents` |
| Container path | `/opt/agents` |
| Ownership mapping | `shift=true` |
| Container access | `readonly=true` |

The bundle contains `AGENTS.md`, `README.md`, `conf/`, `hooks/`, `scripts/`,
and `docs/`. The operator edits the host copy; private runtime files continue
to belong in the identity's existing persistent storage.

The mount was added in place, without stopping or reprovisioning the container:

```sh
incus config device add \
  b0c777e3-0955-cd8b-3d01-ca12998cae28-archived-20260928 \
  agents disk source=/var/lib/opdbus-runtime/agents \
  path=/opt/agents shift=true readonly=true
```

Readback showed the persistent device, an actual Btrfs mount at `/opt/agents`
with `ro` and `idmapped` options, and matching host/container `AGENTS.md` SHA-256
`8245a550294d13b2fc2086b6ad8d5fb3eb81e4f0962645eeab1c5b0cc897d469`.
The instance remained Running, PID 18816. No restart was used to test persistence.

## Copied 3tchedFS binary and container service

At the owner's subsequent request, `/usr/local/bin/threetched-fs` is now a real
copy inside this container, owned by root with mode 0755. It was transferred
from the already installed host release with `incus file push`, checked at a
temporary path, then renamed into place. The temporary binary bind device was
removed. This did not install a newly built host binary or restart anything.

Host and guest SHA-256:
`bef3929ffd0647e4b3549d3fd8f3a3e9787e10351ba98d75c2bf9703b45abede`.
The guest executable returns `3tchedfs 0.1.0`, and its `--help` lists both `mount`
and `mcp`. Use this exact path: the host also has a different older executable
under `/usr/local/sbin`, which must not be selected accidentally through PATH.

This Ubuntu container uses systemd, although the host uses runit. The template
`/etc/systemd/system/threetched-fs@.service` is installed and passes the
container's `systemd-analyze verify`. The unit for the canonical session
`threetched-fs@b0c777e3-0955-cd8b-3d01-ca12998cae28.service` is loaded but
**disabled and inactive**. Its versioned template is in the shared bundle at
`conf/threetched-fs@.service`.

The service requires scoped catalog/state inputs and a usable identity-bound
lungs connection under `/opt/run-mounts/3tched-inputs`. These are not attached.
The existing host `/run/opdbus/lungs.sock` has mode 0600 and explicitly accepts
only host UID 0; this unprivileged container's root is a different host UID.
Merely binding that socket into the container does not provide working calls.
Do not remove that check or substitute another identity's credential. The
existing host-side identity-root lifecycle instead runs the lungs daemon as
host root and bind-mounts its FUSE filesystem into the identity. A container-side
service needs an implemented identity transport before activation.

The owner's mount readback confirms `/opt/run-mounts/persist` is a Btrfs mount
from `identities/chatbot/fstorage.img`. `/dev/fuse` is available, but there is
still no actual mount at `/opt/run-mounts/3tched`.

This document is also saved at
`/var/lib/opdbus-runtime/agents/docs/control-plane-container-2026-09-30.md`,
visible inside the container under `/opt/agents/docs/`.

## Finding the container

The `jeremy` user's Incus client now has this command alias:

```sh
incus alias add list 'list -c n,user.display_name:ALIAS,s46tS'
```

Normal `incus list` displays an ALIAS column. Only the running archived chatbot
has the `3tchedChatbot` display value. Other users, including root, can display
the same metadata without configuring an alias:

```sh
incus list -c n,user.display_name:ALIAS,s46tS
incus list user.display_name=3tchedChatbot
```

The value is display metadata, not an alternate name accepted by `incus exec`
or `incus start`. Continue using the exact instance name for those commands.

## Provisioning and release boundary

**Owner correction, 2026-09-30:** the executable and shipped configuration,
instructions, hooks and scripts belong in the **read-only identity OS seed** so
they upgrade together. This supersedes the earlier proposed automatic guest
copy and copy-once defaults. The live archived container's manual copy above
is a historical repair, not the new provisioning design.

| Content | Container location | Upgrade behavior |
|---|---|---|
| 3tchedFS executable | `/usr/local/bin/threetched-fs` | Supplied by the seed |
| Versioned agent defaults | `/usr/share/op-dbus/agents` | Supplied by the same seed |
| Container service template | `/usr/lib/systemd/system/threetched-fs@.service` | Supplied by the same seed; disabled until transport is ready |
| Explicit shared operator overrides and receipts | `/opt/agents` | Existing persistent host bind; not overwritten |
| Private identity data, Cozo and runtime configuration | `/opt/run-mounts/persist` and owning runtime paths | Preserved outside the seed |

This applies to shipped configuration defaults. Generated live configuration,
credentials, private memory and explicit operator overrides remain with their
existing owners. Consumers must explicitly choose overrides; provisioning does
not merge arbitrary files or copy all defaults into the mutable bundle.

The owner explicitly chose **manual filesystem maintenance**, not a new seed
preparation script. The proposed script was removed. Golden still stages the
versioned agent defaults; no startup copying into writable leaves is intended.

At 2026-09-30T15:20Z, `/dev/sdb2` (Raccoon UUID
`2dcac52c-fb0b-418a-95a4-cb6eacc80209`) was remounted writable at
`/var/lib/opdbus-seeds`, a read-only rollback snapshot was taken, and the three
seed paths in the table above were installed. The partition was then remounted
**read-only**, verified by findmnt. Binary `--version` and `mount --help` ran
successfully using the seed's own dynamic loader/libraries via chroot. Every
installed file's SHA-256 was checked. No container or service restarted.

Receipt: `/var/tmp/opdbus-sdb2-seed-20260930T1520/`. Rollback snapshot:
`/var/lib/opdbus-seeds/.seed-recovery/pre-3tchedfs-20260930T1520`.
Only the known executable, agent defaults and disabled vendor service template
were installed; no credentials, private Cozo data or live Xray config were copied.

**Storage design clarification:** the owner wants Btrfs seed-and-sprout leaves,
not overlayfs. The live Raccoon partition is currently an ordinary Btrfs
filesystem mounted read-only: its superblock flags are `0x1 (WRITTEN)`, without
`SEEDING`. The current `op-identity-root` script still uses an overlay lower and
per-identity `upper/work/cozo`; it has not been converted. The two UUID-named
identity-root instances are stopped, and the running archived chatbot uses a
separate Btrfs root in `3tched-storage` on `/dev/sda4`. No seed consumer was
mounted in any readable mount namespace during the maintenance.

For actual Btrfs seeding, `btrfs device add` initially creates the writable
sprout. Later starts remount the existing leaf; `device remove` relocates its
data and is not ordinary shutdown. A seed used by existing sprouts must remain
unchanged; clearing its SEEDING flag and modifying it invalidates dependents.
See the [Btrfs seeding documentation](https://btrfs.readthedocs.io/en/latest/Seeding-device.html).
Disposable verification on host kernel 7.2.3 found that sequential reuse of one
seed works, but simultaneously mounting both sprouts from that same seed device
fails. Separate reflink-derived seed images with distinct FSIDs pass simultaneous
mount and independent cold-remount checks; both seed hashes remain unchanged.
Receipts: `/var/tmp/opdbus-btrfs-seed-share-20260930/experiment.log` and
`/var/tmp/opdbus-btrfs-distinct-seeds-20260930/experiment.log`. All fixture loop
devices and mounts were cleaned up. This is not a live conversion.

The owner subsequently chose filesystem subvolumes for these static files.
The proposed `agents` reconciliation command and extra Incus bind provisioning
were removed from source; the earlier manual bind is only a historical live
setup, preserved until that archived container is deliberately migrated.
The service template remains opt-in: enabling a systemd unit is separate from
provisioning files, and requires usable identity transport and scoped inputs.

This is the current identity-root/overlay lifecycle. The older Rust seeded-root
provisioning path has a separately pinned device contract; do not add a device
there by bypassing its validation or claim that this shell change updates it.

The source lifecycle edits have not been activated. The archived container's
earlier manual binary, `/etc` unit and shared files remain intact. The owner
has authorized fresh reprovisioning of the human and Control Plane identities
if needed and empty private state after a recovery backup. No account records,
leaf devices or containers have been deleted. Requested display numbers are
`0` for the chatbot, `1` for the owner, then monotonically allocated numbers for
anonymous signup; internal UUID/public-key identities remain separate.

Mounting agent files does not auto-load `AGENTS.md`, enable hooks, mount
3tchedFS, or index chat history. Those require explicit runtime integration.
This receipt documents the setup for later searching; private chats remain in
their existing scoped Cozo store.

## Removing only this setup

```sh
incus config device remove \
  b0c777e3-0955-cd8b-3d01-ca12998cae28-archived-20260928 agents
incus config unset \
  b0c777e3-0955-cd8b-3d01-ca12998cae28-archived-20260928 user.display_name
incus alias remove list
```

These commands retain all files in the host bundle. Remove the client alias
only if it still matches the alias recorded above; preserve subsequent edits.

## Internet transport inventory

The owner chose to retain the **NIC-less** layout. No NIC, Incus proxy device,
GRE link or new IP route is to be added to this container for this setup.

The seed is for identity roots. The `xray`, `mail-3tched` and `qdrant` service
containers use separate roots on sda4 and do not consume sdb2. Their networking
and filesystems are outside this seed change. The running archived chatbot
also retains its historical root until the deferred migration.

The owner clarified that key reuse means the existing **WireGuard identity
keypairs**, not Xray Reality keys. Preserve the peer public key and its
existing identity mapping. Client private keys remain on their clients;
WireGuard terminates at the decoy. Preparing a local socket relay does not
require creating, moving or rotating these keys.

Identity-root creation already uses `--no-profiles` and adds only disk devices.
Provisioning now checks effective expanded devices before reconciling or
starting an identity, rejecting both NICs and proxy devices even when inherited
from profiles. The older workspace provisioner checks actual device types
rather than names and checks reused instances too. It fails without silently
editing a shared profile. Existing service-container networks are separate.

The running container has only loopback and an empty IP routing table.
Its shared `ghostbridge/decoy.sock` is served by `xsock-decoy`: a Unix socket
forwarder to `129.80.64.127:443`. That is one raw stream destination, not a
general-purpose HTTP/SOCKS proxy. The web socket separately reaches op-web.
The container's proxy environment variables are unset. The old NetMaker egress
socket is absent, and its host proxy service is retired; do not restore that
retired service as a shortcut. The shared Xray Reality socket is an inbound
publication path, not an outbound proxy for this container.

An HTTP/SOCKS proxy over a Unix socket can provide application-level internet
access while retaining the NIC-less layout, but the proxy/client protocol must
be configured. An IP route cannot name a Unix socket as its gateway. A GRE
alternative requires an IP path into the container plus routes on both ends;
the existing host `gre-decoy` tunnel does not create such a container route.
No proxy, GRE, firewall, route or Xray configuration was changed by this work.

The owner's suggested [`nmrs`](https://github.com/freedesktop-rs/nmrs) fits
operation-dbus as a Rust client for NetworkManager's D-Bus API. It requires
the NetworkManager daemon and opens a system-bus connection; it does not
replace that daemon or forward internet traffic. The repository currently
has generic D-Bus discovery scaffolding but no dedicated NetworkManager
plugin or `nmrs` dependency. A future integration must retain plugin-owned
schemas, caller grants and MutationEngine state publication.

`networkctl` instead controls systemd-networkd, which also exposes D-Bus through
[`org.freedesktop.network1`](https://github.com/systemd/systemd/blob/main/man/org.freedesktop.network1.xml).
Networkd is already running in the archived identity container. The repository's
`net` plugin mentions networkd but its implemented backend uses OVSDB and
rtnetlink; its two typed methods do not currently implement networkd control.
Neither network manager supplies the HTTP proxy that carries internet bytes
over the identity's Unix socket.

The older `rust-network-mgr` in `deploy/config/container-socket-contracts.json`
is a different project, pinned to
`117087cb1bf99cb55ba3e8e40b9e27752cd08f46`. Its
[Unix socket handler](https://github.com/sparesparrow/rust-network-mgr/blob/117087cb1bf99cb55ba3e8e40b9e27752cd08f46/src/socket.rs)
accepts control commands, not proxied traffic. The historical Netmaker internet
path instead used `nm-warp-egress-proxy` on loopback port 13128 and
`/run/ghostbridge/NetMaker/egress.sock`. Its runit definition depended on the
retired `wgcf-egress` route. This is evidence of the earlier loopback/socket
design, not a current working upstream.

Container relay source now lives in
`deploy/config/agents/conf/op-identity-egress.{socket,service}`. The socket
listens on `127.0.0.1:3128`; the service uses the seed's existing
`systemd-socket-proxyd` to forward to the absolute Unix socket path assigned in
`/etc/op-dbus/identity-egress.env`. The configuration example assigns no
upstream. These are container units, not host service definitions. An upstream
HTTP proxy with HTTPS CONNECT support still needs to be provided and verified
before enabling the socket or setting application proxy variables.

The unit pair is installed **disabled** in the seed's
`/usr/lib/systemd/system`, with copies and the unset configuration example in
`/usr/share/op-dbus/agents/conf`. The seed was returned to read-only. Receipt:
`/var/tmp/opdbus-seed-egress-20260930/`; recovery snapshot:
`.seed-recovery/pre-identity-egress-20260930`. No application proxy environment,
upstream gateway, service enablement or container restart was applied.

The seed's `systemd-analyze verify` accepts both units. A disposable mount and
network namespace test passed loopback-to-UDS HTTP forwarding (HTTP 200 and
`pong`); missing socket validation failed as expected. This proves the local
relay, not HTTPS CONNECT handling or an actual internet gateway. Evidence:
`/var/tmp/op-egress-finalprobe.Yzg2w6/`.

The visible `gre0`, `gretap0` and `erspan0` entries are inactive kernel default
tunnel devices. Each reports DOWN, remote/local `any`, and no address. Incus's
expanded devices contain no NIC or proxy. The container has network namespace
`4026532723`; the host has `4026531833`. The configured, UP `gre-decoy` belongs
to the host namespace. Sharing a kernel does not make that host interface an
interface in the container. Linux registers these default tunnel devices per
network namespace in its [GRE driver](https://github.com/torvalds/linux/blob/master/net/ipv4/ip_gre.c).

## Replacement order and dependency audit

The owner subsequently resumed replacement planning: provision and verify new
identity containers **before** manually deleting old containers or stale
identity-owned Cozo records. Do not add cleanup logic to provisioning scripts.
No container or Cozo record has been deleted by this work. The earlier request
to empty old records is subordinate to keeping the bridge working through
cutover. Central authentication records are distinct from per-identity data.

Observed bridge PID1123 keeps `TCHED_ROUTER_RUNTIME_ENDPOINT` set to
`http://127.0.0.1:8084`; model execution uses the host tched-router process.
Its environment selects chatbot session
`b0c777e3-0955-cd8b-3d01-ca12998cae28` and principal
`1262e982-14c1-44d4-0771-7ce74213211b`. Display labels do not change these
selectors or exact-principal grants. Startup can activate the canonical UUID
instance, which is currently Stopped; the Running archived instance is a
different Incus name. Verify the canonical binding and an authenticated chat
call before any cleanup.

Bridge startup also opens central identity Cozo and recovers its committed
identity state. Human-key registration is in a separate principal store; those
records support authentication with the retained WireGuard keys. The generic
`cozo` plugin serves the cognitive store, not either central identity authority.
The existing `delete_identity_session` example deletes a legacy generic
session row; it is not a complete identity reset. The existing identity-sled
delete helper refuses versioned records and omits revision/provision journals.
The current public provisioning method requires an absent identity and no
prior provisioning journal. None is a verified atomic replacement path.

Dependency observations are complementary:

- The live public `ovsdb_bridge` state was read with `busctl`: bridge `ovsbr0`,
  ports `3tched`, `eth0`, `ovsbr0`, `pub0`, and `svc0`. The subscribed OVSDB
  observer publishes selected Bridge/Port/Interface topology through
  MutationEngine. The sealed blob supplies its contract; changing state goes
  to `/dev/shm/opdbus/state/ovsdb_bridge.json`. This does not inventory arbitrary
  service configuration or NIC-less identity containers.
- `docs/crate-graph/crate-graph.{json,cozo}` contains source dependencies,
  including binaries and runit declarations. `scripts/hooks/pre-commit`
  regenerates it on qualifying staged changes. No broader golden-time graph
  updater was located in either checked repository's golden preparation.
  This does not establish what a separately populated live graph contains.
- The owner explicitly authorized use of their existing identity. The
  installed 3tchedFS client's peer D-Bus connection successfully bound human
  session `414fbc56-03fb-20ab-758a-bcc7a7cbdc0e` through `/run/opdbus/lungs.sock`.
  After the initial grant denial, the owner explicitly authorized adding
  `cap.data.cozo.relations.list@v1` to their account. Exactly that grant was
  added to the durable local grant document and source defaults; materializer
  readback verified every other entry unchanged. The authenticated retry
  succeeded (event203713), listing30 relations including `graph_node` and
  `graph_edge`. No credential/key was copied or minted. Graph row contents
  remain unverified; table existence does not prove a complete inventory.

## Source verification

Earlier fixtures at `/var/tmp/op-identity-agents.Wrzfez` and
`/var/tmp/op-identity-copy.LTqpsx` exercised the superseded copy-once approach.
The removed offline helper passed isolated Btrfs fixtures under
`/var/tmp/opdbus-identity-seed-check-20260930/`; that helper is not retained.
NIC/proxy rejection mocks are in `/var/tmp/opdbus-nic-guard-mock-20260930/`.
`sh scripts/check-workspace.sh verify` was attempted; it stops at existing
workspace-wide Rust formatting drift. Log:
`/var/tmp/opdbus-identity-seed-verify-20260930/verify.log` (268 existing format
diff blocks; Clippy and tests did not run). No full workspace pass or
backend release activation is claimed.

The later required verify attempt likewise stopped at unrelated formatting
drift: `/var/tmp/op-egress-verify-gate.2OuHvf.log`. Container unit validation and
the isolated forwarding probe above passed independently.
