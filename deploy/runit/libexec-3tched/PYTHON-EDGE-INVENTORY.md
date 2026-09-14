# Python edge inventory — /usr/local/libexec/3tched

Captured live from the host 2026-08-16. This is a historical invocation snapshot;
rows explicitly marked retired no longer describe active deployment wiring. It records
the invocation context that
cannot be recovered by reading the scripts: which runit service starts each
one, its exact argv, what it listens on, and what is on the other side.

**Scope note.** The repo tracks 368 `.py` files, but they are vendored skill
scripts under `.agents/skills`, `.factory/skills`, `.aye`, and
`.consolidation-staging` — none are part of this system. The programs below
are the ones that carry live traffic. Eight of the nine have no source in the
repo; the host copies are authoritative.

## 1 · The Rust precedent — read this first

`op-uds-relay` is already a Rust binary in the same directory and is already
running six instances doing what `socket-relay` does. Its flags are
`--tcp-to-unix listen=sock` and `--unix-to-tcp sock=host:port`, repeatable.

| Service | op-uds-relay invocation |
|---|---|
| `uds-xray-reality` | `--tcp-to-unix 0.0.0.0:8444=/run/ghostbridge/xray/reality.sock` |
| `uds-qdrant-grpc` | `--tcp-to-unix 10.200.0.2:6334=/run/ghostbridge/qdrant/grpc.sock` |
| `uds-netmaker-api` | `--tcp-to-unix 127.0.0.1:8081=/run/ghostbridge/NetMaker/api.sock` |
| `uds-netmaker-broker` | `--tcp-to-unix 127.0.0.1:8083=/run/ghostbridge/NetMaker/broker.sock` |
| `mail-port-fabric` | six `--tcp-to-unix` on `100.69.0.1` (25/143/465/587/993/9000) plus `--unix-to-tcp /run/ghostbridge/mail/outbound-smtp.sock=100.69.0.2:25` |

So for most of `socket-relay` the question is not "how do I write this in
Rust" but "does op-uds-relay already cover this argv." See §2.

## 2 · socket-relay — 14 services, 11 running, one program

Distinguished entirely by argv. Grouped by whether op-uds-relay's existing
flags already express them.

### Already expressible as `--unix-to-tcp`

| Service | argv | Running |
|---|---|---|
| `xsock-netmaker` | `unix-listen /run/ghostbridge/netmaker.sock tcp-connect 127.0.0.1 8081` | yes |
| `xsock-netmaker-broker` | `unix-listen /run/ghostbridge/netmaker-broker.sock tcp-connect 127.0.0.1 8083` | yes |
| `xsock-netmaker-egress` | `unix-listen /run/ghostbridge/NetMaker/egress.sock tcp-connect 127.0.0.1 13128` | yes |
| `xsock-qdrant` | `unix-listen /run/ghostbridge/fwd-qdrant.sock tcp-connect 10.200.0.2 6334` | yes |
| `xsock-web` | `unix-listen /run/ghostbridge/fwd-web.sock tcp-connect 127.0.0.1 8080` | yes |
| `mail-web-socket` | `unix-listen "$SOCK" tcp-connect 127.0.0.1 8440` | no |
| `fwd-mail-php-9000` | `unix-listen "$SOCK" tcp-connect 127.0.0.1 9000` | no — `mail-port-fabric` now serves `100.69.0.1:9000` via op-uds-relay |

### Already expressible as `--tcp-to-unix`

| Service | argv | Running |
|---|---|---|
| `fwd-nm-mesh-8090` | `tcp-listen 100.69.0.1 8090 unix-connect /run/opdbus/grpc.sock` | **retired 2026-08-31** — `op-grpc-bridge` conditionally binds the locally-owned configured mesh IP directly |
| `uds-assistant` | `tcp-listen 10.200.0.2 8091 unix-connect "$SOCK"` | no — assistant container is stopped |

### NOT expressible today — TCP-to-TCP, no op-uds-relay flag exists

