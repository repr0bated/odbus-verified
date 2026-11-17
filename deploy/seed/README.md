# Seed overlay

Files the shared seed (`3tched-pool` `images/@seed/rootfs`) carries on top of
its base OS. They were first placed by hand on 2026-11-16 during recovery;
this directory is their source.

- `op-grpc-bridge.service`, `opdbus-lo.service`: the container owns its door,
  op-grpc-bridge on lo `10.0.0.2:8090`.
- `3tchedfs.service`, `threetchedfs.service`: both 3tchedFS views, mounted in
  the container over the **host's** `lungs.sock`.
- `opdbus-sockets.conf`: `/run/opdbus` -> `/opt/run-mounts/opdbus`, where
  provisioning binds the host directories (`lungs` read-only, `tunnel` for
  the chatbot).
- `bridge.env`: the container bridge's own lungs socket is private
  (`/run/op-grpc-bridge/lungs.sock`). Lungs stay on the host; the bridge's
  `bind_socket` unlinks an existing socket, so it must never share the
  host's path.

Apply by making the seed writable, copying these files, and setting it
read-only again. Never while a container depends on the seed.
