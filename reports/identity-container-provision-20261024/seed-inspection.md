# Seed inspection and lifecycle clarification

Task: `identity-container-provision-20261024`; agent Codex `/root`.
Observed: 2026-10-25T00:45–00:57Z.
Commit: `86f03d2dede738e2d780ab789dd5800b58678479` plus retained WIP.

Owner authorized inspecting the first partition while preparing the seed.
No disk writes, new mappings, mounts, service actions or guard changes were made.
All guest files were inspected offline; no guest executable was run.

## Update — 2026-10-25T01:49Z

Owner confirmed the desired outer filesystem is **ext4**, containing a regular
disk-image file with Btrfs inside, and supplied `/run/mounts/seed-3tched/`.
That directory currently exists but is empty and unmounted: findmnt resolves its
containing filesystem to `/run` tmpfs. This does not match the still-mounted
direct-LV GPT layout described below. The previously inspected root filesystem
is still at `/run/agy-mnt/dst-p1/@`; finding its FUSE units was not evidence that
the ext4-backed disk image had been completed.

Antigravity's current task is
`/home/jeremy/.gemini/antigravity-cli/brain/d4de25e9-7c55-4db3-8666-fe91d1ec5e13/task.md`.
Its task-243 log under `.system_generated/tasks/` records successful transfer of
only `@` into the direct LV partition through loop1, from backup loop2. The task
checklist remains unchecked and still says Btrfs for the outer LV; that outer
filesystem proposal is superseded by the owner's explicit ext4 correction.
The read-only inventory worker found the latest transcript unfinished at
00:47:03Z and owner instructions to keep the backup, hold sealing and leave it
writable. No completion receipt for an ext4/image conversion was found.

Current direct stat readback: `seed.img` does not exist under staging,
`/run/mount/3tched-seed/`, or `/run/mounts/seed-3tched/`. Staging contains
`seed-backup-raw.img`, a 32,212,254,720-byte regular file modified00:20:46Z.
Do not repurpose that backup as a new identity image or format the LV based on
the incomplete preparation record. No storage changes were made by this task.

## Observed destination

`/run/agy-mnt/dst-p1` was already mounted read-write from `/dev/loop1p1`.
The loop backs `/dev/dm-3`, the 40 GiB `3tched-seed` LV. Its GPT has two
approximately 20 GiB partitions. Partition 1 is Btrfs `3tched-base`, FSID
`78b59d75-bbcf-4183-941c-7e788941ecac`, without the SEEDING flag. Partition 2
is Btrfs `3tched-upgrade`, FSID `7de8757c-6b8e-43d0-82fc-47e9da564775`.
The backup `/dev/loop2` is read-only and backs
`/var/lib/cargo-build/staging/seed-backup-raw.img`; it is not the destination.

The owner describes an ext4 LV containing the partitioned Btrfs image. The
observed LV still reports `PTTYPE=gpt`, directly loop-mapped, with no ext4 layer.
An asynchronous question asks whether this is the intended destination or
whether a different image path will be supplied. Final disk preparation may
supersede these observations; do not rewrite pins from this temporary state.

## Contents

- Ubuntu 26.04.1, systemd 259.5, glibc 2.43, FUSE3, shell/core utilities,
  util-linux mount/findmnt and CA certificates are present in offline inventory.
- `3tched-fs`, both schema views and public catalog are present. Client SHA256:
  `cd4952f6f260eb246d24f391162c87a4c9146f803c68f9b69fdfa9ced1712e00`.
  Catalog manifest SHA256:
  `40427e0a12373f53470b68fe4a2d53ed3f5463bb878c261d80d4cde988100826`.
  Catalog generation 72 contains 72 plugins, including `tched_router`; this is
  not the new catalog for the unmodified native artifact.
- `3tchedfs.service` and `threetchedfs.service` are installed and enabled by
  multi-user.target symlinks. Both services, preflight, mount helper and
  tmpfiles socket rule exactly match current source. Guest default target is
  multi-user.target; display-manager.service is masked.
- `/usr/share/op-dbus/lungs-empty-state` is empty. Runtime lungs socket and
  `/dev/fuse` still depend on instance attachment; offline wiring proves no boot.
