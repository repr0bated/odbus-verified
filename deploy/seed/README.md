# Seed overlay

Files the shared seed (`3tched-pool` `images/@seed/rootfs`) carries on top of
its base OS. They were first placed by hand on 2026-11-16 during recovery;
this directory is their source.

The seed init is systemd; the host stays runit. The antiX runit and sysvinit
packages are purged from the seed. Containers have no NIC: the default
profile has only the root disk (on 3tched-pool) and the shared lungs socket.
`/run` is an Incus `tmpfs:` disk (`runtime-run`), so the opdbus binds under
`/run/mounts/opdbus/{lungs,tunnel}` are not covered by systemd's own `/run` tmpfs.

- `op-grpc-bridge.service`, `opdbus-lo.service`: the container owns its door,
  op-grpc-bridge on lo `10.0.0.2:8090`.
- `3tchedfs.service`, `threetchedfs.service`: both 3tchedFS views, mounted in
  the container over the **host's** `lungs.sock`.
- `3tchedfs.service.d/10-fuse-first.conf`,
  `threetchedfs.service.d/10-fuse-first.conf`: order is dbus, then both FUSE
  units (the base units keep `After=dbus.service`), then everything else:
  `opdbus-lo.service` and `door-tls-init.service` are `After=dbus.service 3tchedfs.service
  threetchedfs.service` (`network-pre.target` is passive and did not order them, so opdbus-lo
  used to start before dbus), and `op-grpc-bridge` Requires/After FUSE, lo and TLS. No `DefaultDependencies=no` /
  `Before=sysinit.target`: antiX dbus starts after sysinit, so that made an
  ordering cycle and systemd dropped both units. Each checks `/dev/fuse`, the catalog manifest, and the
  lungs socket, and is only treated as started once the FUSE is mounted.
- `opdbus-runtime.conf`: creates `/run/mounts`, `/run/mounts/3tchedFS`,
  `/run/mounts/threetchedFS`, and `/dev/shm/opdbus/state` at boot, plus an empty
  `/var/lib/op-dbus/identity-cozo` (0700): `op-grpc-bridge` refuses to start without
  its durable identity owner directory (`crates/op-grpc-bridge/src/runtime.rs` `Runtime::open`).
- `sysusers.d/opdbus.conf`: creates the `secrets` group at boot. The bridge's identity
  projection needs its credential reader group (`OP_IDENTITY_PROJECTION_GROUP`, default
  `secrets`, `crates/op-grpc-bridge/src/runtime_identity.rs`); the host has it, the seed did not.
- `op-tunnel-door.service`: runs the shared guest launcher `tunnel-door-run`
  (install `deploy/provisioning/runit/op-tunnel-door/run` mode 0755). Only the chatbot
  receives the control socket; user guests receive the read-only browser socket
  directory and forward to their own TLS UI at `/agent/user/OP_UI_LISTEN`.
- `opdbus-sockets.conf`: `/run/opdbus` -> `/run/mounts/opdbus`, where
  provisioning binds the host directories (`lungs` read-only, `tunnel` for
  the chatbot).
- gRPC-ready client config (2026-11-23): `etc/profile.d/op-dbus-env.sh`
  (canonical copy of `deploy/config/profile.d/op-dbus-env.sh`, previously
  missing from guests, so `guest-env` had nothing to source) plus the env
  store defaults `etc/op-dbus/env/no-proxy` =
  `localhost,127.0.0.1,::1,10.0.0.2` and `etc/op-dbus/env/ssl-cert-file` =
  the system CA bundle. There are deliberately no `http-proxy` /
  `https-proxy` settings: outbound gRPC (and everything else) dials
  directly and the host BPF egress (`ct-egress-bpf-<instance>`) carries
  it; local door (`10.0.0.2:8090`) and loopback gRPC must never be
  proxied. Internal gRPC stays on `lungs.sock` / the door, not IP. Env
  files are gitignored as a class, so the seed builders
  (`ubuntu-seeds-adjust.sh`, `deploy/lxd/finish-resolute-template.sh`)
  write these two defaults explicitly. Applied live to `@seed`,
  `@ubuntu-user`, `@ubuntu-subsidiary` (snapshots `*.pre-grpc-*` kept),
  `Jeremy` and `3tchedChatbot` on 2026-11-23.
