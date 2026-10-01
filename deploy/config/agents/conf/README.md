# Configuration

Place shared, non-secret agent configuration here. Configure each consuming
runtime to read its chosen file; the bind mount does not auto-load configuration.

`op-identity-egress.socket` and `op-identity-egress.service` are container
systemd units for the read-only OS seed, installed in `/usr/lib/systemd/system`.
They expose an HTTP proxy on container loopback `127.0.0.1:3128` by forwarding
bytes to an assigned Unix socket. The seed already supplies
`/usr/lib/systemd/systemd-socket-proxyd`. The units do not provide the upstream
HTTP proxy, configure IP routes, or manage NetworkManager.

Keep the socket disabled until a verified HTTP forward-proxy socket is
available to this identity. Copy `identity-egress.env.example` to the identity's
writable `/etc/op-dbus/identity-egress.env` and set `OP_EGRESS_UNIX_SOCKET` to
its absolute container-local path. The socket unit is conditional on that
configuration file; the service checks that the configured path is a socket.
The example intentionally assigns no upstream. An existing socket file alone
does not prove the protocol, permissions, or outbound route works.

Once the upstream is verified, the consuming application can use
`http_proxy=http://127.0.0.1:3128` and `https_proxy=http://127.0.0.1:3128`;
retain suitable `no_proxy` exclusions for its local endpoints. Configure these
only for the intended consumer, not globally in the seed. This is HTTP/HTTPS
application egress, not a default IP route or transparent UDP transport.

The host remains runit-managed. These units belong inside identity containers.
