# Base kit and application overlays

The deployment kit is base-system content. It lives on the host root filesystem,
outside every application release overlay:

- `/usr/local/libexec/opdbus-release/op-release`
- `/usr/local/libexec/opdbus-release/op-release-launch`
- `/usr/local/libexec/opdbus-release/manifest.sha256`
- `/etc/init.d/op-release-activate`

Writable controller state remains in `/var/lib/opdbus-release`. A release must
never cover the kit or that state with an overlay. Application files are prepared
separately, transferred into release storage and activated through overlays.

After building the native GNU release controller, install the kit explicitly:

```sh
OP_RELEASE_DIR="$PWD/target/x86_64-unknown-linux-gnu/release" \
  ./activate-golden.sh --install-kit
```

`./deploy.sh --install-kit` performs the same installation. Installation stops
without preparing golden, changing services, changing the boot default, rebooting,
or activating application overlays. It holds a private root-owned `kit.lock` (shared by launchers, exclusive for
installation), then the controller's existing operation lock, refuses an unfinished release journal, retains the previous kit in a
root-only `kit-backup.*` directory, and publishes the three-file checksum manifest
last. An interrupted mixed installation fails manifest verification; repeat the
installation from the same prepared artifacts to finish it. The backup is retained
for operator recovery.

For the existing Btrfs controller, a later invocation performs application release
activation:

```sh
OP_RELEASE_DIR="$PWD/target/x86_64-unknown-linux-gnu/release" \
  ./activate-golden.sh
```

The installer does not register the boot hook in rcS. Once host prerequisites
are configured, bootstrap must enable `/etc/init.d/op-release-activate` after
local filesystems and before application consumers (the existing controller
expects rcS S12 ordering). Do not enable it before the host migration is ready.

Installing the kit does not establish release-storage, static-receiver,
first-activation or kernel approval. The current controller still requires its
approved Btrfs backing, descriptor-based OverlayFS support and checked Btrfs
retirement ABI. An ext4 root filesystem is suitable for the kit; it does not
satisfy those separate application deployment requirements. Host migration and
an alternative image/overlay backend must be implemented explicitly rather than
bypassing those checks.

## Native system release storage

The host has two system release slots: current and bumped. The inactive slot
receives the next prepared system through Btrfs send/receive; activation rotates
the slots and keeps the previous system for rollback. Both are native Btrfs
subvolumes, not Btrfs images exported over NBD. Controller state records which
physical slot is current; slot names alone do not establish activation.

For a fresh host, prepare an existing Btrfs filesystem explicitly:

```sh
./deploy/release/prepare-host-storage.sh --uuid <existing-btrfs-filesystem-uuid>
```

This creates a dedicated `@opdbus-host` preparation subvolume mounted at
`/opt/op-dbus`, a writable `golden` subvolume, a read-only empty `overlay-base`
subvolume and an administrative `release-send` directory. It writes the exact
filesystem UUID, preparation subvolume ID and two slot names into root-owned
`/var/lib/opdbus-release/storage.json`. It registers the preparation mount in
fstab and retains the old fstab. It never formats a device or adopts, deletes,
or alters historical system/seed/identity subvolumes. An existing binding,
controller state or partial preparation requires explicit reconciliation.

The two received system slots are `@opdbus-system-a` and `@opdbus-system-b` at
the Btrfs top level. They are created by release transfer, not as empty marker
directories during preparation. Every OverlayFS lower layer, including the
empty base, is verified as a native read-only subvolume. Overlay destinations
may be on the ext4 control rootfs; ext4 parent identity uses its native UUID.
The kit, controller state and credentials remain outside release overlays.

## Backend-first bootstrap

For the first release on a fresh golden tree, `OP_INCLUDE_UI=0` omits all UI
repository checks, dependency checks, builds and asset staging:

```sh
OP_INCLUDE_UI=0 OP_RELEASE_DIR="$PWD/target/x86_64-unknown-linux-gnu/release" \
  ./activate-golden.sh
```

This prepares an empty dashboard only. It refuses existing dashboard content,
including symlinks, rather than deleting a previously prepared UI. The manifest
records `omitted/backend-only` for UI source, HEAD and worktree, and uses SHA256
of the empty file listing (`e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`)
for the UI asset tree. This records deliberate omission, not a frontend build.
After the backend deployment, build and publish `3tched.com` separately. The
default `OP_INCLUDE_UI=1` retains the existing combined release behavior.

## Retired broker

EMQX is no longer used. Golden preparation does not require, fetch, extract,
or stage its broker artifact, configuration or preparation helper. The service
is retired and omitted from enabled/managed declarations; bridge runit startup
no longer prepares broker state or supplies ExHook credentials. Historical EMQX
payloads already present in golden cause preparation to stop for explicit
reconciliation. Runtime data and credentials under `/var/lib/emqx`,
`/var/log/emqx` and `/etc/emqx` are preserved.
The production bridge no longer binds the broker's private ExHook listener or
starts its committed-MQTT-wake worker; readiness requires the authenticated TLS
door and identity-bound lungs. The MQTT-only `op-chat-responder` service and
binary are retired rather than replaced with another transport.
