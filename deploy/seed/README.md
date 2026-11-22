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
  `threetchedfs.service.d/10-fuse-first.conf`: order is dbus, then both FUSE
  units (the base units keep `After=dbus.service`), then everything that needs
  FUSE (`op-grpc-bridge` Requires/After them). No `DefaultDependencies=no` /
  `Before=sysinit.target`: antiX dbus starts after sysinit, so that made an
  ordering cycle and systemd dropped both units. Each checks `/dev/fuse`, the catalog manifest, and the
  lungs socket, and is only treated as started once the FUSE is mounted.
- `opdbus-runtime.conf`: creates `/run/mounts`, `/run/mounts/3tchedFS`,
  `/run/mounts/threetched`, and `/dev/shm/opdbus/state` at boot, plus an empty
  `/var/lib/op-dbus/identity-cozo` (0700): `op-grpc-bridge` refuses to start without
  its durable identity owner directory (`crates/op-grpc-bridge/src/runtime.rs` `Runtime::open`).
- `sysusers.d/opdbus.conf`: creates the `secrets` group at boot. The bridge's identity
  projection needs its credential reader group (`OP_IDENTITY_PROJECTION_GROUP`, default
  `secrets`, `crates/op-grpc-bridge/src/runtime_identity.rs`); the host has it, the seed did not.
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

Binaries in the seed: `op-grpc-bridge` is the golden build of `580e206`
(`/mnt/@opdbus-system-a/opt/op-build/target/x86_64-unknown-linux-gnu/release/op-grpc-bridge`,
sha256 `9cb39ace8272…`, installed 2026-11-22 with the `deploy/provisioning/lib.sh`
`BIN_BRIDGE` override). It must match the sealed catalog's state versions
(identity_sled 4.11.0) and carries the host-only tunnel owner gate (`ccaf93e`), which
the host binary `lib.sh` names by default (`/usr/local/libexec/op-dbus/bin/op-grpc-bridge`,
`0dda1cef`) predates. The host bridge itself is not replaced. The bridge runs in the container.
`op-tunnel-door` and `op-identity-accept`
come from the golden live slot (`/srv/golden/opt/op-build/target/release`, the
chroot build of `deploy/provisioning/README.md` step 2b). `3tched-fs` is the
duplicate-path-fixed build named in `deploy/provisioning/lib.sh` (`BIN_3TCHED_FS`).

The container bridge does not own the door tunnel: the seed's `bridge.env` leaves
`OP_TUNNEL_DOOR` unset, so `op-grpc-bridge` only serves its own TLS door on
`10.0.0.2:8090` (lo) and `op-tunnel-door` splices the host door into it over
`/run/opdbus/tunnel/tunnel.sock`. Only the host bridge, which sets `OP_TUNNEL_DOOR`
(`deploy/runit/op-grpc-bridge/run`), binds that socket.

Base-OS fixes in the seed (antiX trixie rootfs, applied 2026-11-22):
- `/etc/initrd-release` is removed. antiX ships it unowned by any package; when
  it exists, systemd treats the root as an initrd, runs `initrd-switch-root`
  (fails: no `/sysroot`) and stops in emergency mode.
- `/etc/hostname` is empty. systemd sets the hostname from that file, which
  overrode `lxc.uts.name = <session>` (`deploy/provisioning/30-provision-container.sh`,
  `raw.lxc`); empty, the session hostname from provisioning stays.
- `/etc/fstab` keeps only the `/tmp` tmpfs line. antiX shipped a bare-metal fstab
  (root `LABEL=antix-seed`, plus `proc`, `sysfs`, `devtmpfs`); in a container LXC and
  the Incus `root` device own those mounts, so `systemd-remount-fs` failed (`can't find
  LABEL=antix-seed`, `/sys: permission denied`) and left the system `degraded`.
- antiX network and desktop services are purged (2026-11-22). Containers are NIC-less
  (`deploy/provisioning/README.md`); these SysV services ran under systemd's sysv
  generator and fought the container's own address setup: `connman` rewrote `lo`
  (dropping `opdbus-lo`'s address) and `avahi-daemon` joined mDNS on it. Purged, with
  `apt-get purge` offline in the unsealed seed chroot (policy-rc.d 101, nothing
  installed), then `apt-get autoremove --purge`:
  - network: `connman`, `cmst`, `ceni`, `antix-wifi-switch`, `dhcpcd`, `dhcpcd-base`,
    `ifupdown`, `pppoeconf`, `wpasupplicant`, `wpagui`, `modem-manager-gui`,
    `avahi-daemon`, `libnss-mdns`, `ipp-usb`, `rpcbind`, `nfs-common`,
    `connectshares-antix`, `ufw`
  - bluetooth: `bluez`, `bluetooth`, `bluez-obexd`, `bluez-alsa-utils`,
    `bluez-firmware`, `blueman`, `gnome-bluetooth-sendto`
  - printing/scanning: `cups`, `cups-*`, `cups-filters*`, `printer-driver-*`
    (cups-pdf, foo2zjs, gutenprint, hpcups, m2300w, pxljr, sag-gdi, splix), `sane-utils`
  - desktop/hardware: `slimski`, `slimski-themes-extras-antix`, `gpm`, `smartmontools`,
    `tlp`, `acpid`, `acpi-support`, `acpi-support-base`, `lm-sensors`
  - the 66/dinit/s6-rc service packages for the above (other inits' files)
  - autoremove: 34 orphaned libs (libnl-*, libnm0, iw, libavahi-core7, gutenprint and
    evolution-data-server libs, etc.)
  Kept on purpose: `systemd`, `dbus`, `fuse3`, `libc6`, `ca-certificates`, and every
  library `ldd` lists for `3tched-fs`, `op-grpc-bridge`, `op-tunnel-door` and
  `op-identity-accept`. `seatd` and `turnstile` stay because `polkitd` and `libseat1`
  depend on `seatd | logind`; purging them would make apt pull in a logind. No network
  config was added: no `.network` files in `/etc/systemd/network`, `systemd-networkd`
  stays disabled, and `opdbus-lo` is the only unit that touches an interface.
