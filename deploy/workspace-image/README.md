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
retaining contents of existing directories. The provenance JSON must contain
exactly these fields:

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

Before metadata preparation, `prepare-lungs-client TOP READ_ONLY_RELEASE`
installs the release's `3tched-fs` client and two public schema-only views in
the already writable offline base. Both mounts run that same binary, as two
separately supervised processes. It requires a root-controlled read-only
release under `/opt/op-dbus/releases` or `/opt/op-dbus/release-send` and reads
the catalog at `usr/share/op-dbus/plugin-blobs`. The receipt distinguishes a
retained received release from a native source snapshot; a source snapshot is
not evidence of receipt or activation. It refuses existing client/view targets and never
captures live SHM or credentials. The enabled `3tchedfs.service` and
`threetchedfs.service` mount `/run/mount/3tchedfs` and `/run/mount/threetched`:

```fstab
3tchedFS /run/mount/3tchedfs fuse.3tchedFS noauto,rw,nosuid,nodev,noexec,noatime 0 0
threetched /run/mount/threetched fuse.3tchedFS noauto,rw,nosuid,nodev,noexec,noatime 0 0
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

Host agents use a separate projection of the same blobs; no copied schema or
synchronization layer is required. A guest may also serve its MCP clients with:

```sh
3tched-fs --store /usr/share/op-dbus/lungs-view mcp \
  --state /usr/share/op-dbus/lungs-empty-state \
  --lungs-socket /run/3tched-lungs.socket --all
```

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
subvolume. The guest-owned `/run/mount/3tchedfs` FUSE mount and per-boot
private state under `/dev/shm` are runtime mounts, not seed contents. The
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