- `nextdns.service` + `usr/local/libexec/op-dbus/nextdns-run`, `resolv.conf`: the
  official NextDNS client (`/usr/bin/nextdns`, release and SHA-256 pinned in
  `deploy/provisioning/lib.sh` `NEXTDNS_VERSION`/`NEXTDNS_SHA256`) on `127.0.0.1:53`;
  `/etc/resolv.conf` points there. Profile and device are the NextDNS context sealed in
  the identity's SID1 (NDN1, `crates/op-identity/src/sealed_id.rs:23,79-81`), read with
  `op-identity-accept` from the read-only sled, as the runit `nextdns/run` does
  (`deploy/provisioning/README.md` "NextDNS"). Never configured from the host or the
  network; the profile ID is not logged. Enable with
  `systemctl --root=<rootfs> enable nextdns.service`. Upstream DoH needs container
  egress, which the network spec does not yet provide (see `HANDOFF.md`).
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

Binaries in the seed (updated 2026-11-22 14:19 EST from golden release `daa771732f8b-301`,
the build of `daa7717`; artifacts from
`/mnt/@upper/golden-release-20261122T180452Z.hT9iSc/os/opt/op-build/target/x86_64-unknown-linux-gnu/release`;
each previous file is kept beside it as `<file>.pre-20261122-141950`):
- `op-grpc-bridge` sha256 `0ed6d37708c41a7dd101ec08f0abab180114f9211f113e74dc0f479b63a21a1d`
  (same bytes as the live host bridge; replaced beef2b7 `0ee7670d…`)
- `op-tunnel-door` sha256 `6d137319a070bbc617e0b41a3e84fb0e9ff5883c0b386736cc7be7f914035827` (replaced `eed88aa3…`)
- `3tched-fs` sha256 `b1b5a32a4ab3b3c9836a49d0f3ee8b1230d4f2110fb34e0c533fc8e69f462b82`
  (same bytes as the host 3tched-fs; replaced the duplicate-path-fixed `b37bffa2…`)
- `op-identity-accept` unchanged (`795faf89…`).
All need glibc ≤ 2.39; the seed has 2.41. The bridge must match the sealed catalog's state
versions (catalog manifest `cc7fb0f4…`, unchanged by this build). Applied with the same
unseal/copy/reseal as the overlay files, together with the `daa7717` overlay
(`/run/mounts` tmpfiles, threetchedfs units, `op-grpc-bridge.service` opdbus-lo ordering,
`nextdns.service` enabled via `multi-user.target.wants`, `nextdns-run`, `resolv.conf`) and the
official NextDNS client `/usr/bin/nextdns` 1.50.2 (tarball SHA-256 checked against `lib.sh`
`NEXTDNS_SHA256`; binary sha256 `e6b47528…`). Running containers are not changed by this.

`opdbus-lo.service` and `door-tls-init.service` (dbus, then FUSE, then everything else,
`88acb44`) were applied to the seed 2026-11-22 15:19 EST the same way; previous units kept as
`<unit>.pre-20261122-151913`.

In-place update of the running `3tchedChatbot` (2026-11-22 15:19 EST, owner choice "a"; same
Incus UUID `b781ea28…`, identity volume `identity-2a2d9aa8…` untouched, no NIC). Rollback
snapshot `3tchedChatbot/pre-optA2-20261122-151928`.
1. `incus config device set` moved `opdbus-run`, `lungs`, `tunnel` to `/run/mounts/opdbus`,
   `/run/mounts/opdbus/lungs`, `/run/mounts/opdbus/tunnel`.
2. `deploy/provisioning/30-provision-container.sh complete 3tchedChatbot` added `runtime-run`
   (`tmpfs:` on `/run`, the trusted shape without `initial.mode`; with it the host bridge refused
   adoption, so the first attempt was rolled back to `pre-optA-20261122-150902`).
3. `deploy/provisioning/35-refresh-binaries.sh 3tchedChatbot` with `BIN_BRIDGE`/`BIN_3TCHED_FS`
   from `/usr/local/libexec/op-dbus/releases/daa771732f8b-301/bin`, `BIN_TUNNEL_DOOR` from the
   release target and `BIN_ACCEPT` from the seed: the same bytes as the seed above (old files
   `.pre-20261122-151929`).
4. The overlay files listed above, `/usr/bin/nextdns`, `zcall` and its completion copied
   byte-identical from `@seed/rootfs` (old files `.pre-20261122-151931`), `nextdns.service` enabled.
