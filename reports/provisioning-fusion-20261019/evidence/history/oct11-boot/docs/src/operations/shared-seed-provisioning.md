# Shared physical seed and private leaves

## Manual provisioning checkpoint: partitioned instance disk

The October 11 manual workflow uses a sparse QEMU raw image under
`/var/lib/3tched-leaf-pool`, mounted from `@3tched-leaf-pool` on the host's
main Btrfs pool. This subvolume isolates the storage path, but shares that
pool's capacity. The shared seed remains the separate sealed 20 GiB partition;
no instance identity or Cozo binding is written into it.

The completed image preparation step is available through the provisioner:

```sh
sudo ./deploy/scripts/provision-workspace-subscriber.sh --prepare-disk 3tchedChatbot
sudo ./deploy/scripts/provision-workspace-subscriber.sh --inspect-disk 3tchedChatbot
sudo ./deploy/scripts/provision-workspace-subscriber.sh --prepare-filesystems 3tchedChatbot \
  --seed-device /dev/disk/by-uuid/3415d575-7224-4484-9e0d-06854ac5f032
```

Preparation creates a 60 GiB sparse raw image with GPT: 20 GiB root and the
remaining approximately 40 GiB upgrade partition. `--root-gib` and
`--upgrade-gib` select capacities on a new image. The upgrade remainder is
slightly smaller than its requested capacity because GPT and alignment consume
space. The image and a private preparation receipt live in the leaf pool;
the receipt pins the image inode and both partition identities. An interrupted
creation retains its intent and image instead of retrying or cleaning them up.
`inspect` does not register or adopt an existing image.

The filesystem stage attaches a read-only loop view of the sealed seed and adds
the blank root partition once as its writable leaf. It inherits the seven seed
subvolumes; it does not copy the root or create replacement agent subvolumes.
Only the upgrade partition is formatted, with one `@` subvolume. Shared and soul
are read-only in the leaf; memory, rules, skills and user are writable. Private
persistence directories are ordinary directories. Both mounts stay attached for
the next manual step. The receipt records loops, mounts and fresh filesystem
UUIDs. Interrupted intents require investigation, and prepared receipts cannot
repeat device-add or formatting. Never modify the seed after leaves depend on it.

Neither stage performs seed transport, Incus creation, identity registration or
startup. They have no identity/backend preflight dependency.
Those later steps remain subject to the owner's manual
script comparison. The existing identity mode described below expects an
unpartitioned leaf and must not be used with this new partitioned image.

The upgrade lifecycle is host → container only, using Btrfs without Unison:

```sh
sudo ./deploy/scripts/provision-workspace-subscriber.sh --pair-upgrade 3tchedChatbot
sudo ./deploy/scripts/provision-workspace-subscriber.sh --sync-upgrade 3tchedChatbot
```

Pairing sends a read-only snapshot of host upgrade `/dev/sdc3` (`@`) to
partition 2 during provisioning, including when the first upgrade is empty.
Subsequent synchronization sends incrementally from the retained
common baseline, including deletions within the upgrade tree. Received baselines stay read-only;
the published upgrade `@` is a writable clone. Transfer intents and prior
versions are retained for recovery; interrupted publication is not retried
automatically. Snapshot retention cleanup is not implemented by this stage.

The chatbot runit startup invokes `boot` for explicitly paired storage before
starting the container. It restores existing loop/mount attachments, synchronizes,
then publishes a read-only OverlayFS OS view at `.mounts/NAME/root-view`.
The upgrade layer takes precedence over the seed-root layer; an empty upgrade
uses the installed seed. Patches removing seed files must include OverlayFS
whiteouts: deleting a file from the upgrade tree alone reveals its seed copy.
The Btrfs layers receive the container UID mapping before mounting OverlayFS,
because this kernel rejects mapping the merged overlay. The view is bound at
the Incus-managed `identity-roots` rootfs, preserving its pool definition.
Six agent subvolumes are separately bound into `/agent`; their existing writable
or read-only policies remain in effect, with private persistence bound separately.
User contents are never sent or received. Synchronization detaches stopped OS
views before rotating a lower layer; it never changes Btrfs membership.
This new image is not yet admitted by the older unpartitioned root-owner lifecycle;
its identity-root integration remains part of the subsequent provisioning step.

