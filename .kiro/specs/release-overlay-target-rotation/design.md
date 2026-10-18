# Design — Release overlays mounted at host target directories

## Overview

Each release is received into the inactive slot as it is today. It is then
exposed as one read-only overlay per target directory, mounted at the host path
consumers already use. A rotation does four things:

1. It builds the new set as detached mounts, with layers pinned by descriptor.
2. It quiesces the consumers whose inputs changed.
3. Per target, it places the new overlay beneath the old one and detaches the
   old top.
4. It stages the catalog through its existing owner, starts the quiesced
   consumers, checks health, and only then commits `active`.

Every step writes its intent durably before acting. Recovery reads the real
mount table (by tag) and the real subvolume state, and uses the journal only to
learn what was intended. One transaction lock covers the whole run. A
readiness marker gates every release service, both at boot and during a live
switch. Only active and previous are retained in the two slots. The inactive
slot is replaced only after safe reuse is established; a pinned slot blocks
replacement. On successful activation the candidate becomes active and the
former active becomes previous. Incoming receive data is temporary transaction
staging, not an additional retained generation.

The owner's rejection of the seven-hour Codex release-retirement work is an
implementation-input constraint (R13.5), not a terminology change. Do not
reuse that work or reintroduce its lifecycle under another name. Historical
source descriptions below are evidence of existing behavior, not approval to
adopt that implementation.

Everything stays inside the existing components: `op-release`,
`op-release-launch`, the rcS hook, `build-golden.sh` and the inventory
collector. There is no new daemon, no new operator command and no second
deployment list.

## Current behavior being replaced

| Concern | Today | Source |
|---|---|---|
| Exposure | one overlay at `/opt/op-dbus/current`, `lowerdir=<slot>:BASE` | `op-release.rs:361-382` |
| Switch | detach loop, then `mount(2)`, leaving a window with no mount | `op-release.rs:373-375` |
| Host reach | per-file symlinks into `current` | `local-release-activation.md:234-255` |
| Reuse guard | own-namespace mountinfo only | `op-release.rs:178-184, 303-306` |
| Restart | every enabled `current/sv/*`, after the switch, with no health check | `op-release-launch:49-58` |
| Durability | state saved after mount changes; crash at `:307-310` loses the received release through `abort` | `op-release.rs:307-336` |
| Golden prep | unconditional reinstall | `build-golden.sh:562-608` |

## [A] Targets and path contracts

### Candidate target set (needs owner approval, O1)

Class-O targets, each its own overlay:

| Target | Golden subpath | Contents |
|---|---|---|
| `/usr/share/op-dbus` | `usr/share/op-dbus` | manifests, agents, graphs, shipped catalog (`plugin-blobs/`), immutable config (`etc/opdbus/`, `etc/op-dbus/`, `emqx-etc/`) |
| `/usr/local/libexec/3tched` | `libexec/3tched` | helpers |
| `/usr/local/libexec/op-dbus/bin` | `bin` | release binaries |
| `/usr/local/libexec/op-dbus/threetched` | `usr/bin` (only `threetched`) or a dedicated subdirectory | `threetched` |
| `/usr/local/libexec/op-dbus/sv` | `sv` | service definitions |
| `/usr/local/share/op-dbus/dashboard-release` | `usr/local/share/op-dbus/dashboard` | UI |
| `/opt/emqx` | `opt/<emqx-install>` | EMQX install (pending task 0.4) |
| `/opt/opdbus` | `opt/opdbus` | provider installs |
| `/opt/op-dbus/execution-python` | `opt/op-dbus/execution-python` | Python environment |

Nested destinations are placed *inside* one target's subtree, so no target
nests another (R1.2). Golden staging moves class-F content into these subtrees
(task 2.2).

### How consumers reach content (R1.4–R1.6)

1. **Existing settings**, set to the new absolute paths in the run scripts.
   These ship in the `sv` target, so they rotate with the release:
   - `OP_GRANTS_SOURCE`
   - `OP_MCP_AUDIENCE_POLICY_PATH` and `OP_MCP_TOOLSETS_PATH`
   - `OP_WEB_STATIC_DIR`
   - `THREETCHED_BIN` and `THREETCHED_MANIFEST`
   - `TCHED_ROUTER_BOOTSTRAP_CONFIG` and `TCHED_ROUTER_BOOTSTRAP_SKILLS`
   - `OP_MODEL_PROVIDERS_PATH`
