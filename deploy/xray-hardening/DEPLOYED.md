OD-106 was activated and verified on 2026-10-04 at 08:29 UTC under the owner's
explicit scoped native Xray maintenance exception. This supersedes the earlier
prepared-only status in the 08:17 handoff. `ACTIVATION.md` remains the procedure
and rollback contract.

The active public component is `20261004T0831-xray-hardening`. It was prepared
from reviewed workspace inputs at HEAD `c7936fda979d6373b4d212b61128344946194f3d`
and transferred to both nodes by Btrfs send/receive. Both received trees are
read-only and every SHA256SUMS entry passed. Source/received-parent UUID is
`61c6cc47-e40d-2a48-b5d1-1dd2baed8419`; host receive UUID is
`faf0a09d-9217-a24b-8381-f1a9be94f036`, decoy receive UUID is
`0eacd0c3-bb79-d24c-983e-b8bbce6cbe70`. The predecessor
`20261004T0818-xray-hardening` is retained. The host's dedicated
`/opt/op-dbus/xray-hardening-current` selector owns the two enabled relay run
scripts. This receipt was written after the immutable component was received.

The three approved private candidates are now live: canonical host-container
`/etc/xray/xray_config.json`, decoy `egress-server.json` and
`web-bridge.json`. The host container remains NIC-less. A loopback HTTP proxy
at `127.0.0.1:13128` selects the new host Reality client and decoy server; it
does not replace the host's default route or select existing application
traffic automatically. Its carrier is decoy TCP9443, with the corrected
`assistant.ghostbridge.tech` cover, Vision and matching MLDSA authentication.
The server retains its existing UUID, Reality private key, nonempty short ID
and marked wgcf outbound.

The separate decoy-to-host XHTTP bridge now uses matching MLKEM VLESS
Encryption with 1-RTT, a shared random path, packet-up, padding and XMUX
settings, and explicit download settings. Host upload address `10.0.0.2`
and download address `192.168.255.1` remain aliases of the same `tun5`.
Existing authenticated web/mail routing and accounts are preserved. The
incompatible anonymous VLESS fallback was removed as approved.

Baseline verification found the existing redirects to container loopback
18080/18081 had no listeners, producing connection-refused errors before the
XHTTP upgrade. The same approved loopback relay now connects those ports to
the existing mounted `fwd-web.sock` and `mail/web.sock`. Temporary relay tests
passed, the repair was included in the second received component, and the
baseline web page returned 200 before activating encrypted XHTTP. No other
service or network route was needed. The mail socket is a TLS origin; this
byte relay does not add TLS to plaintext requests.

Verification evidence:

| Check | Observed result |
| --- | --- |
| Native Xray 26.3.27 on both architectures | All three candidates accepted; installed host config parsed inside its actual container |
| Isolated paired crypto tests | Reality correct/wrong/restored MLDSA verifier and XHTTP correct/wrong/restored MLKEM key checks passed |
| Live Reality proxy | Repeated HTTPS succeeded; Cloudflare trace reports `warp=plus`, egress IPv4 `104.28.196.79` |
| Live encrypted web bridge | HTTPS to private decoy443 with the configured cover name: 200, 553 bytes, certificate verification 0 |
| Live encrypted mail route | TLS through decoy loopback18443 to the mail origin: 200, 161179 bytes; existing origin certificate used only as the test CA, hostname verification 0 |
| Full Reality capture | 39 packets, 19682 captured frame bytes, zero truncated frames or kernel drops; actual ClientHello SNI matches cover; negotiated TLS1.3 / TLS_AES_128_GCM_SHA256; encrypted application records present |
| Full XHTTP capture | 282 packets, 198666 captured frame bytes, zero truncated frames or kernel drops; four POSTs to 10.0.0.2 and one GET to 192.168.255.1 |
| Existing transport | Decoy marked rule still selects wgcf-ingress; unmarked traffic and SSH still select MASQUE; host .2/.4/old GRE alias occur only on tun5 |
| Existing application access | Local and decoy-to-host door: valid TLS plus expected anonymous 401; private page: 200 |
| Preservation | Existing host accounts/outbounds/rules and decoy accounts/private key/short ID/outbound mark compare equal to private originals; bridge, web, WARP and SSH service PIDs unchanged |

Only the affected Xray services and the two new relays were deliberately
started/restarted. App selector `20261004T065332Z-c1e0fe6`, WARP selector
`20261004T0745-warp-ssh`, identities, grants and existing wgcf keys are
unchanged. DNS was already protected by MASQUE. Conditional ClientHello
fragmentation remains disabled because SNI blocking was not demonstrated.
The unrelated host Reality inbound and its cover list were preserved.

Private originals, candidates and parser logs remain in
`/etc/xray/.od106-prepared-20261004/` on each node. Live receipts and full
captures are root-only under `/var/tmp/od106-live-20261004/`; private seeds and
production config values are excluded from this repository and public
components. The originally prepared public receipt at
`/var/tmp/od106-component-20261004/RECEIPT.json` is updated with activation
status and points to the final component.

No whole-host reboot or authenticated browser identity flow was exercised.
The prior required `/verify` attempt stopped at unrelated formatting drift
in `crates/op-cozo-store/src/identity_transactions.rs:1159`; subsequent
workspace Clippy/tests were not reached. No Rust changed in this task.
Native configuration, shell syntax, crypto fixtures and the live transport
checks above passed; they do not imply a full workspace test pass or prove
the traffic indistinguishable from ordinary browsing.
