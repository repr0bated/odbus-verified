# GhostBridge Incus Socket Networking & OpenFlow Architecture

## Overview

GhostBridge on this host should be modeled as:

- `Incus` for container lifecycle, storage, start/stop, and isolation
- `Open vSwitch` for the single host bridge and internal socket ports
- `OpenFlow` for forwarding and policy routing
- standalone `systemd-networkd` under `dinit` for host L3 only
- `op-dbus` for native D-Bus and JSON-RPC control of OVS and plugin state

The important correction is that container networking is not primarily
bridge-per-container or `incusbr0`-style virtual NIC wiring. The intended model
is socket networking on top of `ovsbr0`, with controller-driven forwarding.

## Current Host Network Base

```text
Internet
  |
ens3 (physical uplink, ISP MAC identity)
  |
host routing
  |
ovsbr0 (OVS bridge, private datapath)
  |
  +-- ovsbr0        host L3/internal bridge port
  +-- ovsbr0-mgmt   management internal port
  +-- ovsbr0-sock   socket-network attachment port
  +-- priv_*        predefined privacy socket ports
  +-- sock_*        dynamic container socket ports
```

Host constraints:

- `ens3` is the standalone public uplink and must not be attached to `ovsbr0`
- host public `/32`, default route, and DNS stay on `ens3`
- `ovsbr0` carries the private host/datapath address
- `systemd-networkd` config is for host L3 only, not container dataplane design

## Core Design

### 1. Incus Owns Lifecycle, Not the Dataplane

Use `Incus` for:

- image selection
- storage pools
- container start/stop
- exec, files, profiles, and quotas

Do not treat `Incus` as the authoritative network fabric. In this design:

- the network fabric is `OVS + OpenFlow`
- container attachment is expressed as socket-facing OVS ports
- routing decisions are flow-based, not Linux bridge learning or `Incus` NAT

### 2. OVS Owns the Dataplane

`ovsbr0` is the single bridge for the privacy/router plane. It should be set for
controller-driven forwarding:

- `datapath_type=system`
- `fail_mode=secure`

That matches the host provisioning code in
[privacy_network.rs](/home/jeremy/git/operation-dbus/crates/crates/op-web/src/privacy_network.rs),
which ensures:

- bridge `ovsbr0`
- uplink port
- management internal port `ovsbr0-mgmt`
- socket internal port `ovsbr0-sock`
- OpenFlow controller probe to `127.0.0.1:6653`

### 3. OpenFlow Owns Routing

Traffic routing should be expressed as OpenFlow policy, not as static
per-container bridge membership.

That matches the intent of the `openflow` plugin in
[openflow.rs](/home/jeremy/git/operation-dbus/crates/crates/op-plugins/src/state_plugins/openflow.rs),
which describes:

- socket-based container networking
- predefined privacy ports
- dynamic `sock_{container_name}` ports
- policy-driven flow generation
- automatic discovery of dynamic container ports from OVSDB

## Socket Networking Model

There are two socket-port classes.

### Predefined Privacy Sockets

These are long-lived internal OVS ports for the privacy chain:

- `priv_wg`
- `priv_warp`
- `priv_xray`

The privacy-router plugin describes that chain in
[privacy_router.rs](/home/jeremy/git/operation-dbus/crates/crates/op-plugins/src/state_plugins/privacy_router.rs):

```text
priv_wg -> priv_warp -> priv_xray
```

OpenFlow then installs policy so traffic moves through that chain in the
intended direction.

### Dynamic Container Sockets

Normal application containers should be represented by dynamic socket ports:

- `sock_vectordb-prod`
- `sock_bucket-a`
- `sock_user-123`

The naming convention comes directly from the `openflow` plugin:

```text
sock_{container_name}
```

Those ports are expected to be:

- created at runtime
- discovered from OVSDB state
- routed by OpenFlow rules
- removed when the container stops

This is the actual target model for tenant/application connectivity.

## Incus in This Model

Incus still matters, but its role changes.

### What Incus Should Do

- create the container
- manage storage pool placement
- boot the workload
- expose lifecycle hooks for start/stop
- provide any needed unix socket mounts, bind mounts, or service wrappers

### What Incus Should Not Be The Primary Mechanism For

- default `nictype=bridged parent=ovsbr0` attachment for every workload
- default `incusbr0` NAT networking for the privacy path
- VLAN-as-primary-routing-model for the privacy plane

Bridged Incus NICs may still exist for specific edge cases, but they are not the
primary design if the goal is socket networking with OpenFlow routing.

## Current Repo Status

The repo is partially aligned with this design, not fully.

Aligned pieces:

