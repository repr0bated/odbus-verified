---
name: incus
description: >
  Incus system containers and VMs: instance/profile YAML, disk devices,
  native storage pools and volumes, Btrfs subvolumes, lifecycle and isolation.
  Consult before creating or changing an Incus instance, profile, disk,
  storage volume, root binding or provisioning helper.
triggerPhrases:
  - "Incus"
  - "Incus disk"
  - "Incus profile"
  - "Incus storage"
  - "container root disk"
  - "native storage volume"
last_verified: "2026-10-19"
---

# Incus: configuration first, storage-driver ownership

Project-local skill authored from the official Incus documentation below;
not an imported third-party skill or a container-sandbox wrapper. Documentation
review is not proof that any example has been applied to this host. Check the
installed client/server version and applicable API extensions before using
version-dependent options.

## Start with the actual declaration

An Incus instance is configured through instance properties, `config`,
`devices` and ordered `profiles`. The CLI exposes this as YAML; do not invent
a separate authoritative YAML file on disk. Incus owns its configuration.

Read both local and expanded instance configuration. Later profiles override
earlier ones; instance-local configuration overrides profiles. Editing a
shared profile affects every instance that uses it. Do not use expanded
configuration as a blind replacement: it flattens inherited settings.

Read-only infrastructure diagnostics, within the assigned maintenance scope:

```sh
incus version
incus config show <instance>
incus config show <instance> --expanded
incus profile show <profile>
incus storage show <pool>
incus storage volume list <pool>
incus storage volume show <pool> container/<instance>
```

Confirm the selected remote, project, instance type, native UUID and stopped
or running state. A name alone is not an identity or authorization proof.
Use `--force-local` only when explicitly diagnosing this host's local daemon.

## Declare native disks under devices

Example fragment for a NEW container's root on an existing Btrfs pool named
`3tched` (not a migration instruction for an existing container):

```yaml
devices:
  root:
    type: disk
    pool: 3tched
    path: /
```

Incus creates the instance's root storage volume at instance creation. Its
Btrfs driver uses a subvolume for a container filesystem volume. Do not
manually create a per-container `.img`, GPT, `losetup` mapping or filesystem
as an assumed prerequisite for this native root declaration.

Additional managed filesystem volumes are separate objects. Create the
custom volume through the supported control plane first; its device then
names the pool, volume and guest mount path:

```yaml
devices:
  data:
    type: disk
    pool: 3tched
    source: identity-data
    path: /data
```

This references an existing custom filesystem volume named `identity-data`;
it does not create that volume. Custom volumes persist independently of the
instance until explicitly deleted. Do not casually share identity-private
volumes. A device with a host-path `source` and no pool is a host-backed
mount, not an Incus-managed custom volume.

For standalone Incus administration, the native commands are
`incus storage volume create <pool> <volume>` (filesystem by default),
`incus storage volume attach <pool> <volume> <instance> <device> <guest-path>`
and `incus config device add <instance> <device> disk pool=<pool> source=<volume> path=<guest-path>`.
These are reference syntax, NOT permission to bypass this repository's
identity-bound provisioning operations. Do not replay them against an
already provisioned identity without the authorized maintenance contract.

## Do not conflate these layers

| Object | Meaning |
| --- | --- |
| Btrfs pool backing | Existing block device, loop file or Btrfs subvolume; inspect the pool's `source`. |
| Container filesystem volume | Managed filesystem tree; Btrfs driver represents it as a subvolume. No additional loop per volume is needed. |
| Container `type: disk` device | Mount inside the container, not a VM-style emulated partitioned disk. A host block source is mounted, not automatically passed through as guest block hardware. |
| Custom `block` or `iso` volume | Attachable to VMs, not containers. Do not switch instance type to work around this restriction. |
| Btrfs VM disk | Incus uses a backing file because Btrfs does not natively provide block volumes. This is not the container-volume model. |
| Btrfs seeding/sprouting | Separate block-device filesystem operation; a subvolume is not a writable block member for `btrfs device add`. |

A loop backing an entire existing Incus pool does not imply a loop for each
container. Never detach an existing pool backing to satisfy a statement that
container subvolumes need no loops. Do not create new loop-backed pools or
per-container raw images unless that distinct layout is explicitly requested.

An Incus root declaration alone does NOT implement an external immutable
seed plus writable Btrfs member, a GPT two-partition upgrade replica, or a
read-only overlay. Name any conflict between such requirements and native
filesystem volumes before changing the design. Do not silently replace
seeding with copied roots, snapshots, or an OverlayFS provisioning fallback.
Use the `btrfs` skill for actual seed, membership and send/receive mechanics;
Incus owns its managed-volume lifecycle.

## Preserve ownership, lifecycle and admission

- Begin repository operations in the assigned identity's mounted 3tchedFS.
  Discover declared operations and arguments there. Preserve principal,
  session, grants and Cozo identity-to-storage binding. Missing admission is
  not permission to use the local Incus socket or edit Cozo directly.
