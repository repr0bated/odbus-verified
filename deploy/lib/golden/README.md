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
seed is never written or deleted. `--from` and `GOLDEN_ROOT` must already be on the same btrfs filesystem;
cross-filesystem CoW snapshots are refused (send/receive or a full copy would
need a separate go-ahead). The default root is `/opt/op-dbus/os` on the
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
