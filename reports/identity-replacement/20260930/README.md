# Identity replacement state — 2026-09-30

This is a status receipt, not a deployment or completion claim. Separate “candidate container exists” from “identity replacement is provisioned and verified.”

**22:06 UTC update:** the received release `20260930T212601Z-identity-46c8a19` is active and bridge/web/grants pass readiness. The chatbot replacement now owns its canonical session name, with the original key and anchored identity preserved; supported binding write/attach/readback and actual restart/FUSE/private-memory/protected-file checks pass. The owner's account has a completed seed-provision journal that prevents ordinary rebinding. A supported migration preserving that provenance is being implemented; no journal was erased. Authoritative inventory has two current session rows and two old revision tombstones, rather than four active accounts. Old containers remain until final verification; persistent host-boot activation is pending both identities. Detailed operator evidence is under `/var/lib/opdbus-runtime/physical-seed-preparation-20260930/release/` and release recovery under `/opt/op-dbus/recovery/identity-20260930T212601Z/`. The chronological preparation notes below describe earlier checkpoints.

## Provisioning entrypoint

`deploy/scripts/provision-workspace-subscriber.sh` now has a bounded `--system` path for standalone service containers. It defaults to the ordinary Ubuntu Noble cloud image alias `raccoon` and pool `3tched-storage`; `--image`, `--storage`, and `--no-start` are supported. It checks the exact Incus instance inventory, refuses an existing name, creates with no profiles and unprivileged/no-nesting/no-autostart settings, and does not enter the identity key, identity-directory, shared-socket, Cozo/memory, or fstorage path. This is source-only; it did not create or deploy a service container.

The POSIX shell stub suite passes, including the two argument orders, an explicit `--image` equal to the identity image default, env-selected defaults, malformed inventory refusal, option-prefixed image rejection, trailing-hyphen rejection, existing-name refusal, identity-only flag conflicts, and failure propagation. Evidence and logs:

- `deploy/scripts/tests/test-provision-system-mode.sh`
- `/var/tmp/identity-candidate-image-20260930/system-mode-tests-final.log`
- `/var/tmp/identity-candidate-image-20260930/system-provisioning-evidence.txt`

`sh -n` and `git diff --check` passed. The required workspace verify gate is blocked at `cargo fmt --all -- --check` by existing Rust formatting drift (261 reported diff sections); no formatter was applied and no clippy/test pass is claimed. Full output: `/var/tmp/identity-candidate-image-20260930/system-mode-cargo-fmt.log`. No live Incus service-container operation was performed for this change.

## Current identity candidates and storage direction

The owner withdrew the per-identity copied-seed-image plan and requires identity roots to use the shared physical `/dev/sdb2` partition, with the OS and agent defaults there and a separate initially blank writable identity leaf. The earlier temporary copied-seed builder/image/build-pool and its copied seed/leaf files were removed by root after unmount and exact loop detach. Some plans, journal entries and receipts were deliberately retained. Do not treat the imported copied-image approach as the active replacement design.

Both candidates, `i0` and `i1`, **booted successfully** in the existing `identity-roots` directory pool from the shared physical seed. Each has only its root disk and no profiles. `i0` carries display label `3tched-ctl`; `i1` carries label `1`. Incus rejected the bare numeric container name `0`. Actual names must return to the canonical session UUIDs at cutover because the current authentication contract requires that name. No Cozo data was copied or changed for them, and no WireGuard key, SID1, or grant change was made. The old Incus containers and their Cozo data remain untouched. Boot success is not yet a completed identity cutover.

Root reports one sanitized read-only `identity-base` Btrfs subvolume on `/dev/sdb2` (parent ID 267), with the seven actual agent subvolumes present: `/agent` ID 268 and its `soul`, `rules`, `shared`, `user`, `memory`, and `skills` children IDs 269–274. The agent defaults are ready. The existing `threetched-fs` binary hash is `bef3929ffd0647e4b3549d3fd8f3a3e9787e10351ba98d75c2bf9703b45abede`; the root `AGENTS.md` hash is `feed8ca7b302eb44a9f270b59ae884cb3c662b148447c94c520b7a0047c40a48`. The manual seed-maintenance receipt is `/var/tmp/opdbus-sdb2-seed-20260930T1520/`.

