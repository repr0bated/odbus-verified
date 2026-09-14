# Runit cutover: zcall / busctl / rovs (2026-08-10)

Live host units under `/etc/runit/sv` and helpers under `/usr/local/libexec/3tched`
were rewritten to stop using `ovs-vsctl` / `ovs-ofctl` / `dbus-send` / `socat`
(except intentional leave-alones: `fwd-443` Reality, `xsock-decoy` Oracle).

Sources mirrored here so `build-golden` / reinstall does not revive banned CLIs.

## Ordering
- `dbus` before `op-session-bus`
- `op-grpc-bridge` waits `ovsbr0-uplink` (not `ovsbr0-addr`) so plugins can apply addresses
- `op-grpc-bridge` binds loopback (`127.0.0.1:8090`) until `ovsbr0-svc-addr` touches
  `/run/opdbus/grpc-fabric-bind-ready` and restarts it for `10.200.0.1:50051`
- `ovsbr0-addr` / `svc-addr` / `eth0` wait for `op-grpc-bridge` then use `zcall`

## Helpers
- `wait-op-plugins` — session bus + `zcall list`
- `ovsdb-port` — early-boot OVSDB via `ovsdb-client` (pre-plugin)
- `socket-relay` — byte relay replacing `socat` for xsock/fwd units