2. **Absolute release paths** in run scripts, replacing `/usr/local/bin/<x>`
   and `/opt/op-dbus/current/...`:
   - `incusd/run:16`
   - `opencode-door/run:7`
   - `paseo-door/run:6`
   - `threetched-fs/run:6-7`

   The `PATH` that `tched-router/run:49` builds gains
   `/usr/local/libexec/op-dbus/bin` ahead of `/usr/local/bin`.
3. **Fixed runit stubs** in `/etc/sv/<svc>`, installed once and never changed
   by a release. The `supervise/` and log state stay as they are:

   ```sh
   #!/bin/sh
   # fixed stub; the release ships /usr/local/libexec/op-dbus/sv/<svc>/run
   r=/run/opdbus-release/ready
   case "$(sed -n 1p "$r" 2>/dev/null)" in
     complete\ *) ;;
     transition\ *) grep -qx 'allow <svc>' "$r" || ! grep -qx 'q <svc>' "$r" || exit 111 ;;
     *) exit 111 ;;
   esac
   exec /usr/local/libexec/op-dbus/sv/<svc>/run "$@"
   ```

   In `transition` state the marker lists every member of Q (`q <svc>`) and the
   members already admitted (`allow <svc>`). A service runs if it is admitted,
   or if it is not in Q at all.

   `finish`, `check` and `log/run` stubs differ only in the file name; `finish`
   and `log/run` skip the readiness check. runit starts `run` with the service
   directory as its cwd, and `exec` keeps it. The readiness check is inline, so
   it adds no pinned artifact. The exact format of `ready` is fixed in task 2.7.
   Task 2.8 must
   check every `$0`-relative run script; `notebook-sources-sync/run:13` already
   uses `readlink -f "$0"`.

### Catalog contract (rows 14a and 14b)

- The shipped catalog lives at `/usr/share/op-dbus/plugin-blobs`, read-only and
  rotating with the release.
- The persisted catalog stays at `/var/lib/opdbus/plugin-blobs`. It is
  writable, owned by `opblob`, and never a target.
- Activation, and every revert, runs
  `opblob persist /usr/share/op-dbus/plugin-blobs /var/lib/opdbus/plugin-blobs`,
  then `opblob stage-shm`. This is the existing flow
  (`local-release-activation.md:273-275`). The live and persisted manifests are
  verified before any consumer starts.
- Consumers read the persisted catalog or SHM. `threetched-fs/run:7` is
  rewritten from `current/var/lib/opdbus/plugin-blobs` to the persisted path,
  so there is one consumer-facing catalog.
- `build-golden.sh:55` keeps reading the persisted catalog as its source.

### Grants contract (O2)

`/etc/opdbus` cannot be overlaid: it holds host keys and secrets. The release
ships `capability-grants.json` (`build-golden.sh:742-744`), and
`op-grants-materializer` reads `OP_GRANTS_SOURCE`
(`op-grants-materializer.rs:142-144`). Two consistent contracts are possible:

- **(G1) Release-owned policy.** The materializer reads
  `/usr/share/op-dbus/etc/opdbus/capability-grants.json`. Rotation and revert
  change the grants, and the materializer is in C when that file changes.
- **(G2) Host-writable state.** The release stops shipping the file.
  `/etc/opdbus/capability-grants.json` stays host state, outside this spec.

The two cannot both hold. The owner picks one (O2). MCP audience and toolset
policy follow G1 regardless, through their existing settings.

### ZeroClaw boundary

The release ships only `threetched`, at its class-O location. No target is
`/usr/local/sbin` or `/var/lib/threetched`, and the R1.1 refusal list enforces
that. `tched-router` state is `/var/lib/tched-router`, which is untouched.
Whether `tched-router` should exec the owner-installed `zeroclaw` is a separate
question, not decided here.

### `/opt/op-dbus/current` (O4)

The recommendation is to remove it. After task 2.8 nothing consumes it:
- inspection uses `/run/opdbus-release/slots/<active>` and `op-release status`;
- service enumeration reads the inventory.

Its dangling symlink is preserved and recorded at first activation (R10.4).

## [B] Layers and mount API (R2, R3)

Path-based `mount(2)` is not used, for three reasons:
- it resolves layers by path at mount time, after verification has happened;
- it always attaches the mount, so nothing can be built invisibly;
- it reports errors only as `errno`.

Each target is built with the new mount API instead:

```text
slot_fd  = verified slot root (subvolume id, received_uuid, read-only checked)
layer_fd = open_beneath(slot_fd, "<subpath>", O_DIRECTORY|O_RDONLY)   # objects.rs:24-30
           fstat: directory, same st_dev as slot
fs_fd    = fsopen("overlay", FSOPEN_CLOEXEC)
fsconfig(fs_fd, SET_STRING, "source", "opdbus-release:<slot>:<release>:<target-id>")
fsconfig(fs_fd, SET_FD,     "lowerdir+", layer_fd)                    # v6.13+
[fsconfig(fs_fd, SET_FD,    "lowerdir+", base_fd)]                    # only if task 1.2 requires it
fsconfig(fs_fd, SET_STRING, "redirect_dir", "off"); "index" "off"; "metacopy" "off"
fsconfig(fs_fd, CMD_CREATE)
mnt_fd   = fsmount(fs_fd, FSMOUNT_CLOEXEC, RDONLY|NODEV|NOSUID[|NOEXEC])
record st_dev(mnt root) in the slot record                            # R6.6
```

The target mount point is opened with `O_PATH|O_NOFOLLOW` component by
component. Its `st_dev` and mount ID are checked against the registered parent,
and it is used with `MOVE_MOUNT_T_EMPTY_PATH`. `slot_fd`, `layer_fd` and
`mnt_fd` stay open until the target's step completes.

Crates: the workspace has `libc 0.2.189` and `nix 0.25`–`0.28`. Whether these
expose the syscalls, or raw `syscall()` is needed, is a detail for task 2.3.
No new crate is added.

### Bind-mount alternative (for O6)

A read-only overlay with one lower and no upper shows exactly that directory.
A read-only bind does the same: `open_tree(layer_fd, "", OPEN_TREE_CLONE|AT_EMPTY_PATH)`
plus `mount_setattr`. It needs no module and goes through the same
placement path.

## [C] Switch mechanisms (R4, R7)

| | (a) unmount, then mount | (b) stack over, detach buried | (c) detached build, `move_mount` into place |
|---|---|---|---|
| Visible gap per target | yes: the covered empty directory | no | none with beneath placement (c2) |
| Mount count over 100 rotations | constant | grows unless the buried mount can be detached alone; no path addresses it, and a fd-based detach may take the top with it [inferred] | constant |
| Invisible build before change | no | no | yes |
| Rollback | same gap | same leak | same operation toward `from` |
| Verdict | rejected (R4.2) | rejected unless task 1.4 shows a safe buried detach | **recommended**, as (c2) |

**(c2), per target:**
1. Record the intent.
2. Place the candidate beneath the top with `move_mount(mnt_fd, "", target_fd, "", F_EMPTY_PATH|T_EMPTY_PATH|MOVE_MOUNT_BENEATH)`.
3. Record that it is placed.
4. Detach the old top with `umount2(target, MNT_DETACH)`.
5. Record the move.

On first activation the target is not yet a mount point, so placement is a
plain `move_mount`.

Preconditions on this host:
- targets are below `/`;
- `op-release` runs in the host namespace;
- the candidate is freshly made and has no overmounts;
- propagation is private.

Task 1.3 confirms each per parent filesystem.

Evidence that would confirm the recommendation:
- **1.3:** beneath placement plus top detach works at `@rootfs` and `@opt`
  fixture parents. The record shows the post-placement topology, including
  whether the old top becomes a child of the candidate.
- **1.4:** (a) shows the gap, and (b) either leaks or loses the top.
- **1.7:** over 100 or more primitive rotations the mount counts are constant.
- **1.8:** under the straddle readers there is no `ENOENT` or `ESTALE`, and
  every read is either whole-old or whole-new.

### Transaction lock (R15)

The launcher opens `/var/lib/opdbus-release/lock` on fd 9 and takes
`flock -n 9` before planning. It holds that lock until commit, revert or
recovery is finished. Every `op-release` child inherits fd 9 and checks two
things:
- fd 9 has the lock file's device and inode;
- `flock(9, LOCK_EX|LOCK_NB)` succeeds on that same open file description.

The child does not open the file again: a second open file description would
conflict with the launcher's lock (`op-release.rs:109-114` does that today). So
`sv` actions, mounts, catalog steps, health checks and commit all happen inside
one lock. `boot` takes the same lock. `status` reads without it.

### Rotation sequence

All of this runs under one lock, held by the launcher.

