# Tasks — Release overlays mounted at host target directories

Phases run in order.

- **Phase 0** records provenance and inventory. Everything in it reads only,
  except the isolated rebuild in task 0.1, which writes only its own target
  directory.
- **Phase 1** tests kernel and Btrfs primitives on disposable fixtures. It does
  not need the new implementation.
- **Phase 2** makes the source changes, then exercises the built release tool
  against disposable fixtures. Nothing is installed or run against the live
  host.
- **Phase 3** is install and activation, behind owner gates.

**Owner correction — active and previous only.** Rotate overlays for each
release; do not implement a release-retirement lifecycle or retain another
generation. The owner's rejected seven-hour Codex retirement work is excluded
as implementation input, not merely renamed (R13.5). Historical source paths
and receipts are evidence to inspect, not permission to adopt that work.

**Gate rule.** These actions require recorded owner approval before they run:

- loading a module (including the fixture load in task 1.1);
- any live mount, unmount or `move_mount`;
- snapshot, seal, send or receive on live filesystems;
- editing a pinned file or guard state;
- any `sv` change on a live service;
- activation.

Cargo runs once through the assigned execution worker, never through a new
model dispatcher. Formatting, Clippy, tests and `/verify` are manual/on-request
under the owner's 2026-10-18 rule in `AGENTS.md`; they do not run automatically
before or after deployment. Build and actual-operation safety/readiness receipts
remain distinct from full-suite verification. Use `sudo sv`, never `systemctl`.

---

## Phase 0 — Provenance and inventory

- [ ] 0.1 Establish eligible source provenance; rebuild only an eligible baseline
  - Record the history/session/commit provenance of the source selected for
    implementation. Exclude the owner's rejected seven-hour Codex
    release-retirement work, including copied or renamed derivatives (R13.5).
  - Treat `/srv/git/odbus-btrfs-enforcement-20261013` and source `a2e7dde2…`
    as historical evidence, not an automatically approved baseline. If its
    eligibility cannot be established, do not carry it into Phase 2.
  - Through cargo-dispatch, rebuild only the independently verified eligible
    baseline into a fresh isolated target directory for the provenance check.
  - First record the original build inputs, as far as they are known: the
    toolchain, `Cargo.lock` hash, `RUSTFLAGS`/profile and path remapping from
    the 2026-10-14 kit (`/root/btrfs-enforcement-20261013/deployment-release-slots-20261014/README.md`).
  - Compare the result with the installed `d684f983…`. Report one of:
    - **match**;
    - **mismatch with reproducible inputs**;
    - **inconclusive**: the inputs are unknown, so a mismatch proves nothing
      about the source.
  - Install nothing.
  - _Requirements: R13.5, R14.1, R14.3_

- [ ] 0.2 Read-only inventory of live bindings, mounts and preservation parents
  - For every R1 row, record existence, type, mode, owner, symlink target, and
    the parent mount (UUID and subvolume).
  - Record propagation flags, and every mount namespace (deduplicated) with its
    owner.
  - Confirm that `/var/lib/opdbus-release/base` is empty.
  - Confirm that the preservation parents in design [G] would sit on the
    expected subvolumes.
  - _Requirements: R1.9, R6.2, R9.2, R10.1_

- [ ] 0.3 Read-only guard, trust and module facts
  - Record the build receipt for `guard.bpf.o` (`e6225be2…`).
  - Compare `bpftool prog dump xlated` of the four loaded programs with
    `guard.bpf.c:56-83`.
  - Read the loaded trust map with `bpftool map dump` and confirm whether the
    release cgroup is trusted for `d53eeac0…`.
  - Re-run `sha256sum --check manifest.sha256` and the kernel/BTF pair check.
  - Record `/proc/filesystems`, `lsmod`, and `modinfo overlay` (the parameters
    and defaults from the `.ko`; this does not load it).
  - _Requirements: R11.1, R11.3, R11.5, R14.3_

- [ ] 0.4 Classify the unresolved rows and the include questions
  - Find writers and readers, by file and line, for:
    - the EMQX install tree (row 11);
    - `sbin/` and notebook-sync (row 17);
    - `/etc/pam.d/op-dbus-elevate` (row 8);
    - the D-Bus policy file (row 19).
  - From documentation or source only, check whether Linux-PAM `include` and
    EMQX HOCON `include` accept absolute paths.
  - Find every run script that relies on `$0` or on `PATH` lookup of release
    binaries.
  - _Requirements: R1.6, R1.9_