- Inspect current native configuration and the provisioning owner before
  changing an existing root. Changing the `pool` key is not a storage migration.
  Preserve native UUID, session/key binding, mount ownership and receipts.
- Keep containers unprivileged, their UID/GID mapping, storage ioctl denials
  and existing device allowlist. Do not grant guest block administration,
  nesting, unrestricted host paths or daemon sockets to make storage work.
- Explicit creation may establish approved storage once. Login, boot, start,
  stop, restart, logout and recovery must not allocate new identity leaves
  or add/remove Btrfs members. Restore/verify existing storage instead.
- Do not mutate Incus-owned storage directories or live overlay lower layers
  behind the daemon. Host storage maintenance must use its supported owner,
  exact device/FSID/mount checks and explicit authorized scope.
- Before any receive, verify the intended mounted destination and its FSID;
  an existing directory is insufficient. Never receive into a fallback path
  on the host root filesystem when the intended mount is missing.
- Distinguish source configuration, accepted operation, native state and
  deployed readiness. Report the operation receipt and authoritative result;
  no claim of success from writing YAML alone. Do not auto-start containers,
  change service managers or run test suites merely to install this skill.

## 3tched Ubuntu provisioning contract

- Use Ubuntu 26.04 LTS (Resolute Raccoon) inside the container. This does not
  change the host's distribution or service manager.
- Reuse the initialized 3tched Ubuntu seed contents, including when the owner
  explicitly selects different backing storage. A generic Ubuntu root archive
  may initialize a new seed only; never use a generic image as a later
  container-root fallback. Never modify a sealed seed with retained dependents.
- Compose each root from that seed through the existing host storage owner.
  Preserve existing 3tched integrations and network declarations; install only
  demonstrated container runtime dependencies.
- Keep persistent CozoDB data on the approved identity-owned writable storage.
  Do not force a separate Cozo subvolume through `btrfs device add`: that
  operation applies only when the selected design actually adds a block member.
  An Incus filesystem subvolume is not a block-device operand. Require the
  exact approved device before adding it; do not infer permission to allocate
  a raw image, loop, LV or replacement disk.
- For an explicitly approved partitioned seed image, a host `losetup
  --find --show --partscan IMAGE` mapping exposes partition devices such as
  `/dev/loopNp1` and `/dev/loopNp2`; separate loop allocations per partition
  are not inherently required. Finalize and unmount the root filesystem before
  setting `btrfstune -S 1` on partition 1 only. Check the existing SEEDING flag
  and skip an already sealed base; never retune on login/start/reboot. The flag
  persists in the image, while kernel mappings must be restored after reboot.
  Protect only the base partition read-only, not the whole image when partition
  2 must remain writable. Never set SEEDING on the upgrade receive filesystem.
- Distinguish an immutable base partition from a writable upgrade receiver.
  Keep the receiver filesystem/parent writable for `btrfs receive`, while
  preserving received snapshots and common incremental baselines read-only.
  Receive a new version into the parent, not over an existing `@upgrade`;
  publish it only through the host-owned overlay lifecycle, without modifying
  an in-use lower tree. Track each container's receiver and baseline explicitly;
  do not infer a shared mutable receiver from a shared image/template name.
- When the owner explicitly requests a raw leaf image under the Cozo store,
  first locate the live store through supported metadata or open-path evidence;
  do not create a duplicate database or read private records to find its path.
  Put image files outside RocksDB `data`, use a private directory and exclusive
  creation, and retain the exact size/inode/operation result. A created image
  does not establish Btrfs membership or migrate Cozo data into that image.
- Distinguish direct regular-file rejection from a provisioning-policy denial.
  Installed btrfs-progs may reject a regular `.img` with `is not a block device`;
  report that real result rather than claiming completion or silently creating
  a loop/device-mapper alternative against the owner's instruction. Full
  allocation does not change a regular file's type: if a nonsparse retry is
  explicitly requested, verify allocated bytes (disable compression on the
  empty file before writing zeros), and do not repeat that allocation after
  the same type rejection.
- Reconcile matching creation receipts and verify the retained storage binding
  on repeats. Normal lifecycle operations must not repeat device-add, format
  storage or recreate roots. Interrupted intent requires reconciliation, not
  an automatic new allocation.
- Reuse existing identity, authorization and policy services. Do not build
  parallel services, mint new credentials, broaden grants or edit Cozo
  directly as part of OS provisioning. Verify the mounted filesystem and
  authoritative binding before claiming persistence is configured.

## Official references

Consult the version matching the installed daemon where behavior differs.
These upstream pages were reviewed for this skill on 2026-10-19:

- [Instance configuration](https://linuxcontainers.org/incus/docs/main/explanation/instance_config/)
- [Profiles and inheritance](https://linuxcontainers.org/incus/docs/main/profiles/)
- [Disk devices](https://linuxcontainers.org/incus/docs/main/reference/devices_disk/)
- [Btrfs storage driver](https://linuxcontainers.org/incus/docs/main/reference/storage_btrfs/)
- [Storage volume creation, attachment and content-type restrictions](https://linuxcontainers.org/incus/docs/main/howto/storage_volumes/)
