# Bridge recovery — 2026-10-05

Status: corrected release deployed; bridge and host FUSE healthy; post-deployment
focused Cargo verification passed57, with5 native fixtures ignored. Full
`/verify` has not passed.
Recovery is owned by Codex at the user's explicit request. No replacement
container is being provisioned during bridge recovery.

## Cause

Release `20261005T040425Z-3cf5c47` was built from
`/srv/git/odbus-verified`. The prepared changes in
`/home/jeremy/odbus-provision-fuse` had not been integrated. Both checkouts
shared the same commit but had different uncommitted changes and binary hashes.
The release lacked the new image/profile helpers as well as the bridge repair.

Bridge startup attempted `BTRFS_IOC_ADD_DEV(/dev/sdb2 ->
/var/lib/opdbus-runtime/seed-identities/b0c777e3-0955-cd8b-3d01-ca12998cae28/volume)`.
That target was an ordinary directory on the host root filesystem, not a
mounted identity filesystem. The installed seccomp guard refused the ioctl;
the propagated startup error stopped the bridge before listeners opened.
Runit then retried. The bridge was deliberately stopped with `sv down` after
the owner assigned recovery. The guard remains unchanged.

## Source repair

Prepared Rust and deployment changes were merged into the deployment checkout
against baseline `3cf5c47`, preserving its other uncommitted work, including the
existing tombstone deletion and workstack changes. Lifecycle storage code now
verifies externally mounted storage and never adds/removes devices at startup,
shutdown or failed-start cleanup. External provisioning alone creates a leaf.

Service startup respects an inactive/parked identity. Failed chatbot activation
is logged while the bridge opens the operator interfaces needed for recovery.
Identity recovery, bootstrap, key checks and explicit activation errors remain
enforced. The runit bridge definition no longer waits for `identity-roots`.

The integrated profile/helper changes use a dedicated shared directory at
`/run/opdbus/lungs` and socket `/run/opdbus/lungs/lungs.sock`. The profile is
prepared for later provisioning; recovery does not create a live profile.
The schema declaration and plugin version now both report `identity_sled`
4.4.1; the first integration had left the schema constructor at 4.4.0, caught
and corrected before sealing.

## Evidence and deployment record

Working receipts and logs: `/var/tmp/bridge-recovery-20261005`.
Root-owned activation preparation:
`/opt/op-dbus/recovery/bridge-recovery-20261005-preparation`.
Pre-recovery tracked source diff is `canonical-before.patch`; combined source
and critical-file hashes are recorded separately. The first full native
workspace release build passed in 12m25s, with one existing dead-code warning
for `wake_doorbell::owner_key_for_sled`. The final full build after the schema
constructor correction passed in 12m58s with the same warning. Its freshly
built `opblob` sealed the isolated catalog, generation 1219. All 68 plugin IDs
remain; normalized schema comparison found only the intended `identity_sled`
version/description changes. The other 67 schemas are semantically unchanged,
despite changed hashes from JSON object ordering. No schema structures or
method names changed. Catalog hash-order nondeterminism is outside this repair.

Golden validation first rejected the stale mail bundle. Repackaging the current
release binary with the existing artifact helper resolved that check; no mail
container was changed. Golden preparation then passed, build
`20261005T043950Z`, with the isolated persisted catalog. Transfer capacity is
budgeted after golden preparation as one full received copy plus 16 GiB reserve;
the read-only source snapshot shares existing golden extents. Free space before
transfer was about 20.45 GB, with a 2.31 GB candidate. No other session's cache
or existing release was removed.

Eight safe shell fixtures passed after the initially reported deployment,
before the corrected deployment: seed profile, lungs disk, shared siblings,
owner boundaries, hostname, host folders, provisioning mode and image metadata.
No real device/profile/mount operation was performed by these fixtures.
Further tests are held until the corrected release is deployed.

The inactive preparation checkout's disposable release build intermediates
were removed under its Cargo lock to recover about 6 GiB. Its source and three
built release executables were preserved and their hashes checked unchanged.
The canonical build cache and all golden/received releases were preserved.

Before recovery activation, `/dev/sdb2` remains block-read-only, SEEDING,
FSID `b6561d54-3c69-4897-a2f3-79b867e33601`, generation 40. Recovery does not
operator-edit storage membership, identity bindings, keys or container data.
Normal bridge startup and authorized reads still produce their usual state and
audit writes.

## Activated release and live proof