- [ ] 0.5 Evidence for D9
  - Record:
    - `btrfs filesystem show`;
    - the golden and `release-send` inodes;
    - `op-release.rs:24, 226, 267-268, 274-275`;
    - `build-golden.sh:528-533`;
    - every `storage-policy.json` entry that names `083161be…`.
  - Present proposal [I] to the owner. Change nothing.
  - _Requirements: R14.2_

## Phase 1 — Kernel and Btrfs primitives (disposable fixtures; no new implementation)

All fixtures live in a loop-image Btrfs filesystem under one scratch directory,
which the task creates and removes. Slots, `BASE`, targets and parents live in
that filesystem. A small throwaway harness drives the syscalls directly. These
tasks prove kernel behavior, not `op-release`.

- [ ] 1.1 Owner gate: fixture-only overlay module load (O6)
  - Without approval, run tasks 1.2–1.8 with the bind-mount variant and record
    "overlay not tested".
  - _Requirements: R11.5_

- [ ] 1.2 Layer form and options
  - Try one lower and try `subpath:BASE`, each via `mount(2)` and via
    `fsconfig(SET_FD, "lowerdir+")`, with
    `redirect_dir=off,index=off,metacopy=off`.
  - Record acceptance and the `fs_fd` messages.
  - Show that lower xattrs or 0/0 devices change the merged view.
  - _Requirements: R2.1, R2.4, R2.5, R3.3, R3.5_

- [ ] 1.3 Beneath placement and post-placement topology
  - Place a candidate beneath with `MOVE_MOUNT_BENEATH`, then detach the top.
    Do this under private and under shared fixture parents.
  - Record the mount tree after placement: is the old top re-parented onto the
    candidate?
  - Record what a lazy detach of the candidate would also detach.
  - Record `EINVAL` cases.
  - Test plain first placement.
  - _Requirements: R4.1, R4.2, R5.4, R7.2_

- [ ] 1.4 Counterexamples (a) and (b)
  - (a) Show the gap with a reader.
  - (b) After stacking, try to detach only the buried mount through an `O_PATH`
    descriptor. Record whether the top survives and whether the mount count
    grows.
  - _Requirements: R4.2, R7.2_

- [ ] 1.5 Identity across slots
  - For a file unchanged between two received slots, record `st_dev`, `st_ino`
    and the `maps` device through each overlay, for a process that mmaps and
    executes it.
  - This confirms or refutes the R6 premise.
  - _Requirements: R6.3, R6.6_

- [ ] 1.6 Holder visibility and pre-delete slot-reuse enforcement
  - For each holder, record whether the R6.2 mount scan or the R6.3 process
    scan detects it:
    - an open fd;
    - mmap or exec;
    - a cwd;
    - a lazily detached overlay with a holder;
    - an `unshare -m` clone (OD-89);
    - a clone made while an **attached** top-level mount existed;
    - io_uring registered files;
    - a loop device.
  - Show the descriptor-transfer race: during a sequential process scan, pass
    an fd over `SCM_RIGHTS` from a not-yet-scanned process to an
    already-scanned one, then close the sender's copy. Confirm the scan misses
    it.
  - Confirm that a **detached** `fsmount` of the top level does not appear in
    `/proc/*/mountinfo` and is not copied by `unshare -m` (R6.1).
  - Exercise non-lazy slot unmount with each holder class, including references
    retained through a detached overlay and transferred through `SCM_RIGHTS`.
    Record which kernel checks prevent replacement before any delete.
  - Establish the pre-delete condition required by R6.10; a clean scan or
    successful unmount alone must not be assumed to cover untested aliases.
    Where safe reuse cannot be established, require refusal with both backing
    generations unchanged. Do not substitute a delete-first experiment for
    an enforceable pre-delete gate.
  - After references are released and safe reuse is established, delete only
    the unreferenced fixture slot and confirm cleanup. The deleted list is a
    cleanup check, not proof that an earlier delete was safe.
  - Confirm that an incremental receive never writes into its parent.
  - _Requirements: R3.1, R3.2, R6.1, R6.2, R6.3, R6.7, R6.10, R7.3_

