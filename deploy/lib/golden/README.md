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

## Activation (`activate`, alias `install`)

`cut` only snapshots. `activate` puts the live slot's binaries into service:

1. Stages only the binaries named in `activate-services.tsv` for services that
   will be swapped, from `<live>/opt/op-build/target/release`, into
   `/usr/local/libexec/op-dbus/releases/<commit12>-<live-id>/bin` (sha-checked).
   No target dir, source tree, or toolchain is copied.
2. After the new live slot is in place and its catalog blobs are the ones the
   bridge will load, and before any service health check: runs that slot's
   `op-grants-materializer generate --apply-to-live` (catalog from
   `<live>/usr/share/op-dbus/plugin-blobs` if present, else
   `/dev/shm/opdbus/plugin-blobs`). The command backs up
   `/etc/opdbus/capability-grants.json` to a timestamped file, unions every
   sealed-catalog `required_capability` into `$ROOT`, validates the document,
   and waits for `opdbus-grants` `check` to report Fresh. The grants service is
   not restarted for this step. If generate, validate, or the Fresh wait fails,
   activate aborts, restores or leaves the old grants, and flips no services.
3. For each table service that is **enabled** on the host and whose run
   script execs the listed path: preserves the current binary (copy into
   `releases/pre-golden-<stamp>/bin` if it is a plain file), atomically flips
   the exec path to a symlink into the release dir, `sv -w 30 restart`s it
   (runs `./check` when present), and requires the pid to stay up for the
   `stable:N` window. On any failure every service flipped in this run is
   flipped back and restarted, and the command exits non-zero.
4. Records active/previous in `/var/lib/opdbus-release/golden-activation.json`
   (release id, commit and live id from `state.json`). The op-release kit
   ledger `slots.json` is not modified: its schema is `deny_unknown_fields`
   and its `active`/`previous` can only name kit slots `a`/`b`.

Never swapped, even if listed: 3tched-fs / threetched*, op-grpc-bridge,
cozo-ublk*, zeroclaw*, sddm*, the kernel. Activation never starts or enables
a service and never touches runit `log/` services. Dry-run unless `--apply`.

### 3tchedFS views (`fuse-swap.sh`)

The host FUSE views (`3tched-fs`, `threetched-fs`) are launched by
`opdbus-fuse-bootstrap/mount-3tchedfs`, which pins the binary by path and
SHA256 in the service's 0600 `conf` and refuses symlinks, so `activate` does
not flip them. `fuse-swap.sh <service-dir> <3tchedFS|threetched> <binary>
[--apply]` rewrites the pin, restarts with a bounded `sv restart`, checks the
mount against the capture and the sealed catalog with a stable pid, and
restores the previous conf on failure. The installed `./check` also walks
`.methods` in each `schema.json`, which the mounted documents do not carry, so
it fails for every binary; fuse-swap does not use it.
