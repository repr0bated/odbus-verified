# Golden OS-slot release

Approved design, implemented by `golden.sh` (also `./activate-golden.sh` and
`build-golden.sh`):

1. A btrfs subvolume holds the OS (`$GOLDEN_ROOT/work`, default `/opt/op-dbus/os/work`).
2. `cargo build --workspace --release` runs inside a chroot of that OS.
3. The work OS is snapshotted read-only.
4. That snapshot is the OverlayFS lower dir forming the live system
   (`$GOLDEN_ROOT/slots/live`).
5. There are exactly two slots: `live` and `rollback`.
6. Cutting a release demotes the old live snapshot to rollback and deletes
   the previous rollback.

This is separate from op-cache `BtrfsCache` (one subvolume per category:
embeddings, blocks, queries, diffs, kv) and does not use or delete those
paths.

## Commands

Dry-run is the default. Nothing is created, deleted, snapshotted, mounted, or
built unless you pass `--apply`.

```sh
./activate-golden.sh                     # full plan
./activate-golden.sh status              # current work / live / rollback
./activate-golden.sh init --from /path/to/os-subvolume
./activate-golden.sh build
./activate-golden.sh cut --with-build
./activate-golden.sh rollback
./activate-golden.sh mount-overlay --mount /run/opdbus-os-live
# The same verbs with --apply perform the work (root required).
```

`init --from` may CoW-snapshot a seed such as `images/@seed` into `work`. The
seed is never written or deleted. `--from` and `GOLDEN_ROOT` must be on the same btrfs filesystem for a CoW snapshot.
Cross-filesystem sources require `--send-receive` (source must already be read-only). The default root is `/opt/op-dbus/os` on the
host btrfs (`@opdbus-prep` on antix1), a sibling of the historical payload
tree, not on `3tched-pool` / cozo-ublk. A root on a ublk device is refused
unless `GOLDEN_ALLOW_UBLK=1`, and even then Incus `images/` and container
paths stay forbidden.

`cut` does not remount the host or restart services. `mount-overlay` mounts
the live snapshot as an overlay at `--mount` (default `/run/opdbus-os-live`)
and refuses `/`, `/usr`, `/etc`, `/home`, `/boot`, and other host system
paths.

## What this rewrite left alone

- `build-golden-payload.sh` — the previous payload stager that fills
  `/opt/op-dbus/golden` from already-built artifacts. It never builds Rust
  and never rotates slots.
- `system-payload.sh` — helper for that stager.
- `deploy/release/` — the native `op-release` kit and `@opdbus-system-a/b`
  transfer slots. Still present; this tool does not call or delete them.
- `deploy/reseal-plugins.sh` — plugin catalog reseal; a different operation.
- `images/@seed`, Incus containers, cozo-ublk, and the live 3tched-pool data.

`--apply` on a host still needs an explicit go-ahead: it creates subvolumes
and, for `cut`, deletes only the previous rollback slot under
`$GOLDEN_ROOT/slots/rollback`.


## Seeding work from another filesystem

`--from` on the same btrfs CoW-snapshots into `work`. If the source is on
another filesystem (on antix1, `@antix-seed` lives on 3tched-pool / `ublkb1`,
while `/opt/op-dbus/os` is on `sdb3`), use:

```sh
sudo ./activate-golden.sh init --from /var/lib/incus/storage-pools/3tched-pool/images/@antix-seed --send-receive --apply
```

That `btrfs send`s the read-only source and `btrfs receive`s it under
`/opt/op-dbus/os/` (kept as the maintained release image, e.g.
`/opt/op-dbus/os/@antix-seed`), then CoW-snapshots that copy to `work`.
The source seed is not written or deleted. The source must already be
read-only; this tool will not create a snapshot on 3tched-pool or change
the seed's `ro` property.

`/opt/op-dbus/golden` and the historical `@opdbus-system-a/b` slots are
never used or deleted.

## Deploy / release (rotate + overlay)

After building inside `work` (chroot, using the work OS toolchain — host
glibc binaries will not run on the `@antix-seed` glibc):

```sh
./activate-golden.sh release          # dry-run: snapshot work, rotate slots, plan overlay
./activate-golden.sh rotate           # dry-run of the slot rotation only
```

`release` snapshots `work` read-only, demotes current `live` to `rollback`,
deletes the previous rollback, and promotes the new snapshot to `live`.
That live snapshot is the OverlayFS lower dir. `release` does not remount
the host root; `mount-overlay` is a separate verb, dry-run by default, and
refuses `/` and other system paths. `--apply` of rotate/overlay on the live
rootfs needs an explicit go-ahead.
