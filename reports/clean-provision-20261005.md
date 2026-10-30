# Same-key chatbot provisioning — 2026-10-05

Owner authorization: “do it” after confirming the recovery release was built
and fresh provisioning remained unfinished. Preserve the existing chatbot key
and derived SID `b0c777e3-0955-cd8b-3d01-ca12998cae28`; create fresh current
state and native `3tchedCtlbot`. No legacy persistence import or unrelated
identity/grant changes. Tests follow deployment by explicit owner instruction.

## Source preparation

- `provision-workspace-subscriber` prepares the fresh leaf through the external
  host owner, mounts inherited siblings, attaches the Incus socket profile,
  host folders and FUSE, then calls supported clean provisioning with a pinned
  root-owned plan. The normal flow starts through `touch_session` and records
  boot completion only after authoritative and native readback.
- Schema 4.5.0 adds the supported clean preflight. The owner commits fresh
  defaults/genesis through a same-key Cozo transaction and paired SHM
  publication. Private historical receipts are not imported into the new
  record or leaf. Lifecycle code verifies mounted storage; it never adds or
  removes Btrfs devices.
- Guest `BindCurrentContainer` derives identity from kernel peer credentials,
  held namespaces, native UUID and init process, with revalidation on every
  operation. Host root `Bind` also requires the host PID/user namespaces.
  Socket connect mode is 0666 beneath root-owned 0755; connection permission
  does not grant plugin capability.
- The fresh native definition sets `security.protection.shift=true`, requiring
  native ID mapping rather than an automatic recursive ownership rewrite.
  Read-only evidence from current Xray and Sept30 seed fixtures supports this
  mechanism; it is not a substitute for the fresh container's first start.
- Offline `prepare-lungs-client` stages the received client and public
  schema-only view into the existing base before image metadata/sealing. FUSE
  remains host-owned; guest MCP clients use the mapped socket without a SID1
  file. The base keeps all seven existing sibling subvolumes.

## Runtime boundary and evidence

At 05:29Z the recovery release
`20261005T044146Z-3cf5c47-bridge-recovery` remained active. Bridge and host FUSE
were stable; no replacement container or profile had yet been created.
`/dev/sdb2` remained SEEDING/block-read-only with filesystem UUID
`b6561d54-3c69-4897-a2f3-79b867e33601`, generation 40. Existing legacy receipts
refer to a different seed UUID or the retired layout; no current-seed leaf
was found. The existing source archive signature and checksum were reverified
into root-owned preparation receipts.

Full workspace release build is in progress through the delegated Cargo
worker. Authored Rust and shell regression fixtures are not yet executed.
Preparation/activation scripts are staged, not run, under
`/opt/op-dbus/recovery/clean-provision-20261005-preparation`; public build logs
are under `/var/tmp/clean-provision-20261005`.

Update 05:42Z: the first full workspace release build passed (13m53s; only
the existing unused `owner_key_for_sled` warning). A final incremental build
started 05:38:54Z to include the completed socket-origin SO_PEERPIDFD fix and
the last owner changes. The host boot helper now restores the physical
partition's read-only flag under the shared lock after verifying its identity
and existing SEEDING state. Its mocked regression is authored, unrun.

At 05:36:14Z another session changed the native chatbot alias in shared Rust,
helper and provisioner source from `3tchedCtlbot` to `Chatbot3tched`, without
the corresponding boot-service selector change. These external edits are
preserved. Root asked which name the owner intends; container creation waits
for that clarification. The current build records the new alias. Hostname and
key-derived SID remain unchanged. No new release activation or provisioning
has occurred as of this update.

## Separate workspace networking gap

The base audit confirms XFCE/LightDM and Dracula/Papirus assets; display-manager
units are masked, with obsolete SysV start links scheduled for removal during
offline preparation. General socket internet access is not installed. The
workspace Xray/UDS files remain a proposal; the existing `xray.add_inbound`
method does not author a persistent Unix inbound plus upstream route.
`zibebe/networkmanager-rs` is a NetworkManager D-Bus library, not the traffic
carrier, and is distinct from `nmrs` and `sparesparrow/rust-network-mgr`.
Do not describe the lungs socket/profile as internet connectivity.

## Owner correction and resumed work, 06:43Z

The owner selected `3tchedCtlbot`, and explicitly approved deleting the current
chatbot identity row first. Neither proposed alias is a current identity row:
the authenticated inventory shows the chatbot SID
`b0c777e3-0955-cd8b-3d01-ca12998cae28` still holds the legacy UUID-named native
binding. The human row and two historical tombstones are unrelated and retained.

The actual recorded denial is `btrfs.device_add` with missing
`cap.storage.btrfs.device.add@v1`, through the human session's lungs. Earlier
Incus-device capability speculation is superseded. The bridge handler issues
the native ioctl inside the retained seccomp guard, so adding a grant alone
would not repair this path. An explicit question is pending for the host
provisioner to perform the creation-only leaf add without changing grants or
the bridge guard. No such action has run in this resumed session.

Source now adds a typed `delete_provision_record` under the existing
`identity_sled.write` capability, restricted to a verified human, configured
chatbot key, exact revision and stopped native UUID/ETag. It commits removal
of the current identity/genesis through CAS and paired SHM publication, leaving
an audit tombstone and existing keys/grants. Startup and mint respect that
tombstone. A version3 clean-adoption plan permits fresh creation from precisely
that deletion receipt; ordinary missing records/tombstones remain refused.
The prepared alias is consistently `3tchedCtlbot`. The wrapper again uses the
actual owner preflight/recheck and refuses existing leaf creation intents;
`skipped-preflight` and the unsafe mounted-leaf ADD retry shortcut are removed.

The native workspace release build is delegated to gpt-6-luna using the
existing `target/release`; logs are in
`/var/tmp/clean-provision-delete-20261005`. New Rust fixtures are authored;
execution follows deployment per the owner's existing sequencing instruction.
This section describes source preparation only. The live row is not yet
deleted, the new release is not activated, and provisioning is not complete.

Update 06:56Z: host provisioner creation-only execution is explicitly authorized;
no grants or seccomp changes are authorized or needed. User confirms deleting
the old native containers. Deletion/preflight source accepts native absence
only from a complete authoritative inventory, and rechecks the caller after
native observation. Schema is now 4.6.0. The provisioner calls actual mounted
3tchedFS operation files and verifies receipts. First new native workspace
build passed13m20s; final incremental build pending. An abandoned Chatbot3tched
sprout exists despite its stale create-intent receipt; it must be retired with
exact file/device/native pins before offline seed preparation. Prepared cleanup
archives metadata and proves the physical seed unchanged. Nothing deployed or
provisioned yet in this resumed session.

Update 07:07Z: activation stopped before any live change. Concurrent06:59
source edits renamed the target to `3tchedChatbot`, conflicting with the
`3tchedCtlbot` selection in this conversation. Both compiler runs exited0
(13m20s,10m21s), but both frozen module hashes fail verification. No claim of
consistent source/artifact provenance is possible yet. User clarification is
pending. The isolated catalog has68plugins/no removals and changes only
identity_sled and the already-integrated Incus source_identity field. Live
SHM/persisted catalogs, release symlink and bridge guard remain unchanged.
The old Cozo row and failed candidate remain; no provisioning is complete.