```text
lock     launcher: exec 9>lock; flock -n 9                                     (R15)
recover  if a journal exists: run R5.2 recovery first, then stop
plan     C, transitive Q, wanted state of Q (R8.3, R8.6); abort if any Q member is paused or
         its starts/deps chain reaches a want-d service
build    build every target detached (R4.1); any failure → close fds, done, nothing changed
intent   journal {from, to, order, Q, wanted_state, catalog_from, catalog_to}   ← durable
ready    transition <from> <to>                                                 (R9.7)
quiesce  sv down Q (reverse dep order), confirm down; failure → restore wanted state, ready=complete(from), done
for each target k in order:
  intent   journal {k, step: place_beneath, expect_tags}                        ← durable
  place    move_mount BENEATH
  intent   journal {k, step: detach_top}                                        ← durable
  detach   umount2(top, MNT_DETACH); on failure → R5.4
  done     journal {k, moved}                                                   ← durable
catalog  intent → opblob persist <to shipped>; stage-shm; verify manifests; done
start    per Q member in dep order: add to ready allow-list, then restore its wanted state; sv check + probe
commit   journal cleared, previous=from, active=to                              ← durable
ready    complete <to> <release> <catalog-sha256>
unlock
```

Each journal write is an fsync'd temporary file, renamed into place, followed
by a directory fsync (`op-release.rs:99-107`). Recovery compares the journal
with the tags actually mounted at each target (R5.2). Readiness is written to
tmpfs with the same rename-into-place step.

### Readiness during live activation and at boot (R9.6–R9.9)

A stub starts its service only if one of these holds:
- the state is `complete`;
- the state is `transition`, and the service is not in Q;
- the state is `transition`, and the service is on the allow-list.

Services outside Q read only content that is identical in both sets.
Catalog readers are consumers of changed catalog entries, so they are always in
Q. As a result, a crash-restarted service outside Q can come back mid-switch
safely, and no service in Q can start early, whether runit or another run
script's `wait_dep` triggers it.

At boot, `complete` additionally requires that the persisted catalog manifest
equals the committed release's catalog digest. After an interrupted activation
that had already run `opblob persist` for candidate B, boot mounts committed A,
sees B's catalog, and writes `recovery-pending catalog-mismatch`. No release
consumer starts. The next guarded run restores A's catalog through `opblob`.

### What readers can see

- **Q (declared, changed):** down for the whole switch. It starts only on the
  complete new set and staged catalog.
- **Declared consumers outside Q:** they read only entries that are identical
  in both inventories, so they see the same content whichever generation a
  target shows.
- **Undeclared readers:** the per-open-file guarantee only (R4.4). Nothing
  across targets. The multi-target switch is not atomic and is not described
  as atomic.

### Failure handling

| Failure | Action |
|---|---|
| Build fails | close fds; no visible change |
| Quiesce fails | restore the wanted state of members already stopped; readiness `complete <from>`; abort |
| Placement fails at k | the candidate was never attached; revert 1..k-1 |
| Top detach fails at k | retry under the lock. If it still fails, leave old visible and candidate beneath, mark `stacked`, keep `to` pinned, revert the other moved targets, refuse further rotation until resolved (R5.4). Never lazily detach the candidate. |
| Catalog or health fails | `sv down` the members of Q that started; revert all moved targets; restore the `from` catalog and verify; restore Q's wanted state; readiness `complete <from>` (R5.5) |
| Revert, catalog restore or old health fails | leave affected consumers down, keep the journal, readiness `recovery-pending`; record per-target tags, catalog manifest and down services; report; no retry loop (R5.6) |
| First activation fails (`from` is `null`) | restore the pre-activation state from the preservation record ([G], R10.6); readiness `none` |
| Crash anywhere | the kernel drops the lock. The next guarded run takes it, ensures Q is down, reconciles each target from its tags, reverts to `from` (or restores pre-activation state when `from` is `null`), restores the catalog, and restores Q's wanted state. `boot` mounts `active` only and writes `recovery-pending`. |

## [D] Two-slot reuse safety (R6)

### Lifecycle

Every release rotates the target overlays. The only retained generations are
active and previous, in `@release-a` and `@release-b`. On a successful switch,
the candidate becomes active and the former active becomes previous.

Before the inactive slot can receive new contents, all references to its old
backing tree must be released. An unchanged service can still hold the slot
it originally started from, so a narrow re-home restart may be needed before
replacement. A pinned slot otherwise causes refusal, not deletion or a
rename-aside generation.

