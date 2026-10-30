# Network topology

**Authority:** owner clarification in the active 2026-10-16 network session. This file replaces the NotebookLM export that previously occupied it. That export described a host user-space WireGuard listener, a host SSH listener, and GRE rollback commands. Those statements are withdrawn.

This page is the topology. The OVS uplink cutover below is live as of 2026-10-16T21:32:58Z. OpenFlow and host Qdrant are still unset. `rtly-lungs` is `10.0.0.2` peer `192.168.255.2`.

## Settled layout

`eth0` is the OVS physical uplink. `pub0` owns public host addressing. Administrative SSH does not use `pub0`.

NetworkManager gets disabled and stays installed. Normal operation does not use it. It remains available for emergency recovery only.

Administrative SSH enters through WireGuard on the decoy at `10.0.0.1` and follows the reverse tunnel to the host identified as `10.0.0.2`. There is no WireGuard on the host. `wgcf` belongs on the decoy.

The decoy listener admits administrative SSH only from WireGuard. Temporary public recovery SSH stays until a fresh reverse-path connection succeeds. After that success, public administrative access is removed.

`rtly-lungs` replaces the former `tun5` name. It is point-to-point tonic `10.0.0.2` peer `192.168.255.2`. `192.168.255.2` is the SSH address, and that address is REALITY. MASQUE and SSH are the protocols that carry it, not its name. HTTPS is the payload inside that reverse SSH tunnel. It is not an OVS port.

`ovsbr0` itself has exactly three service IPv4 aliases: EMQX, OpenFlow, and host Qdrant. There are no per-service interfaces.

The shared container socket path is `/run/3tched-lungs.socket`. That guest entrypoint points at the host backing socket `/run/opdbus/lungs/lungs.sock`. Shared directories follow the seed subvolume layout:

- `@`
- `@agent_memory`
- `@agent_rules`
- `@agent_shared`
- `@agent_skills`
- `@agent_soul`
- `@agent_user`

Private writable state stays identity-scoped.

## Two transports

These paths are distinct.

| Path | Where it terminates | Carrier |
| --- | --- | --- |
| Administrative reverse TCP tunnel | Decoy WireGuard `10.0.0.1`, then the reverse tunnel to the host identified as `10.0.0.2` | WireGuard on the decoy, then reverse TCP. The live decoy entry observed on 2026-10-16 is `10.0.0.1:2222` bound to `wg-opdbus`, proxied to decoy loopback `127.0.0.1:2222`, and reversed to the host loopback SSH listener. |
| `rtly-lungs` | Host `10.0.0.2` peer `192.168.255.2` | MASQUE and SSH are the protocols. The SSH address `192.168.255.2` is REALITY. HTTPS is the payload inside the reverse SSH tunnel. |

MASQUE and SSH are protocols, not interface names. They carry `rtly-lungs`. This path is not GRE. It is not the administrative reverse TCP tunnel.

## Address ownership

| Place | What it owns |
| --- | --- |
| `eth0` | OVS physical uplink. After cutover it carries no host address. |
| `pub0` | Public host addressing. The recorded public address is `188.68.58.237/22` via `188.68.56.1`. |
| Decoy WireGuard | Administrative admission at `10.0.0.1`. |
| `rtly-lungs` | Point-to-point tonic `10.0.0.2` peer SSH/REALITY `192.168.255.2`. |
| `ovsbr0` | Exactly three service aliases, on the bridge device: EMQX, OpenFlow, and host Qdrant. |

The 2026-10-16 staging session left the numeric OpenFlow address and the numeric host Qdrant address unset. This document does not assign them. The REALITY address is the SSH address `192.168.255.2` on `rtly-lungs`. The retired `10.200.0.0/24` range is not available for the unset aliases.

`deploy/config/network.conf` still records a single bridge alias, `10.0.0.3/32`, and comments that OpenFlow is inert. That file predates this three-alias rule. Updating it belongs to the network-staging owner. Until that update, `10.0.0.3/32` remains the only alias written in that config; it is the historical EMQX/MQTT alias, not proof that OpenFlow and host Qdrant aliases exist on the machine.

