# VPS checklist — avoiding known incompatibilities

Distilled from the 2026-08 flooding/mesh outage chain. Split by what you control
at **purchase time** (provider choice) and what must hold on **any host** you
run (design rules). Each item cites the incident that earned it.

---

## Part 1 — choosing a provider / box

| check | why | incident |
|---|---|---|
| VRRP / proto 112 / multicast **not filtered** on customer ports | If filtered, the gateway MAC can never be learned → `actions=NORMAL` floods every gateway-bound frame to all ports. Forces static FDB pinning forever. | netcup filters VRRP; root of the flooding saga |
| KVM/dedicated with `/dev/net/tun`, cgroup2, nftables, loadable modules | OVS userspace datapath, WireGuard, and firewalling all need them. Containers/shared kernels break OVS in subtle ways. | — |
| UDP hairpin to own public IP works (`nc -u <own-ip> <own-port>`) | Some clouds (OCI) blackhole an instance talking to its own public endpoint. Self-referential WG peers then look "up" but carry 0 bytes. | decoy-wg2 self-peers, `10.0.0.1` unreachable |
| Public NIC MTU ≥ 1420 | Stack is tunnels-within-tunnels (WG inside WARP). Every missing byte costs MSS-clamp hacks per-path. | `mangle` clamp 1240 for netmaker 1280 |
| rDNS/PTR settable; IPv4 **and** IPv6 delegated | Mail legitimacy; REALITY `dest` credibility. | — |

## Part 2 — design rules for any host we run

1. **Never pin OpenFlow flows to an ofport.** The wire format carries port
   *numbers*; netclient recreates interfaces and numbers churn (88→199 on
   2026-08-24 overnight, mesh SSH died while handshakes stayed green).
   Re-bind by interface name at runtime; a watchdog must do this, not a human.
   Implemented: `/usr/local/libexec/3tched/ensure-ptap-flow.sh`, called from
   the `wg-3tched` loop.
2. **Nothing critical lives only in tmpfs without a boot-time restorer.**
   The sealed blob catalog lives in `/dev/shm`; when the stager regressed
   (`opblob stage-shm` missing from the installed binary), op-grpc-bridge
   crash-looped all boot and took the mesh 8090 door down with it.
3. **Every address has an owner service with a watchdog.** "netclient will
   assign it" is not ownership. `100.69.0.1` vanished mid-run once already;
   `wg-3tched` re-adds it every 10s and recreates the whole interface if the
   link disappears.
4. **One flow-table mutation path per flow class.** Controller-installed or
   watchdog-installed, never both drifting. Today's PTAP flow is explicitly a
   *stopgap* owned by the watchdog until `rovs-openflow` gains
   `packet_type` + `Encap` and the controller installs it itself.
5. **After any control-plane restart, verify the data plane.** WG handshakes
   prove only the outer tunnel. Inner packets can be black-holed at the bridge,
   firewall, or listener while every status looks green. The 60-second ritual:
   ```sh
   ovs-ofctl dump-flows -O OpenFlow15 ovsbr0 | grep packet_type  # counter moves?
   ip -s link show netmaker            # tx errors not climbing?
   ss -tln | grep '100.69.0.1:22'      # sshd still bound to the mesh IP?
   # from a peer:
   timeout 4 bash -c "</dev/tcp/100.69.0.1/22" && echo mesh-ssh-ok
   ```

## Known stacked firewalls on this host (check both after edits)

- `/etc/nftables.conf` — inet filter, policy drop, mesh allowlist by source subnet.
- `/etc/iptables/iptables.rules` — legacy iptables-nft table, restored at boot;
  its `ssh-public` DROP is scoped to `-i pub0`. A blanket port rule here silently
  shadows nft accepts because both tables hook input.

## Open items that keep biting

- ~~`opblob` binary lacks `stage-shm`/`persist`~~ FIXED 2026-08-25 (commit
  `22f44859`): subcommands restored from c4db14ae, binary rebuilt+deployed,
  persist→stage round trip verified. The hand-copy fallback in
  `opdbus-rundirs-up` remains as belt-and-suspenders.
- netclient v1.5 client vs v1.6 server (decoy) — MOOT for the mesh: netmaker
  broker retired 2026-08-25, both WG links are static confs now. Decoy's own
  egress is its self-contained wgcf-ingress (WARP), policy asserted by
  `mesh-policy.service`; its underlay to Cloudflare rides the VPS link
  (mark `0x7777` → table 51822 → MASQ out pub0).
- PTAP flow ownership: capability landed — vendored `rovs-openflow`
  (`vendor/rovs-openflow`, `[patch.crates-io]`) now encodes OXM_OF_PACKET_TYPE
  and OFPAT_ENCAP; translator installs via controller. RUNTIME BLOCKER left:
  OVS rejects the rovs-encoded FlowMod with error type=2 code=10 (BAD_ACTION)
  while identical semantics via `ovs-ofctl -O15 add-flow` install fine.
  Until that wire delta is root-caused (capture 6653 during a controller
  restart), the watchdog-owned flow stays authoritative: `ensure-ptap-flow.sh`
  + `ptap-watch`, and `openflow-static-flows.json` keeps the entry so the
  moment encoding matches, the controller takes over with no further change.
- Router (wrt-router) intentionally disconnected from this host: peer removed
  from static conf; its rc.local still dials a dead endpoint (harmless failed
  keepalives). Reconnect = re-add peer entry with pubkey
  `ppaYyM0y…`, or wipe its `/etc/rc.local` netmaker block.