- [ ] 1.7 100-rotation primitive soak
  - Run 100 or more set rotations over at least three fixture targets with two
    slots and real incremental receives, using (c2) primitives.
  - After each rotation, assert the R7.1 counts.
  - After each delete, assert R7.3.
  - After each successful activation, assert that the only retained releases
    are active and previous. Incoming receive data is temporary transaction
    staging; no rename-aside generation or release history is retained.
  - _Requirements: R7.1, R7.2, R7.3, R7.4_

- [ ] 1.8 Straddling-open test at primitive level
  - During 100 switches, readers do the following at two targets:
    - open, read and close in a loop;
    - hold a file open across a switch;
    - read relative to a cwd inside the target.
  - Assert:
    - no `ENOENT` or `ESTALE`;
    - every single read returns whole-old or whole-new content for that file;
    - a held file keeps its generation;
    - files opened after their target moved are the new generation.
  - **Allowed, recorded, not a failure:** an undeclared reader observing old
    generation at one target and new at another within one pass (R4.3, R4.4).
    Record how often this happens. The test asserts mixing only for the
    fixture's declared consumers. Those are quiesced if their inputs differ,
    so they must never see it.
  - _Requirements: R4.2, R4.3, R4.4_

## Phase 2 — Source changes, then fixture verification of the built tool

Source changes come first. Tasks 2.10 and 2.11 build the release binaries
through cargo-dispatch and run them only against Phase 1-style disposable
fixtures.

- [ ] 2.1 Collector and inventory contract
  - Add `uid`, `gid`, `release_target`, `consumers[]`, `deps[]`, `starts[]`
    and `probe`.
  - Keep `target` as the literal symlink destination.
  - Keep `sensitive()`, the `MANIFEST` exclusion and the `graphs` skip.
  - Declare consumers next to `deploy/runit/<svc>/`.
  - Encode the approved targets and the R1.1 refusal list.
  - _Requirements: R1.1, R1.2, R1.7, R1.8, R8.2, R13.2, R13.4_

- [ ] 2.2 Golden preparation
  - Compare before installing.
  - Delete obsolete files only.
  - Stage class-F content into the target subtrees, including the shipped
    catalog at `usr/share/op-dbus/plugin-blobs`.
  - Add the R2.4 check.
  - Make `build-golden.sh:528-533` refuse a golden that is not a subvolume.
  - _Requirements: R1.3, R2.3, R2.4, R12.1, R14.2_

- [ ] 2.3 Layer and target construction
  - Constrained opening (`open_beneath`) from the verified slot descriptor.
  - Target opened with `O_PATH|O_NOFOLLOW` and checked against its registered
    parent.
  - `fsopen`/`fsconfig`/`fsmount` with the tag, FD layers and R2 options.
  - Record each overlay's device.
  - Replace the attached `TOP` with a detached top-level `fsmount`, used only
    by descriptor (R6.1).
  - Add `GOLDEN_FS` for the golden and `release-send` checks
    (`op-release.rs:226, 267-268, 274-275`). Leave `HOST_FS`, `HOST_TOP` and
    `provision-host-subvolumes` unchanged.
  - Fold guarded golden creation into `provision-slots` ([I]).
  - _Requirements: R2.1, R2.2, R2.5, R3.3, R3.4, R3.5, R6.1, R6.6, R14.2_

- [ ] 2.4 Transaction lock, durable intent and reconciliation
  - Launcher: lock fd 9 held across every phase. `op-release` and `boot`:
    verify the inherited lock fd and never reopen it (R15).
  - Write the journal before every step.
  - Reconcile from mount tags and subvolume state.
  - Implement every R5.2 case, including inactive-slot deletion before
    installation of the verified receive, and `from` = `null`.
  - The `abort` fix (R5.3).
  - The `stacked` state (R5.4).
  - _Requirements: R5.1, R5.2, R5.3, R5.4, R5.8, R5.9, R5.10, R15.1, R15.2, R15.3, R15.4, R15.5_