The temporary validation pair `192.168.254.1`/`192.168.254.2`, page `10.0.0.4`, and `192.168.255.1` are not this link. `rtly-lungs` is `10.0.0.2` peer `192.168.255.2`.

## Bootstrap and control

The owner authorizes native network bootstrap because `op-grpc-bridge` and application D-Bus operations are not deployed. After the bridge is deployed, ongoing network control returns to plugin-owned D-Bus.

`op-ovsbr0-setup --seed-only` brought the bridge up at 2026-10-16T21:32:58Z from `/var/lib/cargo-build/odbus-verified/x86_64-unknown-linux-gnu/release/op-ovsbr0-setup`. `eth0` is the uplink. `pub0` carries `188.68.58.237/22` with uplink MAC `36:f1:18:56:c4:6a`. The default route is via `188.68.56.1` on `pub0`. `ovsbr0` at that moment carried `10.0.0.2/32` (tonic), `10.0.0.3/32` (EMQX), and `10.0.0.4/32` (page). No `svc0` port was created. OpenFlow and host Qdrant still have no assigned numbers. Tonic `10.0.0.2` later moved to `rtly-lungs`.

NetworkManager is disabled and still installed (`network-manager` 1.52.1-1devuan1). The runit service has a `down` file, the daemon is stopped, and the sysv link is `K01`. The package and service definition remain for emergency recovery.

The bridge rows persist in `/etc/openvswitch/conf.db`, and `openvswitch-switch` starts at boot (`S03`). Linux addresses do not persist in that database. `/etc/init.d/ovsbr0-persist` (`S04`, after Open vSwitch) restores `188.68.58.237/22` and the default route on `pub0`, and `10.0.0.3/32` and `10.0.0.4/32` on `ovsbr0`, from `/etc/op-dbus/ovsbr0-persist.conf`. It does not put tonic `10.0.0.2` back on the bridge. This boot hook is installed. It has not been proven by a reboot.

Decoy MASQUE reports Connected and healthy. Its SSH TUN service repeatedly fails with `IPv4 rule ownership differs`. That guard stays in place.

Decoy maintenance uses the existing `jeremy` `decoy_key` and strict pinned host trust.

## Source defaults

The 2026-10-16 source alignment removed the retired addresses from current defaults:

- `op-ovsbr0-setup` seeds no `svc0` port unless `INTERNAL_PORTS` is set.
- An empty OpenFlow attach reports that the controller address is unset.
- `deploy/runit/op-grpc-bridge/run` does not export a Qdrant URL. The shuttle default is `127.0.0.1:6334`.
- `deploy/runit/gre-decoy/run` and `deploy/runit/wgcf-egress/run` exit without creating those interfaces. The previous scripts are `run.retired`.
- `deploy/config/nftables.conf` does not accept `10.200.0.0/24`.
- `deploy/xray.service` does not put an address on `eth0`.

OpenFlow and host Qdrant numbers are still unset. REALITY is the SSH address `192.168.255.2`. `tonic-svc0` remains a certificate filename.

## What is live on 2026-10-16

The host has `lo`, `eth0` (uplink, no IPv4), `ovsbr0` (`10.0.0.3/32`, `10.0.0.4/32`), `pub0` (`188.68.58.237/22`), and `rtly-lungs` (`10.0.0.2` peer `192.168.255.2`, altname `tun5`). The SSH carrier is not attached, so the interface is up with no carrier. Listeners `2223`, `8080`, and `8090` are absent. Public recovery SSH on port 22 is still enabled.

The administrative reverse path has been restored and a fresh authenticated connection returned `jeremy` on `3tched` with the host's authoritative ED25519 key. The external WireGuard-client leg was not exercised in that check. WireGuard-only admission at the decoy listener, and removal of public administrative SSH, remain the required end state after the next fresh reverse-path success that has a working rollback.
