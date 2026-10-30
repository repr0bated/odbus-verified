This component prepares OD-107's owner-selected private-only SSH and Reality
access. The initial cutover was rolled back after the owner reported lost
client connectivity. See `DEPLOYED.md` for current state before activation.
It uses the existing WireGuard/SSH TUN paths and changes no identities, keys,
grants or WARP routing.

The host carrier relay connects its existing Unix socket to `10.0.0.1:9443`.
The decoy's independent `inet op_decoy_private_ingress` table drops TCP22/9443
arriving outside `lo`, `tun5`, `wg-opdbus` and `wg0`, including already
established public connections. Other ports continue through existing policy.
Both IPv4 and IPv6 are covered. The unit installs the guard before public
listeners at boot. Stopping the unit deliberately retains the guard.

Deployment uses a prepared read-only Btrfs snapshot, send/receive to each node,
hash verification, and the separate `private-access-current` selector. The
host carrier run script links into the received component; only that runit
service is restarted. The decoy unit links into its received component and
is activated with its native service manager. Never restart the host network
or reload the entire decoy firewall for this component.

Before installing the live guard, verify a fresh SSH connection to10.0.0.1
using the existing decoy host-key alias and set a five-minute transient timer
to run `private-ingress.sh remove`. Verify a second private SSH connection,
authenticated Reality egress through the existing local proxy, and failed
public TCP22/9443 connects. Cancel the timer only after those pass. Check
private page/door access and unrelated service PIDs. Run `check` for the
ownership receipt and exact normalized nftables readback.

After the live checks, native `cf` changes only ssh.3tched.com and
xray.3tched.com to DNS-only A10.0.0.1 and removes ssh's second public A record.
Public DNS still publishes the private address; reachability requires an
existing private connection. MCP/OpenClaw proxy flags are a separate DNS
change. No certificate or Cloudflare TLS-mode changes belong to this component.

Rollback first restores access through an existing private session:

1. Run the received `private-ingress.sh remove`; it deletes only its exact
   owned table. Disable the decoy unit to prevent reinstall at boot.
2. Restore the saved host carrier run symlink to the prior xray-hardening
   component and restart only `op-xray-egress-carrier` with `sv`.
3. Restore the private DNS before-state through native `cf` if public service
   is deliberately being restored. Recreating the removed duplicate A record
   produces a new Cloudflare record ID.

`test-private-ingress.sh` runs inside disposable user/network namespaces. It
tests packet behavior, unrelated-port preservation, repeated operations,
owned cleanup and refusal to delete foreign rules or sets. Private runtime
receipts and original DNS records remain outside the public component.
