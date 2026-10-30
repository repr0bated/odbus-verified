OD-107's first private-only cutover was rolled back on 2026-10-04 after the
owner reported lost client connectivity and recovery through noVNC. Do not
interpret the prepared component or server-side tests as confirmation that
the owner's client can use the private path.

The initially received read-only component was
`20261004T085307Z-private-access`. Source UUID was
`0ed4c6d2-8d48-ff48-b7d8-de3cd96bb840`; host receive UUID
`15bdad81-fa9e-df47-b087-755e63c18f4f`; decoy receive UUID
`a8c5e189-fa3c-e54b-9cf1-5d791699a2df`. All hashes and read-only properties
passed. The decoy guard's isolated IPv4/IPv6 tests covered both ports, foreign
state refusal and cleanup. Live private SSH and authenticated Reality worked,
and public22/9443 were blocked. This did not verify the owner's client path.

Recovery removed the exact owned decoy table, disabled its unit, restored the
host carrier run link to `20261004T0831-xray-hardening`, and restored both
SSH/Xray public DNS names through native `cf`. The removed secondary SSH A
record was recreated with a new ID. Authoritative DNS and complete35-record
protected-field readback match the before-state, with MCP/OpenClaw's approved
proxy flags retained. Fresh public and private SSH to the decoy work; the
Reality proxy again returns `warp=plus` through its original carrier.

The existing host `ip filter INPUT` policy independently drops all new SSH
outside loopback. OD-104's `inet filter` private acceptance could not override
that separate drop. Host10.0.0.2:22 was listening but a new decoy connection
timed out. Three specific live accepts now allow new TCP22 only on `tun5`,
to10.0.0.2, from10.0.0.0/24 or the two decoy peer addresses. A fresh connection
now receives the host OpenSSH banner. Source `deploy/config/iptables.rules`
contains the same rules. The focused isolated test reproduces the old stacked
drop, verifies all three intended sources, rejects public/wrong-destination
SSH, and preserves8444. Syntax and diff checks pass.

The persistent correction was received as read-only component
`20261004T090355Z-host-private-ssh`, source UUID
`3526352a-48f0-2545-83a5-6fe7354c5286`, host receive UUID
`44a6f9dc-efb6-4147-b11e-f59b581b5811`. All hashes pass.
`/etc/iptables/iptables.rules` now selects that component through
`/opt/op-dbus/host-private-ssh-current`; it matches the source exactly. The
existing iptables runit service remains its boot loader; no full live restore
or service restart was performed. Before/after comparison proves every
preexisting iptables rule is preserved. The blanket public SSH drop,
credentials and daemon remain unchanged. Recovery rollback removes only the
three exact `op-warp-ssh/private-ssh` accepts and restores the retained original
configuration at `host-iptables-persistent.before` in the private evidence
directory. Never flush either stacked table.

The owner confirmed another operator had deliberately stopped op-web and
op-grpc-bridge and directed this task to leave them alone. They subsequently
returned with new PIDs without action by this task. Private page200, trusted
door401 and encrypted decoy web200 now pass. WARP/SSH carrier PIDs and unique
tun5 address aliases remain unchanged.

MCP/OpenClaw now resolve to Cloudflare proxy IPs, but HTTPS still returns526
because the existing origin certificate omits those names. HTTP redirects to
HTTPS. The owner explicitly declined certificate updates; no TLS-mode change,
certificate write, issuer-store permission bypass or Cloudflare grant change
was performed. The earlier exact stale public443 DNAT removal remains applied;
the persistent decoy rules already omitted it. Other DNS names still expose
the decoy's public IP.

Private DNS receipts, before-state, rollback operations and host firewall
backups are in `/var/tmp/decoy-public-dns-20261004`. Decoy prechange NAT evidence
is in `/var/lib/op-warp-ssh/private-access-20261004`. The private-only goal is
unfinished pending client-path verification. No whole-host reboot or full
workspace test pass is claimed; the prior `/verify` stopped at unrelated Rust
formatting drift.
