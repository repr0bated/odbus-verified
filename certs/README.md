# Browser trust for the OP-DBUS door

`op-dbus-ca.crt` is the public CA certificate for the deployed TLS door at
`https://10.0.0.2:8090`. Import it into the trusted certificate authorities
for websites in the browser on your own computer, then restart the browser
and reopen `http://10.0.0.2:8080` over the WireGuard connection.

SHA-256 fingerprint:

```text
12:1A:25:38:82:BD:97:B6:AB:73:14:41:3B:A7:BC:B3:07:77:4B:B2:56:B6:70:76:97:E4:6D:60:34:1E:F2:A6
```

This is a copy of `/etc/op-dbus/tls/tonic-svc0-ca.crt` from the deployed
server. It contains no private key and is not a session credential. Adding
it to the repository does not install it into a browser's trust store.