| Service | argv | Running |
|---|---|---|
| `fwd-8090` | `tcp-listen 10.0.0.3 $TONIC_PORT tcp-connect 127.0.0.1 $TONIC_PORT` (8090) | **retired 2026-08-31** — `op-grpc-bridge` binds `10.0.0.3:8090` directly |
| `fwd-nm-tonic-8081` | `tcp-listen 10.0.0.2 8081 tcp-connect 127.0.0.1 8081` | yes |
| `fwd-nm-broker-8083` | `tcp-listen 10.0.0.2 8083 tcp-connect 127.0.0.1 8083` | yes |
| `fwd-nm-mesh-8081` | `tcp-listen 100.69.0.1 8081 tcp-connect 127.0.0.1 8081` | yes |
| `fwd-nm-mesh-8083` | `tcp-listen 100.69.0.1 8083 tcp-connect 127.0.0.1 8083` | yes |

Adding a `--tcp-to-tcp` mode to op-uds-relay retires all fourteen services'
dependence on Python with no new binary.

## 3 · The other Python programs

### sni-demux.py — service `fwd-443`

```
--listen 188.68.58.237:443 --listen 10.0.0.2:443
--map mail.3tched.com=/run/ghostbridge/mail/web.sock
--map 10.0.0.2=/run/ghostbridge/mail/web.sock
--map 100.69.0.1=/run/ghostbridge/mail/web.sock
--map mail.ghostbridge.tech=/run/ghostbridge/mail/web.sock
--default /run/ghostbridge/xray-reality.sock
```

The public 443 door. **It must not terminate TLS.** It reads the cleartext
SNI out of the ClientHello, then splices bytes; it never holds plaintext. A
port that "improves" this by terminating breaks Reality on the other side of
`xray-reality.sock` and silently relocates the identity boundary — a
terminator is the component that *could* stamp identity headers, which is
tracked as OQ-7 in `.kiro/specs/session-genesis-identity`. Port the behavior
as-is; do not upgrade it here.

Note the default route: anything whose SNI is not one of the four mapped
names goes to xray.

### tls-relay.py — service `op-web-tls`

Listens `0.0.0.0:8448`. Newest of the set (added 2026-08-15) and the only one
whose source is in the repo. The `op-web-tls` run script waits on a
dependency via `sv check` before starting.

### nm-api-tls.py — service `nm-api-tls`

Listens `0.0.0.0:8443`. TLS front for the NetMaker API.

### nm-warp-egress-proxy.py — service `nm-warp-egress-proxy`

`nm-warp-egress-proxy.py 127.0.0.1 13128`. Listens `127.0.0.1:13128`; it was
the egress side of the retired WARP tunnels. The deprecated `wgcf-uiStream`
service has been removed. `xsock-netmaker-egress` feeds it from
`/run/ghostbridge/NetMaker/egress.sock`. A stale
`.bak-pre-mark-flip-20260815T000603Z` copy sits beside it — read the diff
before porting, it records a behavior change.

### mail-port-fabric.py — service `mail-port-fabric`

On disk and referenced by the service, but the running `mail-port-fabric`
process is `op-uds-relay`. Determine whether the Python is dead before
porting it.

### ovsdb-port, or-fusion-archive.py

On disk, not running, no service references them. Confirm dead before
spending effort.

## 4 · Cutover constraints

- Every one of these is on a live path — public 443, mail, NetMaker, qdrant,
  the gRPC door. Migrate per service: new run script, `sv restart <name>`,
  verify, keep the previous run script for rollback. Not a batch swap.
- Ports and addresses are load-bearing and non-obvious: `10.0.0.2` and
  `10.200.0.2` are both on the `3tched` internal port; `10.0.0.3` is `svc0`;
  `100.69.0.1` is the `netmaker` interface. Copy them exactly.
- Some run scripts take their values from environment (`$TONIC_PORT`,
  `$SOCK`) — read the whole run script, not just the exec line.
- CLAUDE.md forbids `Command::new` subprocesses in plugin/service code and
  requires Rust-first. These relays are the standing exception; replacing
  them is what removes it.