### Checks before replacement

`op-release` uses a detached top-level mount descriptor rather than exposing
all slots through `/run/opdbus-release/top` (R6.1). Task 1.6 verifies the
mount-namespace behavior. This avoids introducing a new host pathname to the
slots; it does not by itself prove the absence of existing holders.

Mount-namespace and process checks identify holders and fail closed when
required information cannot be read. They are not an atomic lifetime barrier:
a reference can move to an already-scanned process. Repeating a scan does not
remove that limitation.

Safe reuse must be established before deletion, as R6.10 requires. Task 1.6
must demonstrate the enforcement used for that condition; if it cannot, the
replacement path remains blocked. The kernel's deleted-subvolume list is used
only to confirm cleanup after an already-authorized delete, not to discover
whether that delete was safe.

### Sequence, under the transaction lock

1. Verify that the replacement target is the inactive slot, never active.
2. Remove this run's pathname exposure and perform the mount, process and
   declared-consumer checks (R6.2–R6.4). Any holder or uncertainty refuses.
3. Establish the pre-delete lifetime condition (R6.10). A negative scan alone
   does not permit proceeding.
4. With the incoming receive already verified, durably record intent, delete
   the unreferenced inactive subvolume by ID, and confirm cleanup (R6.5–R6.7).
5. Durably record the install intent, rename the verified incoming subvolume
   into the inactive slot name, and record completion.
6. Rotate the overlays, stage the catalog and check health through [C]. Commit
   the candidate as active and the former active as previous.

There is no additional retained generation. R5.2 handles a crash between
inactive-slot deletion and installation from the verified incoming receive.
R5.10 reports the temporary rollback limitation while that slot is replaced.

### Pinned inactive slot (O3)

- **R (recommended):** re-home exactly the identified declared host-namespace
  services onto the committed active release, preserving wanted state, then
  re-evaluate the complete safety gate.
- **F:** refuse replacement without changing active or previous contents.

Foreign namespaces and processes are never entered, signalled or modified.
Neither O3 option waives R3.1 or expands retention beyond active and previous.

## [E] Inventory and restart policy (R8)

### Collector changes (`crates/op-cozo-store/src/bin/inventory/mod.rs`)

- Add `uid` and `gid` to every entry. Today only `mode` is recorded
  (`:233`).
- Add `release_target`. Keep `target` meaning the literal symlink destination
  (`:258-267`).
- Add `consumers[]` per entry. Add `deps[]`, `starts[]` (parsed from
  `wait_dep`/`sv start` in the staged run file) and an optional `probe` per
  service. These come from declarations next to `deploy/runit/<svc>/`.
- Keep `sensitive()`, the `MANIFEST` exclusion and the `graphs` skip unchanged.

### Change detection

An entry is changed if any part of its tuple differs between the active and
candidate inventories:

```text
(presence, file_type, mode, uid, gid, size, sha256 | hash_status, target)
```

Directories compare presence, mode and owner. A missing hash
(`protected_content_not_read`, `excluded_to_avoid_graph_digest_recursion`) is
always **changed**, never equal.

### Selection

```text
C  = consumers(changed) ∪ {svc : sv/<svc>/* changed} ∪ deps*(…)
Q₀ = C
Qₙ₊₁ = Qₙ ∪ {s linked in runsvdir/default : (starts(s) ∪ deps(s)) ∩ Qₙ ≠ ∅}
Q  = the fixed point (transitive auto-start and dependency closure)
```

`starts[]` comes from the `wait_dep` / `sv start` calls in each staged run
file (`emqx/run:5-13`, `identity-roots/run:11-13`, `incus-ct-*/run`).
Ordering follows `deps`: grants and the bridge before EMQX and op-web
(`local-release-activation.md:276-282`).

### Wanted-state preservation (R8.6)

Before the quiesce, the launcher reads each Q member's `supervise/status`:
pid, paused flag, want (`u` or `d`), and run or finish state. It also records
whether a `down` file exists and whether the service is linked in
`runsvdir/default`.

Restore follows the wanted state, not the observed process state:

| Recorded | Restored with |
|---|---|
| want `u` | `sv up` |
| want `d`, running | `sv once` |
| want `d`, stopped | nothing |

The plan aborts before any change if:
- a member is paused; or
- the transitive `starts`/`deps` chain of any member reaches a service with
  want `d`. Starting the member would `sv start` that service and change its
  wanted state.