At 19:05 UTC `/dev/sdb2` was unmounted and marked **SEEDING**, with FSID `2dcac52c-fb0b-418a-95a4-cb6eacc80209` and generation81. Each identity now has a distinct read-only loop view of that partition and a separate initially blank 8GiB leaf under `/var/lib/opdbus-runtime/identity-leaves/`. Root-only lifecycle receipts are under `/var/lib/opdbus-runtime/seed-identities/`. Source `op-identity-root seed-create` was used as an explicit operator preparation step; the installed legacy helper was not used or replaced.

The first i0 attempt stopped before device-add because the raw source partition was still directly mounted. Its exact-owned loops were detached; the unused blank leaf and receipt were retained under `/var/lib/opdbus-runtime/physical-seed-preparation-20260930/pre-add-mount-conflict`. After unmounting that direct source, fresh creation succeeded for both candidates. The exact seed fstab row now includes `noauto` to prevent this conflict on host boot. No incomplete creation intent was replayed.

Both guests report an idmapped Btrfs root, UID0, systemd PID1 and `3tchedfs 0.1.0`. Protected nested subvolumes and private persist are present; restart/isolation verification is in progress. The seed must remain unchanged while dependent sprouts exist. Canonical-name cutover, persistent startup activation and authenticated Cozo reconciliation/obsolete-record cleanup remain outstanding.

The physical source is now selected by PARTUUID `260b98c7-b4f8-4c77-9fe7-67173c08dc90`, with the filesystem UUID checked separately. The filesystem UUID's udev symlink became ambiguous once the loop views existed; it pointed to `/dev/loop4`. The exact fstab entry also now uses PARTUUID plus `noauto`.

Protection testing found that guest root could clear a cloned subvolume's read-only flag; the test immediately restored it. A read-only nested bind alone was insufficient because guest root could unmount its view. Both candidates now have a read-only `/agent` parent and an additive Incus seccomp policy denying the three Btrfs subvolume flag/deletion ioctls, with lower-32-bit argument masks and the default deny policy retained. Guest property changes are refused even after unmounting the local view; protected entry renames fail; ordinary writes in all four private locations pass. A disposable subvolume deletion was refused and the host removed that test subvolume. This protects the actual subvolumes; guest root can still create its own namespace views. The physical source partition was never writable during these probes.

The corrected v7 helper passed its disposable fixture and real candidate durability checks. Frozen SHA-256: `ce71c46c0c3be3d5ca241252295f59af8a4312d26f9f997c5df4c3427a4d9f4d`. It permits only the exact Incus-managed container parent to have mapped-root ownership and mode0100 after boot; surrounding ancestors remain root-controlled.

The first durability run exposed an i1 seed view created through the earlier ambiguous filesystem-UUID alias: its own RO loop backed another RO loop of sdb2. After verifying both references, backing devices, FSID and generation81, the operator gracefully stopped i1 and used the helper's explicit source override for teardown only. Default-PARTUUID `seed-up` reopened the unchanged leaf against a direct partition view. No device-add, device removal, format, forced unmount or hand-edited receipt was used. Evidence: `physical-seed-validation/i1-repair-v7.log`.

The final run, `physical-seed-validation/validation-final-v7-repair1.log`, passed on both candidates. Each survived graceful stop, complete helper teardown, reattachment and Incus start while the other stayed Running. All four private locations retained their sentinels and neither guest saw the other's; protected writes, mount readbacks, file hashes and nicless checks passed. Test files were removed. Candidate metadata now records the original public key, actor class and role; no key was regenerated.

Separately, the backend start/stop owner now verifies the dedicated filesystem and retains its attachment on guest stop rather than adding/removing its populated device. The native GNU focused suite passed 34 tests, with four privileged integration fixtures ignored. The redundant private-filter compilation was cancelled and its ignored status checked through the already-built test binary. Logs: `/var/tmp/identity-sled-lifecycle-20260930/`. Native release build, scoped FUSE startup, canonical cutover and Cozo reconciliation are still pending.

An isolated writable preparation snapshot exists at `/opt/op-dbus/preparation/identity-20260930T1950`, inherited from the current golden tree. The original manifest and both current catalogs are retained under the root-only `physical-seed-preparation-20260930/release` receipt. This is preparation only: the live `current` selector and running bridge/router remain unchanged.

## Scoped 3tchedFS startup validation