Verified: startup order dbus, threetchedfs, 3tchedfs, door-tls-init, opdbus-lo, nextdns,
op-grpc-bridge, op-tunnel-door; no ordering cycle; `systemctl --failed` empty; only `lo`;
devices visible under `/run/mounts/opdbus`; 3tchedFS at `/run/mounts/3tchedFS` and
`/run/mounts/threetchedFS`; host bridge adopts and serves its FUSE after two host-bridge and
two container restarts. `nextdns` is active on `127.0.0.1:53` but resolves nothing until the
container has egress.

The container bridge does not own the door tunnel: the seed's `bridge.env` leaves
`OP_TUNNEL_DOOR` unset, so `op-grpc-bridge` only serves its own TLS door on
`10.0.0.2:8090` (lo). Only the host sets `OP_TUNNEL_DOOR` and `OP_BROWSER_DOOR`.
See `deploy/provisioning/README.md` for socket selection and existing-guest rollout.

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

`zcall` (operator CLI, `crates/op-dbus-cli/src/bin/zcall/`) is in the seed since
2026-11-22: `/usr/local/bin/zcall`, from golden release `daa771732f8b-301` sha256
`4b740d548331351951d13c62a31b4c80d19cc27e108eca7f495c5e3b0ddf78ad` (glibc ≥ 2.34; replaced the
private build `20ef8e37…`, kept as `.pre-20261122-141950`), and its bash completion
`/usr/share/bash-completion/completions/zcall` (`zcall completion bash`, sha256 `90011e90…`).
It reads plugins, methods and argument schemas only from the mounted 3tchedFS namespace
(`/run/mounts/3tchedFS`, the live mirror of the sealed blobs) and calls through the same
mount; it never reads a blob catalog directory. The running
`Jeremy` and `3tchedChatbot` containers got the same two files with `incus file push`
(owner's choice; no restart). Seed pin unchanged.

## Ubuntu systemd seeds (2026-11-22)

Two sealed systemd seeds on `3tched-pool`, from the earlier Ubuntu seed image
`/var/lib/opdbus/containers/3tchedChatbot/seed.img` (Btrfs label `3tched-seed`, SEEDING,
subvolume `@`, Ubuntu 26.04.1 LTS, fuse3/libfuse3 3.18.2). The source image is unchanged.

- `images/@ubuntu-user` (UUID `f7cca480-0273-bc48-a246-01c1bddf58fd`): user containers.
  Imported with `SEED_NAME=@ubuntu-user 10-seed-acquire.sh image <seed.img>` (a full copy;
  a snapshot cannot cross filesystems), then adjusted by
  `deploy/seed/ubuntu-seeds-adjust.sh` (run copy in `/home/jeremy/provisioning/`):
  the files under `deploy/seed/rootfs` from git HEAD (old files kept as `<file>.pre-20261122-164209`);
  `3tched-fs`, `op-grpc-bridge`, `op-tunnel-door`, `op-identity-accept`, `zcall` and its completion
  byte-identical with `@seed` (`nextdns` and the base `3tchedfs.service` already were); the same
  `multi-user.target.wants` op units as `@seed`; `default.target` -> `multi-user.target`, so the
  Budgie desktop stays installed but `sddm` is not started; `/etc/hostname` emptied. No
  `initrd-release`, no `/etc/fstab` in the image. Sealed read-only.
- `images/@ubuntu-subsidiary` (UUID `49e96bfd-20fd-4844-9e30-524f79eb0cf6`): CoW snapshot of
  `@ubuntu-user` before sealing, with `op-grpc-bridge`, `opdbus-lo`, `door-tls-init` and
  `op-tunnel-door` not enabled (subsidiary spec Req 8.4: no door in the subsidiary). The
  3tchedFS units and `nextdns` stay enabled. Sealed read-only.
- Rollback point: `images/@ubuntu-user.as-imported-20261122-164209` (read-only, as imported).
- Logs and receipts: `/home/jeremy/provisioning/runs/ubuntu-user-acquire-20261122-164053.*`,
  `ubuntu-seeds-adjust-20261122-164209.*` (receipts `exit=0`).

Not yet booted: no container has been provisioned from either seed. The 3tchedFS units still
mount over the host lungs socket, which the subsidiary spec forbids (open question).