The seed has the seven sibling subvolumes and guest systemd FUSE service.
`@agent_rules/rules/rules.md` and `@agent_user/user/user.md` are initial
per-instance documents: their content is changed in each private writable leaf,
not in the sealed seed. Shared owner links expose cognitive memory, skills,
routing and plugins without granting authority or starting those features.

## Earlier unpartitioned-leaf integration

The following describes the earlier helper contract. Its device paths,
host-owned FUSE attachment and early backend gates are not the new manual
partitioned-disk workflow and still require reconciliation before automation.

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

`provision-workspace-subscriber.sh` first verifies the mounted operator
3tchedFS session and sealed catalog, then calls the declared identity inventory
and provisioning preflight operations through their `call` files. It checks
the operation receipts before invoking the read-only
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
and shared subvolumes remain protected in the guest view. Rules and user
preferences are instance-specific and writable in each private leaf.

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

The bridge creates a root-owned socket with mode 0666 beneath the root-owned
0755 directory. Connection permission allows negotiation; each operation still
requires an authenticated identity and its existing capability grant. Host
`Bind(session)` requires UID 0 in the bridge's own user and PID namespaces.
Guest `BindCurrentContainer()` takes no session argument: it pins the socket's
original process with SO_PEERPIDFD and derives identity from the exact running
native container. Process, native UUID and authoritative identity are checked
again for each call. No SID1 file is installed in the guest.

Clean replacement is scoped to the configured chatbot key and service identity
`b0c777e3-0955-cd8b-3d01-ca12998cae28`, using the provisioner's configured native
alias `3tchedCtlbot`. Hostname remains the session UUID. The old native instance
must be stopped or authoritatively absent, and the new name and leaf must be
unused. Explicit retirement uses `identity_sled.delete_provision_record` through
the operator's mounted namespace, with the existing human write capability
and exact revision. This removes the current row and retains an audit tombstone;
startup respects that tombstone. A version3 plan permits fresh provisioning
from that exact deletion receipt. Ordinary missing rows are not accepted.
After host storage/mount
preparation, the wrapper submits a digest-pinned private plan through the
supported `identity_sled.provision_container` operation. The owner atomically
creates fresh current-schema state and binds it to the new native instance;
legacy persistence and record fields are not imported. Existing external
capability policy remains in force. Normal provisioning starts through
`touch_session`; `--no-start` leaves the fresh identity parked. Only verified
completion receives a boot-ready marker.

If creation fails, `op-identity-root seed-inspect NAME` reports sanitized
receipt metadata and recovery guidance without mounting, formatting or
retrying device-add. Preserve the receipt, base, leaf and mappings until the
recorded intent and actual membership have been reconciled.

Preparing or changing the live seed is a separate operator action. The helper
does not set or clear SEEDING, format the partition, or unmount an unrelated
source mount. Once leaves depend on a seed, that seed must remain unchanged.
At boot, the host storage service uses `seed-readonly` to verify the physical
partition identity and existing SEEDING state, then restore its volatile block
read-only flag before restoring opted-in roots. This never changes filesystem
membership or the SEEDING flag.
The current source change has not been deployed or validated with concurrent
live containers on the fresh base. The September 30 file-backed and physical
seed experiments did demonstrate independent writes, peer-active remounts and
cold loop recreation using distinct read-only views of the same seed on this
kernel. That evidence does not validate this new image/layout or a full host
reboot. There is no fallback to copied seeds if a kernel rejects the shared
arrangement. The owner's current sequencing instruction defers all tests until
after deployment; a successful build alone is not a workflow validation.

See the [Btrfs seeding documentation](https://btrfs.readthedocs.io/en/latest/Seeding-device.html).