Received read-only release:
`/opt/op-dbus/releases/20261005T044146Z-3cf5c47-bridge-recovery`.
`current` selects that release. Critical binaries, helpers and service inputs
match the prepared source/artifacts. Old service inputs and both catalogs are
retained under
`/opt/op-dbus/recovery/activation-20261005T044146Z-3cf5c47-bridge-recovery`.
The seccomp wrapper and BPF hashes are unchanged. The bridge runs with
`NoNewPrivs=1`, `Seccomp=2`, one filter.

Bridge PID 14689 passed the TLS door readiness check (unauthenticated 401).
The old unmounted chatbot storage is explicitly rejected and logged, while
the listeners remain available. No Btrfs member-add/remove retry occurred.
Normal startup refreshed two existing identity schema-content hashes; no
operator database edit, new identity/container, key replacement or legacy
data import was performed.

Live `/run/opdbus/lungs` is root:root0755 and its `lungs.sock` is a Unix socket,
root:root0600. Host FUSE PID 15796 runs from the corrected release and mounts
`/run/mount/3tchedFS`. An authenticated, declared read-only
`cozo.list_relations {}` through that mount succeeded: 35 relation summaries,
receipt event 205512. This proves actual FUSE-to-socket dispatch and the existing
human capability, not merely file visibility. The helper's attempts to clear
read-only result/error files emitted permission messages and were ignored by
its existing implementation; the subsequent operation receipt reports success.
Receipt summary: `/var/tmp/bridge-recovery-20261005/fuse-read-probe.json`.

Received, persisted and SHM catalog manifests match. `/dev/sdb2` is still
block-read-only. The old chatbot containers remain stopped; mail, Qdrant and
Xray remain running. No live `3tched-lungs` profile or replacement was created.

## Post-deployment verification

- `cargo fmt --all -- --check`: exit1. Formatting differences include the
  integrated `identity_sled_dispatch.rs` and `incus.rs`, plus several files from
  other work. No automatic formatting or unrelated edits were made.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  exit101 at `op-chat/src/orchestration/workstack_executor.rs:991`,
  `clippy::result_large_err`. This file belongs to the other integrated work.
- `cargo test --workspace --all-targets --all-features`: interrupted (130)
  during compilation, before tests ran, when available space reached about
  8.66 GiB. Resource reserve prevented continuing the broad build.
- Focused Btrfs tests passed4, with1 intentionally ignored native loop fixture.
  Focused identity plugin schema tests passed13. All4 socket tests passed,
  including root-only admission and refusing to replace a non-socket file.
- First bridge lifecycle run:35 passed,1 failed,4 ignored. The newly added
  parked-service regression incorrectly used the shared human fixture, then
  attempted to change its immutable principal kind. A test-only correction now
  creates the service kind initially and asserts it survives bootstrap. The
  runtime kind check is unchanged. The corrected lifecycle filter passed36,
  failed0, ignored4. The correction is inside `cfg(test)` and does not require
  redeploying the binary.
- No ignored/native destructive tests were run.

Another session committed the shared checkout as `db97951` during verification
and started its own release build. That process was not stopped or modified.
Recovery still selects the received release identified above; its manifest and
saved binary hashes are the deployed evidence. Shared-worktree HEAD is not a
substitute for that received-release provenance.

Final independent runtime verification at 05:05:05 UTC passed: current still
selects the recovery release, bridge PID14689 and FUSE PID15796 have not
restarted (1386s and1365s uptime), both readiness checks pass, exact process
executables point into the received release, socket/directory modes remain
0600/0755 with root ownership, all3 catalogs match, and sdb2 is still blockRO.
Receipt: `/var/tmp/bridge-recovery-20261005/final-runtime-proof.txt`.

## Provisioning follow-up

Latest owner direction supersedes the earlier migration proposal: reuse only
the existing keys; create a fresh container, leaf and current-schema records.
Do not import legacy persistence or container/storage metadata into the new
instance. Do not interpret this as clearing unrelated identities or databases.
The wrapper currently requires an already matching Cozo instance binding;
`write_identity` can update a leaf but cannot replace `.instance`. A clean
supported owner path is required; relaxing the wrapper or editing Cozo directly
does not implement that flow.

Host FUSE uses a root-owned socket with mode 0600 in a root-owned 0755 directory.
The host FUSE client runs as root and its filesystem projection is mounted into
the container. Direct guest clients remain unsupported by this socket's
root-only peer authorization; the current guest root maps to host UID 1000000.
Making the socket writable by that UID alone would not securely bind it to its
own identity. Guest socket access needs a verified per-identity authorization
path before it can be reported usable.