`down` files and links are never changed.

### Placement of logic

- `op-release` computes the plan, as JSON, and performs the mount and subvolume
  steps.
- The launcher runs the `sv` steps outside the trusted cgroup, as it does today
  (`op-release-launch:49`). It holds the transaction lock (R15) for the whole
  run. It becomes:

  ```text
  lock → recover → plan → ready=transition → quiesce → switch → catalog → start → commit → ready=complete → unlock
  ```

  with explicit failure branches. It no longer attaches
  `/run/opdbus-release/top` (`op-release-launch:31-37`); `op-release` uses a
  detached top-level mount instead ([D]). This changes a pinned file (O5).

## [F] Boot (R9)

`S10mountall.sh` (fstab, including `@opt` and the `/var/lib` mounts) runs
before `S12op-release-activate`. Then `op-release boot`:

1. Takes the transaction lock. If the lock is busy, it writes
   `recovery-pending lock-busy` and stops.
2. Reads `slots.json` and uses the committed `active`. If `active` is `null`,
   it writes readiness `none` and stops.
3. Mounts the slot read-only at `/run/opdbus-release/slots/<slot>`.
4. For each target, by path depth:
   - verify the parent mount's UUID and subvolume;
   - build the overlay as in [B];
   - attach it with a plain `move_mount`.
5. Computes readiness (R9.6) from three checks:
   - every target is mounted with its tag;
   - the persisted catalog manifest SHA-256 equals the active release's catalog
     digest;
   - no journal exists.

   It writes `complete` or `recovery-pending <reason>`. Boot is never blocked.

`boot` does not join a cgroup, delete anything, recover, or run `opblob`.
Recovery waits for the next guarded launcher run, which the operator triggers
with any verb.

## [G] First activation (R10)

First activation runs inside `release`, under the transaction lock. It covers
only the targets listed in the owner's
`/var/lib/opdbus-release/first-activation.json`. That record also names the
services to link in `runsvdir/default`.

1. **Intent.** Durably record per target:
   - source path, type, mode and owner;
   - a per-file SHA-256 tree;
   - literal symlink targets;
   - destination and restore steps.

   Also record:
   - the prior persisted catalog (absent, or moved aside on the same subvolume);
   - prior SHM catalog presence;
   - every `/etc/sv/<svc>` file a stub replaces;
   - every link to be added.

   Destinations are on the same subvolume:
   - `@rootfs` paths → `/var/lib/opdbus-release/preserved/<ts>/`;
   - `@opt` paths → `/opt/op-dbus/preserved/<ts>/`.
2. **Move** each original with `rename`, never a copy. Create the empty
   root-owned directory (mode 0755). Install the stubs. fsync. Record
   completion.
3. **Rotate** with `from` set to `null`: no quiesce of prior release consumers,
   because none exist. Then place each target (plain `move_mount`), persist and
   stage the catalog, add the approved links, start in dependency order, and
   run health checks.
4. **Commit.**

Failure at any step, including health, restores the pre-activation state, in
this order (R10.6):

1. Stop started services.
2. Detach placed targets.
3. Remove the empty directories and rename the originals back.
4. Restore the prior persisted catalog. If none existed, remove the one this
   run created, after recording its manifest. Restore SHM to its recorded
   presence.
5. Remove the added links and stubs, and restore the preserved service files.
6. Leave `active` as `null` and write readiness `none`.

If restoration itself fails, consumers stay down, the journal stays, and
readiness is `recovery-pending`. Recovery after a crash locates each original
by its recorded type and hash, at the source or at the destination.

## [H] Guard and pinned files (R11)

- **Mounts.** The installed guard has only the four hooks listed in the
  requirements. Mount API calls are not mediated **[inferred; task 0.3]**. No
  guard or trust change is proposed.
- **Btrfs operations** stay in the release cgroup.
- **Pinned edits (O5)** are prepared as a diff and hashes:
  - `op-release`;
  - `op-release-launch`;
  - the rcS hook, only if changed;
  - `manifest.sha256`;
  - `bootstrap-check.sh`, only if an artifact is added.

  The inline readiness check in the stubs adds none. Before
  replacement, back up the 11 pinned files and the manifest, verify the backups
  with `sha256sum --check` against the current manifest, and record the restore
  command.

## [I] D9 — storage locations (prerequisite, proposal)

