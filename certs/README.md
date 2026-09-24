# Browser trust for the OP-DBUS door

For browser access over WireGuard, open `http://dashboard.ghostbridge.tech:8080`.
Its existing Tonic door is `https://dashboard.ghostbridge.tech:8090` and uses
a publicly trusted certificate. No private CA import is needed for that hostname.

`op-dbus-ca.crt` is the public CA certificate retained for internal clients
using `https://localhost:8090` or `https://10.0.0.3:8090`. Import it only when
using those internal addresses.

SHA-256 fingerprint:

```text
12:1A:25:38:82:BD:97:B6:AB:73:14:41:3B:A7:BC:B3:07:77:4B:B2:56:B6:70:76:97:E4:6D:60:34:1E:F2:A6
```

This is a copy of `/etc/op-dbus/tls/tonic-svc0-ca.crt` from the deployed
server. It contains no private key and is not a session credential. Adding
it to the repository does not install it into a browser's trust store.

The dashboard certificate is renewed on the Artix host using Cloudflare
DNS-01. Runtime state and keys stay outside the repository and release tree.
See [renewal setup and verification](../deploy/certbot/README.md).
