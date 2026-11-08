# Shared workspace image metadata

`op-identity-image` records and checks the immutable base used to seed the
seven shared-partition sibling subvolumes. It does not mount or format a
device, alter Btrfs seeding state, copy a root tree, create a subvolume, or
assign container identity. Device preparation, extraction, sealing, and sprout
creation belong to the external provisioning workflow.

The metadata lives at `@/usr/share/op-dbus/image`. `prepare TOP RELEASE_ID
PROVENANCE_JSON` is intended only for an externally prepared, unsealed base
mounted read-write at its Btrfs top level. It requires host root, refuses an
existing metadata target, and does not adopt or delete prior contents. It also
ensures ordinary root-owned mode-0755 directories at `@/run/mount/3tchedfs`
and `@/run/opdbus/lungs`, refusing symlink or non-directory components while
retaining contents of existing directories.

That path belongs to the existing immutable base metadata contract; do not
rewrite a retained seed's metadata to rename a runtime mount. The current
client units create the two runtime paths below after the guest's `/run`
tmpfs is available.

The provenance JSON must contain exactly these fields:

```json
{
  "archive_name": "ubuntu-26.04-server-cloudimg-amd64-root.tar.xz",
  "archive_sha256": "<64 lowercase hexadecimal characters>",
  "archive_url": "https://cloud-images.ubuntu.com/releases/resolute/release-20260927/ubuntu-26.04-server-cloudimg-amd64-root.tar.xz",
  "checksum_url": "https://cloud-images.ubuntu.com/releases/resolute/release-20260927/SHA256SUMS",
  "signature_url": "https://cloud-images.ubuntu.com/releases/resolute/release-20260927/SHA256SUMS.gpg",
  "signer_primary_fingerprint": "D2EB44626FDDC30B513D5BB71A5D6C4C7DB87C81",
  "verified_at_utc": "2026-10-05T00:00:00Z"
}
```

The caller is responsible for downloading and verifying those artifacts before
calling `prepare`. The helper records the downloaded archive's provenance;
it does not claim to perform the signature or archive verification itself.

Before metadata preparation, `prepare-lungs-client.sh TOP READ_ONLY_RELEASE`
installs the release's `3tched-fs` client and two public schema-only views in
the already writable offline base. Both mounts run that same binary, as two
separately supervised processes. It requires a root-controlled read-only
release under `/opt/op-dbus/releases` or `/opt/op-dbus/release-send` and reads
the catalog at `usr/share/op-dbus/plugin-blobs`. The receipt distinguishes a
retained received release from a native source snapshot; a source snapshot is
not evidence of receipt or activation. It refuses a base whose immutable image
metadata already exists, existing client/view targets, and never captures live
SHM or credentials. The enabled `3tchedfs.service` and
`threetchedfs.service` mount `/run/mounts/3tchedFS` and `/run/mounts/threetched`:

```fstab
3tchedFS /run/mounts/3tchedFS fuse.3tchedFS noauto,rw,nosuid,nodev,noexec,noatime 0 0
threetched /run/mounts/threetched fuse.3tchedFS noauto,rw,nosuid,nodev,noexec,noatime 0 0
```

`noauto` prevents competing early-boot mounts; each enabled service starts
after local filesystems and tmpfiles. `/usr/sbin/mount.fuse.3tchedFS` runs the
existing preflight and foreground client under service supervision. Manual
mounts also stay in the foreground. Unsupported mount options are rejected
rather than silently ignored. The main view excludes `threetched,tched_router`;
the upstream view includes them, matching the host adapter. Both retain the
same sealed catalog pin and use `/run/3tched-lungs.socket` for canonical writes.
The stores contain immutable declarations, not separate authoritative state.
The offline empty-state directory does not provide live SHM data-file reads.
Runtime prerequisites remain `/dev/fuse`, the lungs socket, sealed catalog,
schema-only views and empty state directory. Offline configuration checks do
not prove an identity-bound operation or a successful boot.

## Existing private Incus root maintenance

Do not use the offline-image command on a retained shared seed to update an
existing container. The same helper has a separate maintenance mode:

```sh
sudo deploy/workspace-image/prepare-lungs-client.sh --container \
  "$INSTANCE" "$NATIVE_UUID" "$ROOT_SUBVOLUME_UUID" "$OLD_CLIENT_SHA256" \
  "$READ_ONLY_RELEASE"
```

The pins are mandatory, not names inferred from an identity or replacement
storage. `OLD_CLIENT_SHA256` is the inspected existing client digest, or the
literal `absent` when no client exists. The release must be the freshly built
native client's read-only received release or source snapshot, not `target/`,
the mutable golden tree, or a live executable overlay. Compare its client
digest with the selected fresh build receipt before delivery.

