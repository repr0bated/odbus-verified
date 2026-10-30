This is the concrete OD-106 operator procedure, not an activation receipt.
The owner approved its scoped native maintenance exception on 2026-10-04.

The mounted Xray v4 contract has only `get_stats`, `add_user`, `remove_user`
and the restricted `add_inbound` operation. It cannot persist an outbound,
REALITY/XHTTP configuration, or restart the application. `AGENTS.md` says
"Models must not write or reload Xray directly." The prior OD-104 network
exception does not cover this application change. A task-specific operator
exception is required before the live steps below. No grant expansion or
alternate application identity is proposed.

Private preparation on both nodes is `/etc/xray/.od106-prepared-20261004/`,
root-owned and mode 0700. Candidate files and parameter files are 0600.
Host files are `host.before.json`, `host.reality.json` and
`host.candidate.json`; decoy files are `decoy-egress.before.json`,
`decoy-egress.candidate.json`, `decoy-web.before.json` and
`decoy-web.candidate.json`. The generated seeds remain in this existing
private Xray configuration store. They do not belong in source control,
general memory, logs, or the public release component.

Requested operator scope:

- Transfer the tested public helpers and two runit definitions as a prepared
  read-only Btrfs snapshot; verify source/received UUIDs and file hashes.
- Enable only `op-xray-egress-carrier` and `op-xray-egress-loopback` on the
  host. Their existing relay executable carries bytes to the fixed decoy
  endpoint 129.80.64.127:9443; HTTP proxy exposure is host loopback 13128.
  Baseline checks additionally found the existing XHTTP web/mail routes had
  no container listeners at 18080/18081. The same loopback relay supplies
  those fixed destinations from the already mounted web/mail sockets. No
  additional service, network interface or application route is introduced.
- Atomically replace the canonical host-container
  `/etc/xray/xray_config.json`, and restart only host runit `xray-router`.
- Atomically replace decoy `/etc/xray/egress-server.json` and
  `/etc/xray/web-bridge.json`, and restart only their existing
  `xray-egress` and `xray-web-bridge` services using the decoy's native
  service manager.
- Perform bounded positive/negative transport checks and restore those
  exact files/services if acceptance fails.

Before activation, compare all active files against their private before
copies. Any difference means regenerate/review the candidate; do not overwrite
concurrent work. Record the current application and WARP component selectors,
protected service PIDs, interface aliases and private original file metadata.
Keep existing Xray UUIDs, Reality private key, short ID and wgcf keys unchanged.

Apply in two recoverable stages. First activate the decoy Reality candidate
and the host `host.reality.json` candidate plus the two relays. Verify a
bounded HTTP CONNECT through localhost 13128, correct cover hostname,
ML-DSA verification and decoy marked wgcf egress. Then coordinate the host
combined candidate with the decoy web-bridge candidate; this stage replaces
the XHTTP anonymous fallback with authenticated VLESS Encryption and changes
the matching path on both sides. Existing TCP connections may close during
these deliberate application restarts. Verify the authenticated web/mail
route behavior before accepting this second stage.

The first routing rule selects Reality only for the new local proxy. All
pre-existing host listeners, rules and default outbound remain. The two XHTTP
addresses, 10.0.0.2 and 192.168.255.1, remain aliases on tun5 and reach the same
inbound; splitting its HTTP connections does not create independent cover
paths. No default route, OVS port, WARP service, SSH transport, identity,
credential grant, or application release selector is changed.

Rollback stage two by restoring the private host Reality-only file and decoy
web before-file atomically, then restarting just the two affected Xray
services. Rollback the whole change by restoring all three original files,
restarting those Xray services, and disabling the two new relays. Check the
original file digests and the prior door/page transport again. Preserve all
unrelated runit enablement and every received release.

The decoy's DNS already uses the protected MASQUE path from OD-104, so no new
Xray DNS override is proposed. ClientHello fragmentation was conditional on
demonstrated SNI blocking; none has been demonstrated, so it remains disabled.
The old host Reality Apple/short-ID settings belong to a separate inbound and
are preserved. These boundaries correct the pasted advice's conflation of
that inbound with the decoy egress server and its stale plaintext-GRE claim.