- [ ] 2.5 Switch, revert and rollback
  - Implement the rotation sequence from design [C], including:
    - the reverse revert;
    - the catalog restore with manifest verification;
    - leaving consumers down and reporting when recovery fails;
    - `rollback` on the same path.
  - _Requirements: R4.1, R4.2, R4.3, R4.5, R5.5, R5.6, R5.7, R8.7_

- [ ] 2.6 Two-slot reuse gate
  - Verify the replacement target is inactive, never active.
  - Unmount its slot mount without lazy or forced unmount; refuse if busy.
  - Check mount namespaces and visible holders; fail closed on unreadable
    information, and do not treat negative scans alone as proof.
  - Apply the declared-consumer boundary and the verified pre-delete
    enforcement from task 1.6. If safety cannot be established, refuse before
    changing either backing generation.
  - With the incoming receive verified, journal and delete only the
    unreferenced inactive slot, confirm cleanup, then journal and rename the
    verified receive into that slot.
  - Never rename the old slot aside or retain another generation.
  - Output identified holders and implement only the O3 re-home/refusal choice.
  - _Requirements: R6.1, R6.2, R6.3, R6.4, R6.5, R6.6, R6.7, R6.8, R6.9, R6.10_

- [ ] 2.7 Boot, readiness, first activation
  - `boot`:
    - takes the lock;
    - mounts only;
    - verifies parents;
    - orders by depth.
  - Readiness marker: the `complete`, `transition` (with `q`/`allow` lines),
    `recovery-pending`, `none` and `disabled` states, including the
    persisted-catalog digest match.
  - Stubs gate on the marker.
  - First activation, gated by the approval record:
    - intent covering targets, the prior persisted and SHM catalog, service
      files and links;
    - move, empty directory, completion;
    - full pre-activation restoration on failure.
  - _Requirements: R9.1, R9.2, R9.3, R9.4, R9.5, R9.6, R9.7, R9.8, R9.9, R10.1, R10.2, R10.3, R10.4, R10.5, R10.6, R10.7_

- [ ] 2.8 Consumer cutover (clean, no shim)
  - Add fixed stubs that forward `"$@"` and check readiness.
  - Rewrite the run scripts to use settings and absolute paths, including
    `threetched-fs/run:7` → the persisted catalog.
  - Fix `$0`- and `PATH`-dependent scripts.
  - Remove the symlink rows from `local-release-activation.md:234-255` and
    document the target model.
  - _Requirements: R1.4, R1.5, R1.6, R13.1, R13.3_

- [ ] 2.9 Launcher plan and sequencing (prepared as a diff; pinned)
  - The launcher runs, under one lock: recover, plan, `ready=transition`,
    quiesce Q, switch, catalog, start (admitting each member of Q), commit,
    `ready=complete`.
  - Compute Q as the transitive fixed point over `starts[]` and `deps[]`.
  - Record and restore supervisor wanted state from `supervise/status`.
  - Abort on a paused member, or on a chain that reaches a want-`d` service.
  - Remove the restart of every enabled service.
  - Remove the attached `/run/opdbus-release/top` (`op-release-launch:31-37`).
  - _Requirements: R6.1, R8.1, R8.3, R8.4, R8.5, R8.6, R8.8, R9.7, R15.1_

- [ ] 2.10 Fixture verification of the built tool
  - Build `op-release` and the collector through cargo-dispatch.
  - Re-run the scenarios from tasks 1.6–1.8 against the built binary on
    fixtures:
    - 100 or more rotations with R7.1 counts;
    - pinned-slot refusal for each mount and process holder class;
    - the straddle test, with mixing allowed for undeclared readers.
  - Inject kills at every journal step: mid-receive, before inactive-slot
    deletion, after deletion before rename-in, rename done before record,
    after placement, after top detach, after catalog persist before commit,
    and a failed top detach.
  - Assert that each recovery ends in the previous complete set, or in the
    reported stopped state.
  - Assert that unsafe or uncertain reuse refuses before deletion, leaving
    active and previous backing contents intact, and that successful activation
    retains only the new active and the former active as previous.
  - Assert that boot after a crash following catalog persist writes
    `recovery-pending catalog-mismatch` and that stubs refuse.
  - First-activation failure at every step restores the recorded
    pre-activation state.
  - Two concurrent launcher invocations: the second refuses on the lock.
  - Wanted-state matrix: up, down, once and paused, plus `down` files and an
    auto-start chain.
  - Count re-home restarts. Record the R12.3 stage times as fixture values.
  - _Requirements: R5.2, R5.4, R5.5, R5.6, R6.7, R7.1, R8.6, R9.6, R9.8, R10.6, R12.3, R15.1_

