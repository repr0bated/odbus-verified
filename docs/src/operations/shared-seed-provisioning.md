# Shared physical seed and private leaves

Identity provisioning uses one immutable Btrfs seed on the physical partition
currently named `/dev/sdb2`. Each container has its own writable leaf. A leaf
stores that container's changes; it does not contain a copied root filesystem.
Golden preparation includes the wrapper and its helpers under `libexec/3tched`;
after release activation, the operator entrypoint is
`/usr/local/libexec/3tched/provision-workspace-subscriber`.

The seed's top-level layout is `@`, `@agent_memory`, `@agent_rules`,
`@agent_shared`, `@agent_skills`, `@agent_soul`, and `@agent_user`. Provisioning
uses these existing subvolumes in the sprout. It does not create snapshots,
send/receive root filesystems, copy seed images, or use an overlay root.
Readonly loop views reference the same physical partition bytes.

`provision-workspace-subscriber.sh` first invokes the read-only
`op-identity-root seed-preflight`, followed by `seed-profile`, before creating
an empty Incus instance with the `3tched-lungs` profile.
The helper checks the physical seed identity, SEEDING flag and block read-only
state, and refuses a direct mount of the physical source. The wrapper holds
a per-session lock; the helper serializes seed loop setup and leaf creation.
`seed-create` adds the new private writable leaf once. Later starts mount
that existing leaf; stopping does not remove a Btrfs member. Failed starts
also retain membership. The bridge only verifies externally mounted storage;
its `attached` field describes logical attachment, not device membership.
The session hostname and private persistence are
created through the sprout, so those writes belong to the leaf. Persistence is
an ordinary mode-0700 directory at `@agent_user/persist`, bound to the guest's
`/opt/run-mounts/persist`. Provisioning creates no subvolumes at all: only the
seven inherited members are used. Agent soul,
rules and shared subvolumes remain protected in the guest view.

Storage receipts identify this layout as `shared-partition-siblings-v2` and
pin its inherited subvolume identities plus the persistence directory's inode,
parent subvolume identity, sprout FSID and image metadata digest. The directory must remain on its
parent filesystem and cannot be a symlink, separate mount, or subvolume.
Older v1 receipts with an extra `@persist` subvolume and snapshot-based receipts are
rejected; this change does not silently migrate them or replace existing
container roots. Existing live persistence and older containers are neither
deleted nor converted; replacement must be prepared separately. Historical
soul reconciliation now verifies existing subvolumes and native devices only,
and fails when they are missing instead of creating them.

Before sealing a new base, `op-identity-image prepare TOP RELEASE_ID
PROVENANCE_JSON` records the archive provenance, offline package inventory,
critical file hashes and exact seven-member layout. It also prepares ordinary
mountpoint directories under `@/run/mount/3tchedFS` and `@/run/opdbus/lungs`.
The helper does not download or verify the archive signature, mount storage,
toggle SEEDING, format devices or create subvolumes. See
[image preparation](../../../deploy/workspace-image/README.md) for its input
contract. Creation fully verifies the pristine image before adding a leaf;
later checks verify the pinned metadata without rejecting legitimate private
package updates in the leaf.

FUSE runs on the host. Fresh provisioning selects a persistent Incus `disk`
device named `3tched`, exposing the session's own host FUSE mount at
`/run/mount/3tchedFS`. The seed needs an empty destination directory, not a
guest FUSE daemon. A native instance UUID pins the host source and process
receipt. An existing legacy hook receipt is verified without silently
converting its attachment mechanism. The common `/etc/3tched` directories
are a separate read-only Incus disk attachment, with links from the inherited
agent subvolumes.

The `3tched-lungs` Incus profile supplies these inherited devices:

```yaml
config: {}
devices:
  runtime-run:
    type: disk
    source: "tmpfs:"
    path: /run
    initial.mode: "0755"
  lungs-socket-dir:
    type: disk
    source: /run/opdbus/lungs
    path: /run/opdbus/lungs
    readonly: "true"
```

The bridge service creates its existing shared socket at
`/run/opdbus/lungs/lungs.sock`. Mounting its dedicated parent directory keeps
socket replacement visible to each guest without exposing other host runtime
files. `seed-profile` creates this exact profile if absent and rejects existing
configuration drift. Provisioning also verifies the inherited devices and
rejects local overrides. The `/run` tmpfs is mounted before its child devices;
image directories alone would not establish that boot ordering.

The mount does not change socket ownership, mode 0600, or the bridge's host-UID-0
peer check. Direct calls from an unprivileged guest remain refused by that
existing authentication contract. No scoped listener, relay, shared GID mapping
or permission relaxation is included. The profile and path migration are
prepared source changes, not live configuration; the complete arrangement still
needs post-deployment validation.

The native container name defaults to the session UUID. Only the service
identity `b0c777e3-0955-cd8b-3d01-ca12998cae28` may use `--container-name
3tchedCtlbot`, and its authoritative Cozo instance binding must already name
that container. The wrapper does not rewrite the binding or recreate existing
instances. Hostname remains the session UUID. Start and stop target the
validated stored native name. Incomplete replacements do not receive a
boot-ready marker or replace the retained chatbot in the boot service.

If creation fails, `op-identity-root seed-inspect NAME` reports sanitized
receipt metadata and recovery guidance without mounting, formatting or
retrying device-add. Preserve the receipt, base, leaf and mappings until the
recorded intent and actual membership have been reconciled.

Preparing or changing the live seed is a separate operator action. The helper
does not set or clear SEEDING, format the partition, or unmount an unrelated
source mount. Once leaves depend on a seed, that seed must remain unchanged.
The current source change has not been deployed or validated with concurrent
live containers on the fresh base. The September 30 file-backed and physical
seed experiments did demonstrate independent writes, peer-active remounts and
cold loop recreation using distinct read-only views of the same seed on this
kernel. That evidence does not validate this new image/layout or a full host
reboot. There is no fallback to copied seeds if a kernel rejects the shared
arrangement. The owner's current sequencing instruction defers all tests until
after deployment; a successful build alone is not a workflow validation.

See the [Btrfs seeding documentation](https://btrfs.readthedocs.io/en/latest/Seeding-device.html).
