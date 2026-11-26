# Network runtime (OpenFlow on `ovsbr0`)

Spec: `.kiro/specs/rovs-openflow/`. Network reference: `docs/src/operations/network.md`.

**Status: source and documentation only. Nothing in this change is built into a release, installed or deployed.** The live switch still has its `NORMAL` flow (cookie `0x0`) and fail mode `standalone`. The live steps below are owner-approved steps (task 12). Run none of them without the owner's OK for that step.

## What the network runtime is

The network runtime lives inside the bridge process (`op-grpc-bridge`). It has two parts:

- The in-process OpenFlow owner. It programs flows with rovs, at OpenFlow 1.5.
- `ovsdb_bridge`. It changes bridges, ports, controllers and fail mode over OVSDB, also with rovs.

There is no separate controller process and no D-Bus hop between the bridge and OVS.

## Socket

OVS's own management socket, one per bridge:

```
unix:/var/run/openvswitch/<bridge>.mgmt     (for example ovsbr0.mgmt)
```

OVSDB keeps its own socket (`/usr/local/var/run/openvswitch/db.sock`, fallback `/var/run/openvswitch/db.sock`).

## Flows

- Every flow is a child of the `openflow` plugin. It is created by `add_flow`.
- The cookie is derived, not stored. The high 32 bits are the prefix `0x33444348`. The low 32 bits come from SHA-256 over bridge, table, priority and match.
- A flow without that prefix on a managed bridge is reported as foreign. It is not shown as a child.
- There is no `NORMAL` flow in the target state. Fail mode is `secure`.

## OpenFlow address

`10.200.0.1` on `ovsbr0` (next to `10.0.0.2` and `100.96.0.5`) is the OpenFlow controller address. OVS listens on `ptcp:6653:10.200.0.1` only when a packet-in controller is enabled. It is off by default. No flow needs it. This does not mean a `10.200.0.0/24` fabric exists.

## Retired: `op-of-controller`

- The runit service `op-of-controller` (`deploy/runit/op-of-controller/`) is retired.
- The `OF_CONTROLLER_*` settings (for example `OF_CONTROLLER_LISTEN`, `OF_CONTROLLER_TLS_*`, and the ones in `deploy/config/network.conf`) are retired with it.
- Task 9 removes the service directory, its entries in `deploy/runit/managed-services` and the golden `activate-services.tsv`, and the `OF_CONTROLLER_*` settings. Until task 9 lands, those files are still in the tree. Their presence does not make the service current.

## Managing it

Only through sealed plugins: `zcall` or 3tchedFS call files (`operations/openflow/<method>/call`). Never `ovs-vsctl`, `ovs-ofctl`, `ovs-appctl` or any other `ovs-*` command. Mutations need `--yes` and the owner's OK.

## Removing `NORMAL` (owner-approved live steps, task 12)

Today `ovsbr0` has only `priority=0 actions=NORMAL`. It carries all host traffic. Deleting it first would cut the host off. The order is fixed:

1. Deploy the bridge build and restart the host bridge (12.1). Owner OK first.
2. Add the two uplink flows, `eth0` to `LOCAL` and `LOCAL` to `eth0` (12.2). Read both back. Check that the gateway is reachable.
3. Delete the cookie-`0x0` `NORMAL` flow (12.3). Owner OK first.
4. Set fail mode to `secure` (12.3). Re-check reachability: SSH, the WARP doors, and `https://10.0.0.2:20000/`.
5. Add, modify and delete a test flow through `zcall`. Confirm it appears and disappears as a child on D-Bus and in 3tchedFS (12.4).

After any flow wipe, restore the uplink pair first. A new container or namespace port gets its own explicit flows. There is never a catch-all.

## What is not installed

This change installs no binary, edits nothing under `/etc` or `/usr/local`, restarts no service and does not reseal the live catalog. Deployment stays the prepared snapshot moved by `btrfs send/receive` (`./activate-golden.sh`).