- [ ] 2.11 Preparation idempotence and the pinned-file package
  - Prepare golden twice from identical inputs. Assert that the second
    incremental stream changes only `MANIFEST`, `release.json` and the
    `index.html` metadata (R12.4).
  - Produce the diff and new SHA-256 for every pinned artifact that changes.
    Hand them to the owner for O5.
  - _Requirements: R11.4, R12.1, R12.2, R12.4_

## Phase 3 — Owner-gated install and activation

- [ ] 3.1 Owner gate: record the decisions O1–O10 and D9
  - Execution order: 3.1, 3.3, 3.4, 3.2, 3.5, 3.6, 3.7, 3.8, 3.9. Task 3.2
    needs the new `op-release` from 3.4.
  - _Requirements: R1.1, R11.4, R14.2_

- [ ] 3.2 Owner gate: apply D9 through the guarded path (O10)
  - After the pinned artifacts are installed (3.4), run
    `op-release-launch provision-slots`. Under the bootstrap check, the guard
    check, the lock and the release cgroup, it does the following:
    - verify that `/opt/op-dbus` is on `@opt` (id 271) on `d53eeac0…`;
    - record the empty plain `golden` directory, then `rmdir` it;
    - create the `golden` subvolume;
    - verify inode 256, the filesystem UUID and the parent id;
    - verify `release-send`.
  - Only then may `./activate-golden.sh` run.
  - _Requirements: R10.1, R11.2, R14.2_

- [ ] 3.3 Owner gate: back up and verify recovery artifacts
  - Copy the 11 pinned files and `manifest.sha256` to a root-only backup
    directory.
  - Run `sha256sum --check` on the backups against the current manifest.
  - Record the exact restore commands.
  - _Requirements: R11.4_

- [ ] 3.4 Owner gate: install the pinned artifacts (O5)
  - Install them and the new manifest.
  - Run `verify_bootstrap` and `verify_guard_ready`.
  - On failure, restore from 3.3 and stop.
  - _Requirements: R11.1, R11.2, R11.4_

- [ ] 3.5 Owner gate: load the overlay module live (O6)
  - Record `/proc/filesystems` and the module parameters.
  - Configure persistence across boot only if approved.
  - _Requirements: R11.5_

- [ ] 3.6 Owner gate: first activation through the normal entrypoint (O9)
  - The owner writes `first-activation.json`.
  - Run `sudo ./activate-golden.sh`. It runs preservation intent, moves,
    placement, catalog staging, start in dependency order, and health.
  - Verify:
    - executable digests equal the inventory;
    - the live and persisted catalogs match;
    - authenticated MCP works.
  - _Requirements: R4.6, R8.5, R8.7, R10.1, R10.5, R13.1_

- [ ] 3.7 Owner gate: second release, rollback, roll-forward
  - Record:
    - mount counts;
    - C and Q per release;
    - re-home restarts;
    - the R12.3 values (live);
    - zero reboots.
  - _Requirements: R5.7, R7.1, R12.3, R13.1_

- [ ] 3.8 Owner gate: boot re-mount at the next reboot the owner schedules
  - Never reboot for this task.
  - Confirm that the active set and the `ready` marker exist before `runsvdir`.
  - _Requirements: R9.1, R9.2, R9.3, R9.6_

- [ ] 3.9 Record the outcome
  - Add rows to `HANDOFF.md`, `SIGNALS.md` and `WISHLIST.md`, with evidence
    paths.
  - _Requirements: R14.3_

---

### Out of scope (restated)

- Seed and SEEDING;
- identity roots;
- Incus;
- Cozo, Qdrant and vectors;
- Snowball;
- EMQX data;
- credentials and grants state;
- runit supervision state;
- network, OVS, WARP and Xray;
- storage-guard bootstrap;
- op-storage hourly sets;
- owner-installed ZeroClaw (`/usr/local/sbin/zeroclaw`, `/var/lib/threetched`);
- `--live-only` and `--no-restart`;
- a second deployment list;
- a parallel schema source.
