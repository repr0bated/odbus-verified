# Historical SSH TUN inside MASQUE

OD-104 was deployed on 2026-10-04 using the owner's explicit, task-specific
native operator maintenance approval. This does not widen the mounted wgcf.read
capability. Application identities, TLS credentials and existing wgcf keys remain.

**Owner correction, 2026-10-17:** the target is the existing decoy wgcf carrier,
not a second MASQUE carrier. Xray moves externally. The historical procedures
below are not authorization to recreate MASQUE, GRE or a host WireGuard peer.

## Current host-local endpoint prerequisite

The restored host uses independent runit `op-warp-endpoint` only to restore
the local endpoint declared in the existing `/etc/op-dbus/warp-ssh.json`.
Its run script calls `endpoint.sh up`; its read-only check verifies the
persistent TUN's declared owner, alias, MTU and exact local/peer addresses.
It does not start SSH, MASQUE, WARP, firewall or policy-routing helpers.
The current receipt is local rtly-lungs/tun5,10.0.0.2 peer192.168.255.2,
UP/NO-CARRIER: that is sufficient for local address ownership, not evidence
of a remote carrier or authenticated application ingress.

This base prerequisite stays outside application overlay ownership. The
historical carrier procedures and receipts below do not authorize restoring
their retired components or changing the working management route.


## Retained wgcf selected-egress policy

`../decoy/decoy-fabric-egress` restores the existing IPv4 selector at priority41:
packet mark `0x51820` selects table51820. It requires that table's existing
`wgcf-ingress` default and refuses a foreign selector or a non-WARP route.
An unreachable default at metric32766 prevents selected traffic falling through
to the physical default when the interface disappears. Unmarked management
traffic and the peer-return route remain unchanged.

`--policy-only` applies just that routing repair, preserving packet-classification
rules and counters. The default also applies the existing fabric marks and MSS
clamp. `../decoy/systemd/decoy-fabric-egress.service` invokes this helper instead
of the Xray-coupled `decoy-egress warp`; it does not start or restart Xray.
Run `bash deploy/decoy/test-fabric-egress.sh` from the repository root for isolated
kernel routing, management preservation, idempotency, foreign-selector refusal
and fail-closed link-loss checks.

This selected-egress repair does not replace the private application carrier:
the existing classifier exempts10/8, and old private routes may still name the
SSH TUN. Public-VPS reachability is not proof of access to10.0.0.2:8090. Record
live policy application separately from receipt-backed persistent deployment.


## Traffic and address ownership

The prepared configuration names the interface `rtly-lungs`.
MASQUE and SSH are the protocols that carry it, not its name. OpenSSH still attaches using numeric TUN ID 5 through the kernel
alternative name `tun5`; that alias is an attachment detail, not GRE.
Existing installations without `interface_name` retain their previous name
until a deliberate migration updates routes, firewall matches and readiness
checks together. The endpoint helper refuses to rename an attached carrier.
The historical deployment evidence below predates this rename and does not
establish that it has been activated on either node.

The official MASQUE daemon runs in network namespace `op-warp`, with private
DNS mounts and blocked host DNS-control sockets. This confines the client's
routing and firewall changes. Root `warp-uplink` (192.168.253.1) supplies the
carrier; root `warp-data` (192.168.252.1, fd42:7761:7270:1::1) supplies application
traffic. Namespace NAT44/NAT66 applies only at WARP egress; carrier NAT applies
only to its uplink veth. Private SSH TUN packets retain their source addresses.

- Mark 0x51820 selects existing wgcf-ingress, table 51820.
- Private host destinations select rtly-lungs, table 28051.
- Remaining IPv4/IPv6 select warp-data, table 28053.
- Exact carrier/bootstrap, local-network and inbound-reply exceptions use table
  28050. The physical egress guard rejects other traffic.
- Public DNS uses 1.1.1.1/1.0.0.1 on warp-data; the namespace retains its own
  metadata resolver for bootstrap.

The decoy opens restricted SSH to host port 2223 through MASQUE. Dedicated
warp-link accounts, a pinned host key and a TUN-5-only key are used. The listener
permits no shell sessions, passwords, root login or ordinary port forwarding.
SSH protects the inner payload from Cloudflare. The Cloudflare-to-host public
leg is SSH, while the decoy-to-Cloudflare carrier is MASQUE.

