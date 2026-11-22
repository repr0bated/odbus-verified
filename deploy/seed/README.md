# Seed overlay

Files the shared seed (`3tched-pool` `images/@seed/rootfs`) carries on top of
its base OS. They were first placed by hand on 2026-11-16 during recovery;
this directory is their source.

The seed init is systemd; the host stays runit. The antiX runit and sysvinit
packages are purged from the seed. Containers have no NIC: the default
profile has only the root disk (on 3tched-pool) and the shared lungs socket.
The seed includes `/opt/run-mounts/opdbus/{lungs,tunnel}` so those binds have
a target directory.

- `op-grpc-bridge.service`, `opdbus-lo.service`: the container owns its door,
  op-grpc-bridge on lo `10.0.0.2:8090`.
- `3tchedfs.service`, `threetchedfs.service`: both 3tchedFS views, mounted in
  the container over the **host's** `lungs.sock`.
- `3tchedfs.service.d/10-fuse-first.conf`,
  `threetchedfs.service.d/10-fuse-first.conf`: start those units before
  `sysinit.target`. Each checks `/dev/fuse`, the catalog manifest, and the
  lungs socket, and is only treated as started once the FUSE is mounted.
- `opdbus-runtime.conf`: creates `/run/mounts`, `/run/mounts/3tchedFS`,
  `/run/mounts/threetched`, and `/dev/shm/opdbus/state` at boot.
- `op-tunnel-door.service`: only where provisioning bound `/run/opdbus/tunnel`
  (the chatbot); holds the shared door socket and splices the host door into
  the container door. Binary: `op-tunnel-door` from `crates/op-tunnel-mux`.
- `opdbus-sockets.conf`: `/run/opdbus` -> `/opt/run-mounts/opdbus`, where
  provisioning binds the host directories (`lungs` read-only, `tunnel` for
  the chatbot).
- `bridge.env`: the container bridge's own lungs socket is private
  (`/run/op-grpc-bridge/lungs.sock`). Lungs stay on the host; the bridge's
  `bind_socket` unlinks an existing socket, so it must never share the
  host's path. Door TLS uses the container's own pair in
  `/var/lib/op-dbus/door-tls/`.
- `door-tls-init.service` + `usr/local/libexec/op-dbus/door-tls-init`:
  generates the door key and self-signed certificate once, inside the
  container; the key never exists on the host. Only the public certificate is
  exported as `/run/opdbus/door.crt` (host `/run/opdbus/ct/<name>/door.crt`)
  for clients and op-web to pin. End-to-end TLS: client to container.

Apply by making the seed writable, copying these files, and setting it
read-only again. Never while a container depends on the seed.
