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
ensures ordinary root-owned mode-0755 directories at `@/run/mount/3tchedFS`
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
subvolume. The host-projected `/run/mount/3tchedFS` FUSE mount and per-boot
private state under `/dev/shm` are explicitly described as external runtime
mounts and are not seed contents. The `3tched-lungs` Incus profile supplies
`/run` as tmpfs and mounts the host's dedicated `/run/opdbus/lungs` directory
read-only at the same guest path. Socket permissions and authentication remain
the responsibility of the socket service.

The base should remain unchanged while any sprout depends on it. Seed recovery
tools and an external copy of the manifest must remain available outside the
seed so recovery does not depend on mounting the base itself. The helper never
seals or changes the filesystem read-only; the caller owns that lifecycle
transition.

The isolated shell fixture test is `tests/test-op-identity-image.sh`. It uses
ordinary temporary directories and a mocked `btrfs` listing; it does not mount
or modify a filesystem device.
