# zcall-driven init — state of play, 2026-08-10

Handoff after the Cursor session `95a643ff-452c-416c-b045-62e60b5bee7e` crashed
mid-deploy. Written from the recovered transcript plus live inspection of this
host. Facts are marked **verified** (I checked it on this box) or **unverified**
(needs confirming before you rely on it).

## The goal

The init system (`/etc/runit/sv/*`, `/usr/local/libexec/3tched/*`) drives all
network state through `zcall` / `zbusctl` against the sealed plugin tree — no
`ip`, no `ovs-vsctl`, no `ovs-ofctl`, no shell-outs.

## Why the reboots failed

**Not** a networking bug. A **dependency cycle** created by a half-applied deploy.

The previous session wrote a coherent change set into `deploy/runit/`, but
crashed before installing most of it. Exactly one file reached `/etc/runit/sv/`:
`op-grpc-bridge/run`. That new file waits on `ovsbr0-eth0` and
`ovsbr0-svc-addr`, while the *old* `ovsbr0-*` scripts still sitting in
`/etc/runit/sv/` wait on `op-grpc-bridge`.

**Verified** — three cycles in the live tree, none in `deploy/runit/`:

```
ovsbr0-eth0    -> ovsbr0-addr      -> op-grpc-bridge   -> ovsbr0-eth0
ovsbr0-addr    -> op-grpc-bridge   -> ovsbr0-svc-addr  -> ovsbr0-addr
op-grpc-bridge -> ovsbr0-svc-addr  -> op-grpc-bridge
```

Live log evidence (**verified**), boot of 01:43:

```
ovsbr0-eth0/current      dependency ovsbr0-addr not ready after 120s
ovsbr0-addr/current      dependency op-grpc-bridge not ready after 120s
ovsbr0-svc-addr/current  dependency ovsbr0-addr not ready after 120s
```

So `eth0` is never enslaved and `pub0` is never addressed. On the boots where
the capture *did* happen before the deadlock bit, the box went dark and needed
console + `ovs-vsctl del-br ovsbr0`.

A cycle detector over `wait_dep` edges is the cheap regression test here —
run it against `/etc/runit/sv` filtered by `/etc/runit/runsvdir/default`
before any reboot.

## What is still undeployed (verified)

| Artifact | Source | Live |
|---|---|---|
| `op-rtnetlink-init` | built `target/release/`, 01:33 | **absent** from `/usr/local/bin` |
| `ovsbr0-shared-mac` | new | old |
| `ovsbr0-uplink-up` | new | old |
| `ovsbr0-eth0-up` | new | old |
| `ovsbr0-addr-up` | new | old |
| `ovsbr0-svc-addr-up` | new | old |
| `ovsbr0-eth0/{run,check}` | new | old |
| `ovsbr0-addr/{run,check}` | new | old |
| `ovsbr0-svc-addr/{run,check}` | new | old |
| `ovsdb-port` | — | identical |
| `op-grpc-bridge/run` | new | **new (the one that landed)** |

`/usr/local/bin/op-ovsbr0-setup` is from Aug 2 and predates the source changes
to `op-ovsbr0-setup.rs`.

## Why you should not just deploy the rest

The undeployed change set **abandons zcall for boot networking**. It adds a new
native binary, `crates/op-network/src/bin/op-rtnetlink-init.rs`, and rewrites
`ovsbr0-addr-up` / `ovsbr0-svc-addr-up` down to:

```sh
exec /usr/local/bin/op-rtnetlink-init public     # was: zcall rtnetlink ...
exec /usr/local/bin/op-rtnetlink-init services   # was: zcall rtnetlink ...
```

That was done to break the cycle by removing boot networking's dependency on
`op-grpc-bridge` — which is the opposite of the point of the exercise.
Deploying it verbatim gives you a host that boots but is no longer zcall-driven.

## The direction that keeps zcall

The cycle is real but it is being cut on the wrong side. What forces
`op-grpc-bridge` to be late is **one listener address**, not the whole service.

**Verified** — `zcall`'s transport has no network dependency at all:

```
bus       unix:path=/run/opdbus/session-bus.sock
service   org.opdbus.v1.plugins
interface org.opdbus.v1.PluginV1   Call(ss) -> s
```

**Verified** — `op-grpc-bridge/run` sets:

```sh
FABRIC_BIND="127.0.0.1:8090,10.200.0.1:50051"
```

`127.0.0.1:8090` and `ZEROCLAW_UNIX_SOCKET=/run/opdbus/grpc.sock` are available
from the moment the process starts. Only `10.200.0.1:50051` — the ovsbr0 fabric
address that `ovsbr0-svc-addr` creates — needs the network. So:

1. `op-grpc-bridge` starts **early**, binding loopback + unix socket only, and
   serves the D-Bus plugin tree. Drop `wait_dep ovsbr0-eth0`,
   `wait_dep ovsbr0-svc-addr`, `wait_dep netclient`, `wait_dep incus-ct-qdrant`.
2. All `ovsbr0-*` boot scripts go back to `zcall rtnetlink ...` / `zcall
   rovs_commands ...`, depending only on `dbus` + `op-session-bus`.
3. The fabric listener `10.200.0.1:50051` is added **after** `ovsbr0-svc-addr`.

Step 3 already half-exists. **Verified** log lines from the 00:37–00:45 boots:

```
ovsbr0-svc-addr: fabric IP 10.200.0.1 on ovsbr0 — restarting op-grpc-bridge for :50051
ovsbr0-svc-addr: WARN 10.200.0.1 not on ovsbr0; deferring fabric gRPC bind
```

This was offered as an explicit choice in the crashed session ("Loopback bridge
early; fabric listener after egress") and the other branch was taken. That is
the decision to revisit.

Note this also satisfies the constraint you raised about egress — gRPC's
*egress-dependent* surface still comes up last. Only the local plugin-tree
surface moves early.

## Good news: the zcall rtnetlink backend is real

An earlier claim in the transcript — that the rtnetlink plugin methods "only
echo" because the mutation engine has no match — is **stale**. The session fixed
it. `crates/op-plugins/src/state_plugins/rtnetlink.rs:360` now has
`dispatch_rtnetlink_method`, which really calls the netlink backend:

- `set_link_state`  -> `link_up` / `link_down`
- `add_ipv4_address` -> `add_ipv4_address` (treats EEXIST as success)
- `set_mac_address`  -> `set_mac_address`
- `set_default_route` -> `replace_default_route_onlink`

Its doc comment is explicit that it exists so schema calls "must not fall
through to [the MutationEngine's] generic audit-only echo result."

Also worth keeping from that session, independent of the zcall question:

- `rtnetlink::add_ipv4_address` is now idempotent (returns Ok if the address is
  already present) — required for declarative boot retries.
- `rtnetlink::replace_default_route_onlink` installs the new default *before*
  deleting the old one, removing the delete-then-add outage window. This is the
  right primitive for the `eth0 -> pub0` cutover; keep it and expose it via
  `zcall rtnetlink set_default_route`, which it already backs.

## Open items

- **Stale doc, must resolve.** `/etc/op-dbus/network.conf`'s header comment
  still describes the old order (`uplink -> addr -> svc-addr -> eth0` last).
  The undeployed code does the opposite (`uplink -> eth0 -> addr`). Decide which
  is authoritative and fix the other. Both orders appear in the tree right now.
- **Unverified:** does `op-grpc-bridge` start cleanly when `10.200.0.1:50051` is
  unavailable, or does a failed bind abort the process? This determines whether
  step 1 above is a run-script change or also a code change.
- **Unverified:** do any plugins block at load on `COGNITIVE_MCP_QDRANT_URL`
  (`http://10.200.0.2:6334`) or `COGNITIVE_MCP_MCP_URL`? That is the real reason
  `wait_dep incus-ct-qdrant` was added; if plugin init is lazy, the wait is
  unnecessary and can be dropped.
- **Consider a cutover guard.** The window between enslaving `eth0` and
  addressing `pub0` is when the box goes dark. A watchdog that detaches `eth0`
  and restores its address if the gateway is unreachable after ~90s turns a
  lockout into a self-heal. `ovsdb-port del-port` already exists for the OVS
  half.
- `crates/op-network/src/bin/op-rtnetlink-init.rs` is untracked and is the
  non-zcall path. Delete it if you commit to the direction above.
- `build-golden.sh` was patched to stage `check` scripts and the whole
  `libexec-3tched/` directory — previously it shipped neither, which is part of
  why source and live drifted so far apart. Keep that fix regardless.

## Current host state (verified, 02:0x)

Reachable and stable. `eth0` holds `188.68.58.237/22` and the default route;
`ovsbr0` exists with `pub0`/`svc0`/`grpc0` internal ports only, no `eth0`.
`dhcpcd` on `ovsbr0` is a manual recovery action, not part of the boot chain —
ignore it when reasoning about boot.

The deadlock is, ironically, why the box is currently up: it stopped
`ovsbr0-eth0` before the capture.