A schema-only FUSE view can mount with a configured but absent lungs socket; connection happens when a method is called. The host capture used the persisted catalog and an empty private state directory, never the global private SHM state. However, pre-mounting FUSE beneath the candidate root made the installed LXC recursive idmap operation fail with `EINVAL`. That test mount was normally unmounted and i0 restarted successfully. Do not implement prestart recursive inheritance of FUSE on this idmapped root.

An installed-version `lxc.hook.start-host` probe confirmed host UID0, the initial user and mount namespaces, the exact container name/root path, and a valid `LXC_PID` before guest initialization. The temporary hook was removed afterward. A plain mount-only `nsenter` bind also failed: util-linux attempted `open_tree(CLONE)` after entering the child namespace, where the source host mount was outside the namespace. All probe mounts are unmounted and both candidates are Running.

The native implementation uses a host-private per-identity FUSE mount and clones its mount descriptor before entering the target namespace, then attaches it through pinned descriptors after idmapping. The compiled GNU helper passed the live i0 transfer. Provider listing through the guest mount returned10 providers; an identity read missing its exact grant was denied. A temporary `lxc.hook.start-host` then passed an actual stop/start and the provider call still succeeded. The temporary hook was removed, while the verified host FUSE and current guest view remain mounted. The shared privileged lungs socket never entered the guest.

The source manager now prepares that schema-only host mount, records its exact session/process/mount ownership, and normally unmounts it during explicit stopped-container teardown. Persistent hook integration and release activation remain in progress. Probe receipts are under `/dev/shm/opdbus-physical-lungs-preflight-20260930` and `/var/tmp/opdbus-lxc-hook-probe-20260930`; the standalone compiled harness is under `/var/tmp/identity-sled-lifecycle-20260930/attach-harness/`.

Both current candidate mounts also passed the exact backend `verify_dedicated_mount` implementation, with zero failures, using the native GNU harness at `/var/tmp/identity-sled-lifecycle-20260930/verify_dedicated_mount`. Activate the new lifecycle binary before rebinding Cozo to these leaves: the former stop path removes devices, whereas the new path verifies and retains each populated dedicated filesystem. The old chatbot persist mount remains available for bootstrap under its old binding until supported write/attach calls replace that binding.

The registered `btrfs_device.mount_point` identifies the top-level dedicated filesystem (`seed-identities/<session>/volume`). The guest's persistent subvolume is beneath it at `guest/opt/run-mounts/persist`; `/agent/memory` is separately persistent. Existing bridge cognitive memory remains in its authenticated host-owned Cozo store selected by `COGNITIVE_MCP_DB_PATH`; changing the identity's Btrfs metadata does not relocate that database. `cozo_id` is ownership metadata, not a database path. Preserve the existing owner keys and do not claim guest-local Cozo memory migration from a storage rebind alone.

## Shared read-only view experiments

Before final activation, the helper's normal FUSE shutdown was tested on i0:
graceful stop, `seed-down`, `seed-up`, start and observational check all passed
while i1 stayed Running. The earlier temporary FUSE mount and daemon were
removed by their owner. Persistent received-release hooks remain a cutover step.

Final boot integration requires `boot-ready.json`, pinned to the canonical
identity, native UUID, seed generation and exact leaf. A preparation receipt
alone never starts a guest. The operator publishes the marker only after
authoritative attach/readback and native verification; the `false` policy
restores storage and scoped FUSE but leaves a stopped guest stopped. Fourteen
helper fixtures and eight boot fixtures pass. Final helper SHA-256:
`9750ec334ce6a550d3ea3edcc2cdebd70ff84a3165c892a096a43849abb38c68`.

The first corrected backend activation retains the old names and bindings so
the new metadata-only inventory can inspect all identity and journal IDs.
Any repair/provision journal on a replacement target requires its supported
recovery path, including a completed journal; it is not disposable cleanup.
Canonical replacement, binding and manual obsolete-record cleanup follow
that audit. At this checkpoint the release build is still running.

Disposable file-backed and block-backed experiments for simultaneous private leaves over a shared read-only seed both pass. Logs record concurrent views/writes, reverse-order cold remounts, unchanged seed hash `a817d85c7824b3b43abff246eb1460b83c056616804b21f50da01aee72eebe8c`, and exit 0:

- `shared-seed-views/file-backed.log`
- `shared-seed-views/block-backed.log`

These are fixture results; live conversion and initial boot evidence are recorded separately above.