This mode supports the local daemon's `default` project and its existing
native Btrfs container root. It verifies a stopped container, its native UUID,
session/key configuration presence, exclusive root volume, exact root
subvolume UUID, writable filesystem/property, and root-owned private paths.
An idmapped root whose inspected ownership is not host `0:0` fails closed;
do not change its idmap or privilege to get past that check. Raw LXC overrides,
attached disks overlapping `/usr` or `/etc`, live mounts under the private
root, symlinks, hard-linked target files, and group/other-writable targets
are rejected. The existing container configuration is pinned for the entire
delivery, not rewritten.

Only the bounded client payload is staged in a private temporary directory:
the client, two schema views, empty state directory, sealed catalog, mount
assets, fstab additions and receipt. Incus 7's native `file create`/`file push`
operations deliver it while the instance remains stopped; host root access
to Incus-owned storage is **read-only inspection**, never installation.
Existing client replacement requires the explicit digest pin. Other existing
payload targets, fstab entries and service overrides are refused rather than
overwritten. Payload bytes and service links are read back before publishing
`/usr/share/op-dbus/lungs-client.json`; the receipt records native and root
UUIDs separately. A partial delivery leaves the guest stopped and refuses
blind replay. Inspect the reported exact target rather than deleting a root
or running provisioning again.

The helper does not start the container, change profiles/devices, install OS
packages, alter grants, copy credentials, update identity records, or modify
base image metadata. Before delivery, rename the retained configured service
through the fresh sealed `identity_sled.rename_container` method, using its
existing `identity_sled.write` grant. From the inspected existing session,
UUID and current committed/native name, construct the input once and retain it:

```sh
read -r RENAME_OPERATION_ID < /proc/sys/kernel/random/uuid
RENAME_ARGS=$(jq -cn --arg operation "$RENAME_OPERATION_ID" \
  --arg session "$TARGET_SESSION" --arg uuid "$NATIVE_UUID" --arg old "$CURRENT_NAME" \
  '{operation_id:$operation,session_id:$session,native_uuid:$uuid,
    old_name:$old,new_name:"3tchedCockpitBot"}')
```

Invoke it as the existing host mount's declared local call owner, through
`3tched-fs call identity_sled rename_container --mount /run/mounts/3tchedFS
--expected-session "$HOST_SESSION" --confirm-side-effects --args "$RENAME_ARGS"`.
The owner verifies the configured key/session, anchored credential, native
UUID, stopped default-project instance, original storage receipts, current
revision and sealed schema. A private durable intent permits exactly one
native rename POST; the final name binding and receipt commit through the
existing paired identity publication. No credentials or grants are reminted.
Current name reservations follow that exact UUID while original creation
and storage receipts remain unchanged.

Repeat only the **same** input to reconcile a lost reply. A pending intent
with the UUID still at its old name is unknown, not permission for another
POST. Neither a new operation UUID nor `incus move` is a recovery mechanism.
Other native lifecycle effects are fenced until the original intent commits.
Completed replays verify the current UUID/name and may observe an already
running container. A retained legacy storage plan which derives its native
name from the immutable session id fails before the rename; this method does
not rewrite that storage layout.

Incus 7's rename additionally queues its `rename` template when no template
is pending and regenerates `volatile.cloud-init.instance-id`. The receipt
accepts exactly those daemon-owned outputs; `volatile.uuid`, identity keys,
session, devices and storage sources remain pinned. Cloud-init can rerun on
the next boot if installed; this helper does not disable it or modify its
configuration. Persistent device source filenames keep their old names.

After the completed rename receipt and bounded file delivery, start through
the existing admitted `identity_sled.touch_session` method with
`--args "$(jq -cn --arg sid "$TARGET_SESSION" '{session_id:$sid}')"`
and `--confirm-side-effects`.
It retains the existing storage/start guards and commits the observed Running
binding. Do not substitute an unowned raw Incus start after a guard refusal.

Host preparation requires Bash, Incus 7 native file commands, `btrfs-progs`,
`findmnt`, `flock`, `jq`, findutils and coreutils. The existing guest needs
the fresh client's compatible ELF loader/shared libraries, systemd,
`mount`/`findmnt`, `fusermount3`, coreutils/findutils, `/dev/fuse`, and the
already-approved read-only socket-directory attachment. Check the actual
guest OS/ABI; retaining an existing guest is not authorization to reimage it.
Host services remain runit-owned. Guest units are started by the guest's
existing init on the next authorized Incus start.

After start, check both kernel mount entries, their distinct sources, the
`guest_kernel` marker in `.3tchedfs.json`, and the same catalog hash/generation
and complete plugin partition in `view.json`. A successful mount is not
backend readiness. Perform a granted read through each mounted namespace and
retain its completed, attributed operation receipt. No SID1 file or host
`--lungs-session` is used:

