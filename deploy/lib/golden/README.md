# Golden OS-slot release

Approved design, implemented by `golden.sh` (front doors: `./activate-golden.sh`
at the repo root and `build-golden.sh` here):

1. The release image **is the seed subvolume on the pool** — by default
   `images/@antix-seed` on 3tched-pool
   (`/var/lib/incus/storage-pools/3tched-pool/images/@antix-seed`). It is never
   sent, received, copied to another filesystem, written, or deleted.
2. `work` is a writable CoW snapshot of the seed on the **same btrfs**
   (`$GOLDEN_ROOT/work`, default `<pool>/golden-os/work`).
3. `cargo build --workspace --release` runs inside a chroot of `work`, using the
   work OS toolchain (`/root/.cargo/bin/cargo`, following
   `25-build-in-chroot.sh`; host glibc binaries will not run on the seed's glibc).
4. `work` is snapshotted read-only into the `live` slot
   (`$GOLDEN_ROOT/slots/live`).
5. The live snapshot — or, before the first release, the seed itself — is the
   OverlayFS lower dir on the live rootfs (`lowerdir=<live-or-seed>/rootfs`).
6. There are exactly two slots: `live` and `rollback`. Cutting a release
   demotes the old live snapshot to rollback and deletes the previous rollback.

There is no `btrfs send`/`receive` step and no copy under `/opt/op-dbus/os`.
`init --send-receive` was removed and is rejected with an explanation.

This is separate from op-cache `BtrfsCache` (one subvolume per category:
embeddings, blocks, queries, diffs, kv) and does not use or delete those
paths.

## Configuration

| Setting | Flag | Env | Default |
| --- | --- | --- | --- |
| Pool mount holding the seed | `--pool PATH` | `GOLDEN_POOL_ROOT` | `/var/lib/incus/storage-pools/3tched-pool` |
| Seed = release image | `--seed PATH` (alias `--from`) | `GOLDEN_SEED` | `<pool>/images/@antix-seed` |
| Release root (work, slots, overlay dirs, state) | `--root PATH` | `GOLDEN_ROOT` | `<pool>/golden-os` |
| OS dir inside seed/slots | — | `GOLDEN_OS_SUBDIR` | `rootfs` (Incus image layout; set empty for a flat subvolume) |
| Overlay lower dir | `--lower auto\|live\|seed` | `GOLDEN_LOWER` | `auto` (live release, else the seed) |
| Probe timeout for status/plan | — | `GOLDEN_PROBE_TIMEOUT` | `5` seconds |

`GOLDEN_ROOT` and the seed must be on the same btrfs filesystem: a
cross-filesystem root is refused (snapshots only work on one filesystem), and
the error points back at `<pool>/golden-os`. The filesystem is identified from
the mount table only, so the seed itself is not touched. Under
`/var/lib/incus`, the root must be a dedicated directory directly under the
pool, never `images/`, `containers/`, `custom/`, or another Incus-managed
directory. Being on a ublk/cozo-ublk device is expected for that pool release
dir; any other ublk-backed root still needs `GOLDEN_ALLOW_UBLK=1`.

When the host moves to a new pool, point `GOLDEN_POOL_ROOT` (or `--pool`) at
it; the seed must exist on that pool.

`status` and dry-run plans read the pool through hard-timed probes
(`timeout -s KILL`), and path normalization is lexical, so a pool with hung
I/O reports "unknown" instead of hanging the tool.

## Commands

Dry-run is the default. Nothing is created, deleted, snapshotted, mounted, or
built unless you pass `--apply`.

```sh
./activate-golden.sh                     # full plan
./activate-golden.sh status              # seed / work / live / rollback
./activate-golden.sh init                # work = CoW snapshot of the seed
./activate-golden.sh build               # chroot cargo build in work
./activate-golden.sh release             # snapshot work, rotate live -> rollback
./activate-golden.sh rotate              # slot rotation only
./activate-golden.sh cut --with-build
./activate-golden.sh rollback
./activate-golden.sh mount-overlay --mount /run/opdbus-os-live
# The same verbs with --apply perform the work (root required).
```

`cut`/`release` do not remount the host or restart services. `mount-overlay`
mounts the chosen lower dir with upper/work dirs under `$GOLDEN_ROOT` at
`--mount` (default `/run/opdbus-os-live`) and refuses `/`, `/usr`, `/etc`,
`/home`, `/boot`, and other host system paths. Making it the boot-time root
overlay is a separate step and is not done by this tool.

## What this tool leaves alone

- The seed (`images/@antix-seed`), `images/@seed`, Incus images/containers.
- `build-golden-payload.sh` / `system-payload.sh` — the previous payload
  stager for `/opt/op-dbus/golden`.
- `deploy/release/` — the native `op-release` kit and `@opdbus-system-a/b`
  transfer slots.
- `deploy/reseal-plugins.sh` — plugin catalog reseal.
- cozo-ublk and its services.

`--apply` on a host still needs an explicit go-ahead: it creates `work` and the
slots under `$GOLDEN_ROOT`, and for `cut`/`release` deletes only the previous
rollback slot.
