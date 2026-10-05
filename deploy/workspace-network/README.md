# Workspace socket transport proposal

These files are an isolated transport example, not installed provisioning or
enabled services. They do not change a base, container, identity record, Xray
configuration, proxy policy, or network interface. No live credentials appear.

The proposed chain follows the existing NIC-less Xray carrier pattern:

```text
workspace application using SOCKS5 (TCP or UDP)
  -> guest Xray on 127.0.0.1:1080 (UDP enabled)
  -> guest VLESS outbound to 127.0.0.1:19443
  -> guest op-uds-relay TCP-to-Unix stream
  -> dedicated mounted directory /run/opdbus-workspace-egress/vless.sock
  -> dedicated owner Xray VLESS inbound
  -> operator-approved upstream outbound
```

`guest-xray.json` and `host-xray.json` use `REPLACE_TRANSPORT_UUID`. This is a
transport credential placeholder, not a workspace session ID or existing key.
No identity creation, credential migration, or reuse policy is implied. The host
template deliberately uses a blackhole upstream until its owner supplies an
approved outbound. It must not be deployed as a substitute for existing policy.

For the intended central integration, use `prepare-central-xray.sh` to prepare a
**new private candidate file** from the existing canonical configuration:

```sh
bash deploy/workspace-network/prepare-central-xray.sh \
  /path/to/canonical-config.json /private/operator-supplied-transport-uuid \
  /run/opdbus-workspace-egress/vless.sock /private/candidate/xray_config.json
```

The supplied UUID file must be caller-owned mode 0600; the new output directory
must be caller-owned mode 0700. The transformer does not generate a UUID, read
identity keys, or print credentials. It preserves all existing JSON fields and
appends only a dedicated `workspace-egress` VLESS Unix inbound plus a route to
the **existing** `od106-reality-egress` VLESS outbound. It does not copy that
outbound's private settings into guest templates. It rejects missing/duplicate
upstream tags, inbound tag/socket collisions, occupied socket paths, existing
output files, and earlier unscoped routes that could shadow the appended rule.
Output is mode 0600, published exclusively without overwriting a file.

`test-central-candidate.sh` exercises this transformer using synthetic input,
checks that removing the two appended entries restores the original JSON,
validates the result with the native Xray parser, and checks rejection guards.
Its synthetic upstream is sufficient for parser validation; production REALITY
credentials and policy are never read by that test.

The eventual boot integration belongs to host provisioning: expose only the
dedicated directory to the guest; prepare the guest config with the explicitly
provided transport credential; start the central socket owner before the guest
relay/Xray; and start guest loopback listeners before proxy-aware applications.
The guest relay and guest Xray should have persistent supervisors and fail
closed when the socket owner is absent. Incus native start must wait for these
externally owned prerequisites. No service units are enabled or provisioning
scripts modified by this proposal. Activating the candidate in the central
Xray configuration requires the scoped operator exception described by the
repository's Xray configuration policy, as well as pending base acceptance.

The guest carrier command, after an external owner exposes only its dedicated
directory through an Incus read-only disk device, is:

```sh
op-uds-relay --tcp-to-unix '127.0.0.1:19443=/run/opdbus-workspace-egress/vless.sock'
```

The socket owner must define directory/socket permissions compatible with the
container's UID mapping. Mounting the directory allows socket recreation; this
proposal does not mount the broad ghostbridge directory or any control sockets.
The private loopback/Unix-socket hop uses no transport encryption; filesystem
authority and transport authentication need review before deployment.

The loopback SOCKS server handles the local UDP association. VLESS then carries
the datagrams in its stream (UoT), so the Unix stream relay does not need native
UDP support. Mux is disabled in this minimal example. Xray documents VLESS UoT
and optional XUDP separately in its [outbound documentation](https://xtls.github.io/en/config/outbound.html).
The [SOCKS documentation](https://xtls.github.io/en/config/inbounds/socks.html)
requires `udp: true` for UDP. If XUDP is selected later, review its default UDP
443 rejection. Likewise an upstream Vision flow can suppress UDP 443 unless its
UDP443 variant is selected; see [VLESS documentation](https://xtls.github.io/en/config/outbounds/vless.html).
No existing upstream settings are changed here.

Run the disposable proof with:

```sh
bash deploy/workspace-network/test-socket-transport.sh
```

It uses existing `xray`, `op-uds-relay`, `rustc`, `jq`, `ip`, `ss`, and rootless
user/network namespaces. The fixture compiles a standalone Rust echo server and
SOCKS probe without Cargo. It starts a host VLESS server on a private temporary
Unix socket and loopback-only TCP/UDP echo endpoints, then starts the guest
processes in a new namespace containing only `lo` and no route. It verifies the
host TCP echo is unreachable directly from the guest, then verifies both SOCKS
TCP CONNECT and UDP ASSOCIATE round trips over the Unix stream. The fixture host
routes only destination `127.0.0.1`; all other destinations are blocked. Child
processes and temporary files are removed on exit. It never accesses production
Xray configuration or sockets, and makes no external network requests.

On 2026-10-05, both config validation and both round trips passed with Xray
26.3.27 (`d2758a0`). This proves the TCP/UDP transport through a Unix stream,
not internet access through the production upstream. The fixture shares its
temporary socket pathname across network namespaces; it does not validate an
Incus disk mount, read-only bind, UID mapping, reboot readiness, DNS resolver,
or per-identity authorization.

This provides proxy-aware TCP/UDP access. An ordinary application's arbitrary
`connect` or `sendto` call still needs SOCKS support or a separate interception
mechanism. Loopback alone supplies neither interception nor a default internet
route. `nmrs` is a NetworkManager D-Bus binding, and the separately installed
`sparesparrow/rust-network-mgr` manages interfaces through its control socket;
neither is the VLESS traffic carrier demonstrated here. Transparent application
integration remains unresolved and is not claimed by this proposal.