```sh
incus --force-local --project default exec "$INSTANCE" -- \
  /usr/local/bin/3tched-fs describe --mount /run/mounts/3tchedFS --guest-kernel
incus --force-local --project default exec "$INSTANCE" -- \
  /usr/local/bin/3tched-fs call tched_fs get_protocol \
  --mount /run/mounts/3tchedFS --guest-kernel --args '{}'
incus --force-local --project default exec "$INSTANCE" -- \
  /usr/local/bin/3tched-fs describe --mount /run/mounts/threetched --guest-kernel
incus --force-local --project default exec "$INSTANCE" -- \
  /usr/local/bin/3tched-fs call tched_router GetState \
  --mount /run/mounts/threetched --guest-kernel --args '{}'
```

`--guest-kernel` is available on `describe`, `call`, and `invoke`. It requires
the explicit kernel-bound mode and conflicts with `--expected-session`;
the existing host-session checks remain unchanged. Transport alternatives
are still forbidden with mounted invocation. Discover the external view's
actual sealed methods and arguments before calling them. A denial, missing
native owner, unsupported method, or unknown outcome is not readiness and
does not authorize a grant change, alternate transport, or blind retry.

## Identity and runtime boundaries


Host agents use a separate projection of the same blobs. Both host and guest
operations use their identity-bound mounted operation files. MCP is retired;
the presence of a historical CLI subcommand does not make it a supported
guest interface.

Omitting `--lungs-session` selects `BindCurrentContainer`; the bridge derives
the caller from its kernel peer process and the committed native container.
The socket accepts connections with mode `0666` beneath a root-owned `0755`
directory. Connection permission conveys no plugin authority: binding and
every operation revalidate the native identity and existing grants. Explicit
session binding is restricted to root in the bridge's host namespaces.

Incus shares only the host's dedicated `/run/opdbus/lungs` directory, read-only,
at the same guest path. The installed tmpfiles rule `3tched-lungs.conf` creates
the guest-local `/run/3tched-lungs.socket` entrypoint pointing to
`/run/opdbus/lungs/lungs.sock` after `/run` is mounted. The public entrypoint is
not a second listener. Directory binding keeps socket replacement visible
after a bridge restart; binding the socket inode alone does not. Never bind
the host's entire `/run` to provide the public entrypoint.

Owner correction (2026-10-19): retain the sole `:8090` door. Do not add a
separate internet-proxy socket attachment to the seed/profile as a workaround.
`nmrs` is a NetworkManager D-Bus client, not a replacement networking daemon.
Installed NetworkManager or a configured FUSE mount alone does not prove
internet connectivity through that door.

For a headless seed, keep display-manager packages installed but mask the
installed display-manager unit and `display-manager.service`, and select
`multi-user.target` as the default. A network manager's enablement must follow
the actual selected adapter's dependencies, not a similarly named project's
documentation. Verify the existing single-door transport rather than adding a
separate egress socket; a D-Bus socket alone does not establish internet access.

`verify TOP` is the pre-seal check. It validates the exact seven Btrfs sibling
names and mount layout, metadata schema and artifact hashes, the complete
offline dpkg installed-package inventory, hashes and symlink targets for the
critical OS/init files, and neutral machine identity defaults. It reads the
target tree as files and never runs binaries from it. The package component
table marks each selected component present or missing based on the offline
dpkg database; a missing optional component is reported as missing rather
than inferred from filenames or treated as installed.

`verify-metadata TOP` is for later checks of a writable sprout. It validates
the metadata schema, pinned layout, and all metadata artifact hashes, then
prints a stable one-line JSON receipt containing `schema_version`,
`release_id`, `layout`, and `manifest_sha256`. It deliberately does not compare
installed package versions, critical OS files, hostname, machine ID, or
per-leaf contents, so ordinary guest updates and identity changes do not
invalidate the base metadata receipt. The caller should pin these exact fields
in its provisioning receipt and compare them on subsequent checks.

`inspect TOP` performs full verification and prints the manifest. The
manifest's critical hash inventory is deliberately limited to `/etc/os-release`,
`/etc/fstab`, the systemd executable, and `/usr/sbin/init`; it excludes
hostname, machine ID, SSH keys, credentials, agent data, persistence contents,
and runtime state. `@agent_user/persist` is an ordinary directory, not another
subvolume. The guest-owned `/run/mounts/3tchedFS` and `/run/mounts/threetched`
FUSE views and per-boot private state under `/dev/shm` are runtime mounts, not seed contents. The
`3tched-lungs` Incus profile supplies `/run` as tmpfs and mounts the host's
dedicated `/run/opdbus/lungs` directory read-only at the same guest path.
Socket permissions and per-connection identity authentication remain the
responsibility of the socket service.

The base should remain unchanged while any sprout depends on it. Seed recovery
tools and an external copy of the manifest must remain available outside the
seed so recovery does not depend on mounting the base itself. The helper never
seals or changes the filesystem read-only; the caller owns that lifecycle
transition.

The isolated shell fixture test is `tests/test-op-identity-image.sh`. It uses
ordinary temporary directories and a mocked `btrfs` listing; it does not mount
or modify a filesystem device.