Host `rtly-lungs` is point-to-point `10.0.0.2` peer `192.168.255.2`.
`10.0.0.2` is tonic. `192.168.255.2` is the SSH address, and that address is REALITY.
HTTPS is the payload inside this reverse SSH tunnel.
Decoy `rtly-lungs` is the reverse pair, `192.168.255.2` peer `10.0.0.2`.
The temporary `192.168.254.1`/`192.168.254.2` pair, page `10.0.0.4`, and `192.168.255.1` are not this link.
GRE services/interfaces and stale host DNAT for decoy `10.0.0.1` are retired.
MQTT `10.0.0.3` stays on `ovsbr0`. There is no `svc0` port and no `10.200.0.0/24` fabric.

## Release and lifecycle

Network-component snapshots are prepared and transferred with Btrfs send/receive.
`/opt/op-dbus/warp-ssh-current` selects this component independently of the
application `/opt/op-dbus/current`. Credentials and supervision stay outside the
read-only snapshot. Decoy uses a dedicated Btrfs image because its root is ext4.
No application binary was hand-installed.

Host uses runit `op-warp-ssh-host` and `op-warp-ssh-routes`. Received host run
inputs select rtly-lungs for door readiness, replace op-web's GRE dependency and stop
wgcf from recreating the duplicate door address. Updated OVS configuration owns
only its retained internal addresses. Subsequent application releases must
preserve this topology and the separate component selector.

Decoy uses the native systemd units in `systemd/` and the final `warp-svc.conf`
drop-in. `warp-svc-pilot.conf` is test-only. PartOf/Wants propagate daemon restarts
to SSH. Preparation orders after the existing selected-egress setup and configures
DNS on warp-data. The component is separate from the application golden service
inventory; these files are not a replacement deployment script.

## Helpers and receipts

Helpers use `/etc/op-dbus/warp-ssh.json` and `/run/op-warp-ssh`. Disposable
fixtures override OP_WARP_SSH_CONFIG and OP_WARP_SSH_STATE_DIR.

- endpoint.sh: persistent owned TUN and unique aliases (up/check/status/down).
- namespace.sh: owned namespace/veths, scoped carrier allowances, NAT and data
  failure guard (prepare/check/down). Teardown refuses foreign state/resident PIDs.
- admission.sh: exact scoped entries in existing base chains (install/check/remove).
- guard.sh: SSH carrier output-interface check (install/check/remove).
- decoy-egress.sh: selected wgcf preservation, physical exceptions/guard and
  retirement of temporary GRE exceptions.
- remainder.sh: dual-stack routes and terminal unreachable defaults; supports
  interface recreation and explicit rollback.
- routing.sh: private-prefix policy after actual peer health (activate/check/rollback).
- connect.sh: verifies route/guard and drops to the dedicated transport account.

Receipts verify exact owned kernel state and refuse unrelated changes. An UP
persistent TUN alone does not prove SSH connectivity. Failure guards and private
routes remain across disconnects; rollback is deliberate.

## Verification and recovery

Live checks passed actual IPv4/IPv6 WARP traces, selected wgcf packet forwarding,
1280-byte DF packets both ways, DNS over warp-data, page HTTP 200, and trusted door
TLS with expected anonymous 401. With MASQUE disconnected, remainder traffic was
blocked in both families while wgcf and management remained reachable. Restarting
the official daemon restored SSH and the door in eight seconds. GRE was removed
only after replacement proof. Whole-machine reboot and a new authenticated
application login were not tested.

Native isolated fixtures cover ownership, NAT44/NAT66, failure guards, interface
recreation, partial failure and cancellation. Repository /verify stopped at
unrelated formatting drift in identity_transactions.rs; Clippy/tests did not run.
See HANDOFF.md for exact receipts and fixture paths.

Original live files and maintenance evidence are retained in root-only
`/var/lib/op-warp-ssh/cutover-20261004` on both nodes. Seven older runit definitions
are archived separately in `retired-services-20261004` on host. GRE definitions
and enable links are retained for recovery, not enabled.

After GRE retirement, routing.sh rollback alone does not restore connectivity.
First restore the archived GRE definitions/interfaces/routes and original
transport addresses, removing conflicting tun5 aliases. Restore the saved
firewall/configuration inputs, then deliberately remove the owned private policy.
Use native console/management and verify the complete path before stopping SSH
or MASQUE. Never flush unrelated routes/rules or restore/change identity keys.

Sources: [OpenSSH TUN](https://man.openbsd.org/ssh#SSH-BASED_VIRTUAL_PRIVATE_NETWORKS),
[sshd restrictions](https://man.openbsd.org/sshd_config),
[WARP Linux client](https://developers.cloudflare.com/warp-client/get-started/linux/),
[client firewall requirements](https://developers.cloudflare.com/cloudflare-one/team-and-resources/devices/cloudflare-one-client/deployment/firewall/).