Every release-tool location below is on the mounted root filesystem
`d53eeac0…`:

| Role | Proposed location | State today |
|---|---|---|
| golden | subvolume `/opt/op-dbus/golden`, nested in `@opt` (subvolume id 271) | empty plain directory, inode 258, owner `jeremy` |
| source snapshots | `/opt/op-dbus/release-send/<name>` (in `@opt`, same filesystem as golden) | plain directory, inode 171017 |
| incoming receives | `release-<uuid>` at the top level of `d53eeac0…`, reached through the detached top-level descriptor ([D]) | 2026-10-18 correction: same parent as both slots |
| slots | `@release-a` and `@release-b` at the top level of `d53eeac0…` | unchanged |

The received root remains read-only throughout installation. Linux VFS requires
write permission on a directory when renaming it across parents, and Btrfs
correctly refuses that permission for a read-only subvolume. Receive beside the
slots and perform a same-parent `RENAME_NOREPLACE`; never make the received
root writable or reconstruct its received UUID. Recovery and abort address
only the journal's exact staged name and verified native identity. Other
top-level roots remain outside cleanup.

### Code change, scoped away from Incus

Add a new constant, `GOLDEN_FS = "d53eeac0…"`. Use it only at the golden and
`release-send` checks (`op-release.rs:226, 267-268, 274-275`).

`HOST_FS` (`:24`), `HOST_TOP` (`:28`), `provision-host-subvolumes`
(`:119-134`) and the launcher's host-top branch (`op-release-launch:38-45`) are
left byte-for-byte unchanged, so the Incus provisioning path does not change
behavior. That path currently targets the absent `083161be…`, which is out of
scope here and is reported to the owner (task 0.5).

### Guarded creation path and ordering