- Only `@` exists as a Btrfs subvolume. The six `@agent_*` siblings required by
  the current provisioning implementation are absent. `/agent/*` are ordinary
  empty directories; default `AGENTS.md` is absent.
- Native `threetched` binary/manifest/config location, image provenance metadata
  and `lungs-client.json` preparation receipt are absent.
- `/etc/machine-id` is populated (33 bytes), SSH host keys are present, hostname
  is `seed`, and fstab retains old root/swap UUIDs. No identity contents or key
  material were printed. These observations are not a neutral-seed pass.

## Owner's boot ordering

Provisioning establishes sender/receiver pairing. Each boot sends the next OS
version to the receiver on partition 2, then that partition supplies the upgrade
layer over the seed rootfs. The owner requested an Incus hook instead of an OS
boot trigger, and clarified that the receiver mounts inside the container.

Current source differs: `sync-instance-upgrade.sh` mounts the receiver on the
host, performs initial or incremental send/receive, then assembles mapped
read-only layers before start. `identity-roots` calls restoration at host boot.
The helper still pins the historical 30 GiB direct-LV GPT geometry and old seed
FSID; it cannot accept the currently observed 40 GiB layout or a future ext4
image without a deliberate, receipt-bound storage update.

Do not simply attach this script as an Incus hook: it queries and updates the
same Incus instance while a start operation would hold lifecycle ownership.
Independent source review of Incus 7.0.1 and LXC 7 found that ordinary pre-mount
runs in the mapped user namespace, while mount hooks run after rootfs pinning
and cannot overmount the pinned root. An earlier authenticated host-worker
handoff into the held child mount namespace is not implemented. No hook or
privilege relaxation has been added; guest FUSE remains systemd-owned.

Sources:
- https://raw.githubusercontent.com/lxc/incus/v7.0.1/internal/server/instance/drivers/driver_lxc.go
- https://raw.githubusercontent.com/lxc/lxc/v7.0.0/src/lxc/start.c
- https://raw.githubusercontent.com/lxc/lxc/v7.0.0/src/lxc/conf.c

Reviewer denial: `incus info` as jeremy UID1000 could not access
`/var/lib/incus/unix.socket`. It was not retried with privilege or another
transport. Client version 7.0.1 is verified; daemon/liblxc versions are not.

## Kernel guard request

Owner requested removal of an obsolete Btrfs classification for the ext4 LV.
Read-only `guardctl status` shows mode OFF with four pinned links. Exact trust
inventory contains only Incus pool FSID `a1ab7b42-12a8-437c-8f65-d0469892c8dd`
and host-root FSID `d53eeac0-949b-499c-b051-9fecf8678f59` for two cgroups.
There is no seed-LV entry to remove. Loaded BPF program 6's translated
instructions explicitly return without enforcement when filesystem magic is
not Btrfs (`0x9123683e`) and the device is not the Btrfs control character device.
Thus ext4 is already excluded; no code or policy removal was necessary.
Guard state was not changed or re-enabled.

Installed guard object SHA256 is
`d35507fca12d6acca0ff037ef9233830d9e8a69444d4a1bb433c21d7879f0303`.
It differs from the historical enforcement checkout's staged object, so the
loaded-program readback, not that checkout alone, supports the conclusion.
Reading the historical build source under `/var/tmp/dashboard-guard-update-20261020`
as jeremy was denied; it was not retried with elevation. The separate storage
daemon policy retains old `083161be-...` filesystem references; those are not
seed-LV guard enrollments and were not changed under this request.

## Source preparation

`prepare-lungs-client.sh` and its README now stage the protected release's
unmodified native binary and exact manifest at the sealed descriptor paths.
They use the existing public schema projection to validate descriptor hashes
without executing native or guest binaries. Native config uses an empty
private directory in existing `@agent_user/persist`, bound by guest fstab to
the sealed config path. No new subvolume or credential is created. Existing
targets are refused; staging is explicitly distinct from verified guest boot.
Final private-bind source review is clear, including corrected fail-closed
fstab parsing. Helper has not been run. No test suite or smoke fixture ran.