- [privacy_network.rs](/home/jeremy/git/operation-dbus/crates/crates/op-web/src/privacy_network.rs) already provisions `ovsbr0`, `ovsbr0-mgmt`, `ovsbr0-sock`, and probes the OpenFlow controller
- [openflow.rs](/home/jeremy/git/operation-dbus/crates/crates/op-plugins/src/state_plugins/openflow.rs) already models dynamic `sock_*` ports and policy-driven routing
- [privacy_router.rs](/home/jeremy/git/operation-dbus/crates/crates/op-plugins/src/state_plugins/privacy_router.rs) already models privacy sockets and flow-based privacy chaining

Current gap:

- [privacy_container.rs](/home/jeremy/git/operation-dbus/crates/crates/op-web/src/privacy_container.rs) still attaches a bridged Incus NIC with `nictype=bridged parent=ovsbr0`

So if the requirement is strict socket networking for privacy/user containers,
that provisioning path still needs to be migrated. The document below describes
the target architecture, not that legacy bridged attachment path.

## Boot Sequence

Expected service order:

1. `op-session-bus`
2. `op-dbus`
3. `ovs-attach-ports`
4. standalone `systemd-networkd`
5. services that depend on the privacy fabric

At boot:

- `op-dbus` verifies persisted `ovsbr0` and the required internal datapath ports
- `ovs-attach-ports` restores the kernel-facing attachment and link state
- host L3 is applied to standalone `ens3`, secondary `uplink1`, and private `ovsbr0`
- privacy-network provisioning ensures `ovsbr0-mgmt` and `ovsbr0-sock`
- the OpenFlow controller becomes reachable on `127.0.0.1:6653`
- plugins publish socket ports and flows as containers appear

Relevant files:

- [privacy_router.rs](/home/jeremy/git/operation-dbus-proto/crates/op-plugins/src/state_plugins/privacy_router.rs)
- [op-of-controller](/home/jeremy/git/operation-dbus-proto/deploy/dinit/op-of-controller)
- [20-ovsbr0.network](/home/jeremy/git/operation-dbus/deploy/systemd/networkd/20-ovsbr0.network)
- [op-dbus-dinit.md](/home/jeremy/git/operation-dbus/docs/operations/op-dbus-dinit.md)

## Control Plane

Do not center this design around NetworkManager.

Preferred control paths:

- `org.opdbus.OvsdbV1` on D-Bus
- native OVSDB JSON-RPC
- the `openflow` and `privacy_router` plugins for policy/state generation

This matches the repo direction:

- D-Bus first
- OVSDB native access where possible
- OpenFlow policy from plugins, not ad hoc shell state

## Plugin Model

This architecture ties directly into the plugin system.

### `openflow` Plugin

Responsibilities:

- model bridges under OpenFlow control
- create/discover `sock_*` ports
- manage flow rules and flow policies
- generate controller-driven forwarding behavior

### `privacy_router` Plugin

Responsibilities:

- define privacy tunnel sockets (`priv_wg`, `priv_warp`, `priv_xray`)
- define privacy chain routing policy
- express function routing toward privacy sockets

### `service` Plugin

Responsibilities:

- install and supervise the controller/process layer under `dinit`
- keep the standalone `systemd-networkd` and OVS helper services present

### Publication Model

The D-Bus tree should expose the resulting objects as publication, not
desired-vs-current reconciliation. For this design that means:

- OVSDB rows appear as native objects
- socket ports appear when published by source state
- flow/controller state appears as native objects under the OpenFlow/OVS tree

## Example OpenFlow Intent

The routing model should look like policy, not interface wiring:

```text
in_port=sock_user-123   -> output:priv_wg
in_port=priv_wg         -> output:priv_warp
in_port=priv_warp       -> output:priv_xray
return traffic          -> reverse through the chain
```

For non-privacy workloads, policy can instead route:

- `sock_app-a -> sock_db-a`
- `sock_app-a -> sock_cache-a`
- `sock_app-a -> uplink/egress policy`

The point is that forwarding is explicit in OpenFlow, not inherited from a
bridged NIC topology.

## What Changed From The Earlier Draft

The previous draft I wrote still assumed too much bridge-style container
attachment:

- `incusbr0` as the default container network
- `nictype=bridged parent=ovsbr0` as the normal pattern
- Incus NIC wiring as the main model

That was the wrong emphasis.

The corrected model is:

- `Incus` for lifecycle
- `OVS` for dataplane
- internal socket ports for attachment
- `OpenFlow` for routing
- plugins generating and publishing state

## Practical Follow-Up

If you want the implementation to match this document end-to-end, the next code
change is not in the bridge bootstrap. It is in
[privacy_container.rs](/home/jeremy/git/operation-dbus/crates/crates/op-web/src/privacy_container.rs),
which still provisions a bridged Incus NIC. That path needs to be reworked so
container lifecycle results in `sock_{container}` publication and OpenFlow
routing instead of direct bridge attachment.