Under `GUARD_GLOBAL`, subvolume creation by ordinary root is refused (see the
requirements' ground truth). So golden is created by `op-release` in the
`opdbus-release` cgroup, which is trusted for `d53eeac0…` (verified in the
loaded map). Creation is folded into the existing `provision-slots` verb. There
is no new command.

1. Owner approves D9. Run `op-release-launch provision-slots`: the bootstrap
   check, the guard-ready check, the transaction lock, and entry into the
   release cgroup.
2. `op-release` opens `/opt/op-dbus` with `O_DIRECTORY|O_NOFOLLOW` and checks:
   - the filesystem is `d53eeac0…`;
   - the subvolume is `@opt` (id 271);
   - the directory is root-owned and not group- or world-writable.
3. If `golden` is a plain directory:
   1. record a preservation receipt (inode, owner, mode, entry count);
   2. refuse unless it is empty;
   3. `rmdir` it (a plain directory, so not guard-hooked:
      `guard.bpf.c:63-70` acts on inode 256 only).
4. `BTRFS_IOC_SUBVOL_CREATE` "golden" on the parent descriptor, which is
   trusted. Verify inode 256, the filesystem UUID and parent id 271.
5. Confirm that `release-send` is a root-owned directory with mode 0700 on
   `d53eeac0…`.
6. Only after that does `./activate-golden.sh` run `build-golden.sh`.
   `build-golden.sh:528-533` is changed to:
   - refuse when golden is not a subvolume root;
   - never create golden itself, because its `btrfs subvolume create` would be
     refused outside the trusted cgroup.

### Consequences

- **Pinned files:** `op-release` and `op-release-launch` change, so
  `manifest.sha256` changes (O5). `bootstrap-check.sh` is unchanged.
- **Guard trust:** unchanged, because cgroup 42 is already trusted for
  `d53eeac0…`. `STORAGE_FSID_A` is not changed.
- **`storage-policy.json`:** entries naming `083161be…` (`golden.source`,
  `golden_transfer.send_parent`, the `logs` member) belong to op-storage. They
  are listed for the owner and not changed. `op-release` reads only
  `receive_executable`, which matches the host.
- **Alternative:** a new filesystem means changing `STORAGE_FSID_A` and guard
  trust. Not recommended.

## Verification

- Phase 0 records provenance and inventory. Every step reads only, except the
  isolated rebuild in task 0.1.
- Phase 1 tests kernel primitives on disposable Btrfs only.
- Phase 2 makes the source changes. The built release tool is then exercised
  only against disposable fixtures.
- Phase 3 runs only behind owner gates.
- Cargo runs through cargo-dispatch (`AGENTS.md:207-212`).
- Formatting, Clippy, the full test suite and `/verify` follow deployment, by
  the owner's downtime agent (`HANDOFF.md:23`). They are not gates here.

## Open decisions

### Owner choices

- **O1 — Target layout.** `owner decision needed`.
  - Approve the targets in [A].
  - Approve the dedicated directories that change absolute paths:
    `/usr/local/libexec/op-dbus/{bin,threetched,sv}`,
    `/usr/local/share/op-dbus/dashboard-release`, and
    `/usr/share/op-dbus/{etc,emqx-etc,plugin-blobs}`.
  - Approve the fixed runit stubs.
  - **Operator commands.** Binaries leave `/usr/local/bin`, which is on
    operators' `PATH`. Choose one:
    1. a static `/etc/profile.d` entry that adds
       `/usr/local/bin`'s replacement directory;
    2. absolute paths only;
    3. per-command symlinks in `/usr/local/bin`, justified as operator
       convenience, not consumer binding.
  - Decide the static files: PAM (row 8), EMQX configuration (row 9) and the
    D-Bus policy (row 19). Each is either installed statically once, or
    included from a release path if task 0.4 shows the include works.
- **O2 — Grants.** `owner decision needed`. Choose G1 (release-owned policy,
  rotates) or G2 (host-writable state, not shipped).
- **O3 — Slot-reuse policy.** `owner decision needed`.
  - When declared host-namespace consumers hold the slot, choose:
    - R (recommended): narrowly scoped re-home restarts, preserving wanted
      state;
    - F: refusal.
  - This choice only determines how an identified pinned inactive slot is
    handled. After any approved re-home restart, the full pre-delete safety
    gate must still pass.
  - There is no approval option to delete a referenced backing tree, retain
    another generation or adopt the rejected release-retirement work. Active
    and previous are the complete retained release set.
- **O4 — `/opt/op-dbus/current`.** `owner decision needed`. Remove it
  (recommended), or keep it as a read-only inspection overlay that no consumer
  may use.
- **O7 — Restart policy.** `owner decision needed`. Approve:
  - changed consumers from the inventory difference with closure, plus the
    transitive auto-start set Q (this quiesces more services than C alone);
  - quiesce before the switch, preserving supervisor wanted state;
  - `sv check` plus probes;
  - automatic revert;
  - consumers left down and reported when recovery itself fails.
- **O8 — Switch mechanism.** `owner decision needed`. Approve (c2) once tasks
  1.3, 1.4, 1.7 and 1.8 pass. There is no technical alternative to choose; this
  is approval of live behavior.

### Live-operation approvals

- **O5 — Pinned-file edit.** `owner decision needed`. Applies to the diff from
  task 2.11. The backups are verified first.
- **O6 — Overlay module.** `owner decision needed`. Covers:
  - the fixture load in Phase 1;
  - the live load and its persistence across boot in Phase 3;
  - or the bind-mount alternative in [B], which needs no module.
- **O9 — First activation.** `owner decision needed`. Write
  `/var/lib/opdbus-release/first-activation.json` listing the approved targets
  and the services to link in `runsvdir/default`.
- **O10 — Golden creation.** `owner decision needed`. Run
  `op-release-launch provision-slots` once, after D9 is approved, so that
  golden is created as a subvolume in the release cgroup ([I]).

### Prerequisite

- **D9 — Storage locations.** `owner decision needed`. Approve proposal [I]:
  - everything on `d53eeac0…`;
  - golden as a nested subvolume in `@opt`, created by `provision-slots` in the
    release cgroup;
  - a new `GOLDEN_FS` constant, with `HOST_FS` and the Incus provisioning path
    unchanged;
  - no guard trust change.

  Every deploy is blocked until this is resolved. Separately, the Incus verb's
  dependence on the absent `083161be…` is reported, not fixed. Its owner
  decides that.

### Technical points resolved in this revision (not owner decisions)

These were resolved in the spec, not left to the owner:
- the durable intent and recovery rules;
- candidate cleanup on a failed top detach;
- the active/previous-only lifecycle and fail-closed inactive-slot replacement
  contract; task 1.6 must still demonstrate the pre-delete safety enforcement;
- the single transaction lock;
- readiness states at boot and during live activation, including the catalog
  match;
- the transitive quiesce set and wanted-state preservation;
- first-activation restoration when `from` is `null`;
- the rollback ordering;
- the catalog contract;
- the inventory fields;
- the stub argument forwarding;
- task ordering.
