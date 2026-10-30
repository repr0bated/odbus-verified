# Requirements — Release overlays mounted at host target directories

## Why this spec exists

Every release is to be deployed by OverlayFS and rotated each time. The owner's
direction of 2026-10-16 is:

1. Every release is deployed by OverlayFS, rotating each time.
2. Overlays are mounted **directly at the host target directories** that
   consumers already use (for example under `/var/lib/...`). The model is not
   one hub overlay at `/opt/op-dbus/current` with per-file symlinks from host
   paths into it. Whether `/opt/op-dbus/current` survives at all is an open
   decision.
3. Targeted, not blanket. Never overlay `/`, `/usr`, `/usr/bin`, `/etc`,
   `/etc/op-dbus`, `/etc/emqx` or `/etc/runit/sv/<svc>`. Overlay only
   directories that a release wholly owns.
4. No reboot, ever, to activate or roll back a release.
5. The last heavy-development day produced 72 releases, about one every 20
   minutes. Rotation is a frequent live operation.
6. Transport stays Btrfs send/receive of a read-only golden snapshot.
   `./activate-golden.sh` stays the single deploy command and
   `op-release-launch rollback` stays the undo.
7. Rotate the overlays for every release and retain only **active and
   previous** in the two slots. There is no release-retirement lifecycle,
   rename-aside store or additional retained generation. A verified incoming
   receive is temporary transaction staging, not another retained release.

**Rejected implementation input — owner correction, 2026-10-16.** The owner
explicitly rejects the seven-hour Codex release-retirement work. Do not copy,
cherry-pick, rename or refactor that work into this implementation. Historical
code, plans, build receipts and handoffs describing it are not implementation
authority. Establish an independently verified, non-retirement source baseline
before Phase 2 (R13.5). This correction supersedes the retirement proposals in
earlier revisions of this spec.

**Supersession.** This direction supersedes the 2026-10-16T11:17Z
scoped-deployment decision (`HANDOFF.md:596-606`, row `HANDOFF.md:385`). That
decision recommended one atomic selector and manifest-declared per-file
bindings, and advised avoiding OverlayFS (`HANDOFF.md:602`). Two parts of it
still hold:

- its rule against blanket overlays of `/`, `/usr`, `/etc/op-dbus`, runit
  directories and application data (`HANDOFF.md:598`);
- its findings on preparation efficiency and changed-consumer activation
  (`HANDOFF.md:602-604`).

The owner's redirect is recorded at `HANDOFF.md:382`. The per-file symlink
scheme in `docs/src/getting-started/local-release-activation.md:234-255` is not
kept as a shim for any target that moves to a direct overlay (R13.3). The owner
stated the clean-cutover rule in the authoring prompt. I did not find it
written at a file and line in `AGENTS.md`, `HANDOFF.md` or `SIGNALS.md`.

This is a specification only. It changes no code, mounts nothing, loads no
module, edits no guard pin and restarts no service. Per `HANDOFF.md:23`,
formatting, Clippy, full tests and `/verify` run after deployment, by the
owner's downtime agent. They are not a gate in this spec.

## Ground truth captured during authoring (2026-10-16)

Each item is marked **[verified]** (read on the host or in the repository
today), **[read, not executed]** (derived by reading code), or
**[inferred]**. A **[corrected]** item is one where the authoring prompt's
premise differed from what was found.

Repository location. The user rule names `/srv/git/odbus`, but that path does
not exist **[verified]**. This spec lives in `/srv/git/odbus-verified`, which is
the recovered VPS main repository (`HANDOFF.md:56`). The `op-release` source
exists only in `/srv/git/odbus-btrfs-enforcement-20261013`, where git does not
track it (`??`) **[verified]**. Both checkouts are at HEAD `34ff9a5` with dirty
trees **[verified]**.

### `op-release` source and installed binary

Source: `/srv/git/odbus-btrfs-enforcement-20261013/crates/op-storage/src/bin/op-release.rs`,
SHA-256 `a2e7dde2ac948804ee2654d51700931f904ee5466d0d6ee46a9f09c7ca8ffb13`.

- **[verified]** The installed `/usr/local/libexec/opdbus-storage/op-release`
  (SHA-256 `d684f98385086b7319910f06bf3751a6722c4638510ba5d0a5cfc739d7fc85cc`,
  also in `manifest.sha256`) is byte-identical to the kit binary
  `/root/btrfs-enforcement-20261013/deployment-release-slots-20261014/op-release`.
  That kit directory holds an `op-release.rs` with the source's hash. The
  installed binary contains the source's distinctive strings.
- **[inferred]** The binary was built from this source. The evidence above is
  co-location, not build provenance. A rebuild that does not match settles
  nothing unless the original build inputs (toolchain, lockfile, flags, path
  remapping) are known (task 0.1).
- **[verified]** There are two slots, `@release-a` and `@release-b`
  (`op-release.rs:33`). State lives in `/var/lib/opdbus-release/slots.json`
  (`:37-38, 53-60`). `deploy` picks the non-active slot (`:258-262`). It
  replaces the slot only after receive and fingerprint verification
  (`:293-310`). It clears `previous` when that slot is the target (`:317`).
  `release` is deploy then activate (`:405`).
- **[verified]** `overlay()` mounts one read-only overlay at
  `/opt/op-dbus/current`, `lowerdir=<slot>:/var/lib/opdbus-release/base`, with
  `MS_RDONLY|MS_NODEV|MS_NOSUID` (`:374-375`). Before that it detaches every
  existing overlay there (`:373`), leaving a window with no mount.
- **[verified]** Slots are mounted read-only under
  `/run/opdbus-release/slots/<slot>` (`:32, 348-358`). The replace guard reads
  only `/proc/self/mountinfo` (`:178-184, 303-306`).
- **[verified]** Deletion is by subvolume ID, with no wait (`:166-174`).
- **[read, not executed]** Crash between the slot delete (`:307`) and the
  rename (`:309-310`):
  - `deploy` refuses while `pending` is set (`:256`).
  - `abort` deletes everything in `.release-incoming` (`:336`), including the
    verified received subvolume, then clears `pending`.
  - The next `deploy` fails with "slots not provisioned" (`:257`).
  - `provision-slots` (`:218-224`) recreates an empty slot. `slots.json` still
    holds the evicted record, which is updated only at `:315-316`, so a later
    `activate` fails the `received_uuid` check (`:366-367`).

### Launcher, boot hook and guard

- **[verified]** `op-release-launch` (SHA-256 `9b925f8d…`):
  - allows only fixed verbs (`:11-14`);
  - runs `verify_bootstrap` (`:22-23`);
  - runs `boot` outside the release cgroup (`:24-29`);
  - for other verbs, runs `verify_guard_ready` (`:30`), mounts the root
    filesystem's top level at `/run/opdbus-release/top` in the host namespace
    (`:36-37`), and runs `op-release` in `/sys/fs/cgroup/opdbus-release`
    (`:47-48`);
  - after `release`, `activate` or `rollback`, restarts every
    `/opt/op-dbus/current/sv/*` service that is enabled (`:49-58`).
  It does no consumer selection, does no health rollback, and does not unshare
  a mount namespace.
- **[verified]** `/etc/init.d/op-release-activate`:
  - runs `op-release-launch boot` at rcS;
  - has `Required-Start: $local_fs` (`:5`);
  - honors the `opdbus.release=off` escape (`:21`);
  - never blocks boot (`:22`);
  - is linked as `/etc/rcS.d/S12op-release-activate`, after `S10mountall.sh`.
- **[verified]** The 2026-10-14 kernel pin no longer blocks.
  `bootstrap-check.sh:24-28` accepts `6.18.56-x64v3-xanmod1 488c0106…cb2`,
  which matches the live `uname -r` and BTF hash. `sha256sum --check` of the
  11-entry `manifest.sha256` reports all OK (read-only).
- **[verified]** `bootstrap-check.sh:37-51` pins exactly 11 manifest paths.
  Changing a pinned file changes `manifest.sha256`. Adding one changes the
  allowlist and `n!=11`.
- **[verified]** `bootstrap-check.sh:70-72` has `verify_guard_ready` pass the
  release cgroup both `STORAGE_FSID_A` (`083161be…`) and `STORAGE_FSID_B`
  (`d53eeac0…`).
- **[verified]** `guard.bpf.c:56-83` hooks only:
  - `lsm/file_ioctl` and `lsm/file_ioctl_compat`;
  - `lsm/inode_rmdir` and `lsm/inode_rename`, restricted to Btrfs subvolume
    roots (`i_ino == 256`, Btrfs magic `:34`).
  `bpftool prog list` shows exactly these four LSM programs. The kernel-guard
  README says mount and `fsconfig` administration "are not mediated here"
  (`README.md:40-42`).
- **[verified]** The installed `guard.bpf.o` (`e6225be2…`) equals the copy in
  `/root/btrfs-enforcement-20261013/kernel-guard-6.18.56-xanmod/`, whose
  `guard.bpf.c` equals the repository's (`764e0ec1…`).
- **[inferred]** The guard does not mediate `mount(2)`, `fsopen`, `fsmount`,
  `move_mount` or `umount2`. Slot delete and subvolume rename still require the
  trusted release cgroup.
- **[verified, read-only `bpftool map dump`]** The loaded guard is in
  `GUARD_GLOBAL` mode (`settings.mode = 2`; `policy.h` defines 2 as
  `GUARD_GLOBAL`). The trusted map holds:
  - `opdbus-storage` (cgroup 27): `083161be…` and `d53eeac0…`;
  - `opdbus-release` (cgroup 42): `d53eeac0…` and `083161be…`;
  - `opdbus-incus` (cgroup 57): `a1ab7b42…`.
- **[verified]** `BTRFS_IOC_SUBVOL_CREATE`, `SNAP_CREATE` and `SNAP_DESTROY`
  are management ioctls (`allow.generated.h:34-40`). Under `GUARD_GLOBAL`, a
  root process outside a trusted cgroup gets `EPERM` for them on `d53eeac0…`
  (`guard.bpf.c:21-54`). **[read, not executed]** Therefore the
  `btrfs subvolume create` at `build-golden.sh:529`, which runs as ordinary
  root, would be refused.
- **[verified]** At rest, no mount of the top level of `d53eeac0…` exists in
  any mount namespace. `op-release-launch:36` creates one at
  `/run/opdbus-release/top` in the **host** namespace for the duration of a
  run. Through it, every slot is reachable by path, by root.
- **[verified]** `op-release` takes a non-blocking `flock` on
  `/var/lib/opdbus-release/lock` per process (`op-release.rs:109-114`). The
  launcher's `sv` actions (`op-release-launch:49-58`) run after `op-release`
  has exited, without that lock. `/usr/bin/flock` exists.
- **[verified]** The kernel provides `fsmount`, `move_mount`, `open_tree_attr`,
  `listmount` and `statmount` (`/proc/kallsyms`).

### Storage locations (prerequisite D9)

- **[verified]** Filesystem `083161be-7e41-4524-bcdc-c9648093e798` is absent
  from `btrfs filesystem show` and `blkid`. It is named in:
  - `HOST_FS` (`op-release.rs:24`);
  - `STORAGE_FSID_A` (`bootstrap-check.sh:6`);
  - `/etc/op-dbus/storage-policy.json`, at `golden.source`,
    `golden_transfer.send_parent` (`/opt/op-dbus/send`) and the `logs` host
    member (`/var/log`).

  `/var/log` is actually on `d53eeac0…` (findmnt).
- **[verified]** `/opt` is subvolume `@opt` on `d53eeac0…`.
  `/opt/op-dbus/golden` is an empty plain directory (inode 258).
  `/opt/op-dbus/release-send` is a plain directory (inode 171017).
- **[read, not executed]** `deploy` would fail at `op-release.rs:267-268` on
  both the filesystem and the inode check.
  `deploy/lib/golden/build-golden.sh:528-533` treats any existing directory as
  "reusing existing subvolume", so it would fill the plain directory and never
  create a subvolume.
- **[inferred]** `083161be…` is the filesystem that held golden before the
  2026-10-14 storage changes (`HANDOFF.md:33`; backup directory
  `before-lvm-uuids-20261014T233056Z`). This was not checked.
- **[verified]** `storage-policy.json`'s `receive_executable` pin
  (`/usr/bin/btrfs`, `50acb3d9…`, inode 243453) matches the host today.

### Host at authoring

- **[verified]**
  - `/opt/op-dbus/current` is a dangling symlink.
  - No overlay is mounted.
  - `/run/opdbus-release` does not exist.
  - `slots.json` has both slots `null`.
  - `target/release` has no release binaries.
- **[verified]** Host mount propagation is private: there are no `shared:` tags
  on `/`, `/opt`, `/run` or `/var/log`.
- **[verified]** There are 7 non-host mount namespaces. The five sampled belong
  to Incus container `qdrant-recovered`.
- **[verified]** Devuan runit is in use. `/etc/runit/runsvdir/default` links to
  `/usr/share/runit/sv.current/<svc>`. `/etc/sv` exists. `/etc/runit/sv` does
  not exist, although `local-release-activation.md:249` names it.
- **[verified]** Run scripts start their own dependencies. They run
  `sv start <dep>` and then wait for `sv check`:
  - `deploy/runit/emqx/run:5-13`
  - `identity-roots/run:11-13`
  - every `incus-ct-*/run`
  - `boot-net-summary/run:4-6`

### Kernel

- **[verified]** `6.18.56-x64v3-xanmod1`. `/boot/config-6.18.56-x64v3-xanmod1`
  has:
  - `CONFIG_OVERLAY_FS=m`
  - `REDIRECT_DIR=y`
  - `INDEX=y`
  - `XINO_AUTO=y`
  - `METACOPY=y`
  - `REDIRECT_ALWAYS_FOLLOW` not set

  `overlay` is absent from `/proc/filesystems` and from `lsmod`. `overlay.ko`
  is present. The module parameters are unreadable until the module is loaded.
- **[verified]** The kernel overlayfs documentation
  (docs.kernel.org/filesystems/overlayfs.html) says:
  - Changing an underlying filesystem under a mounted overlay is undefined, but
    will not crash or deadlock.
  - Offline lower changes are allowed only if `metacopy`, `index`, `xino` and
    `redirect_dir` were never used.
  - `upperdir` and `workdir` may be omitted, which makes the overlay read-only.
- **[corrected]** `lowerdir+` through `fsconfig` arrived in v6.8.
  File-descriptor layers (`FSCONFIG_SET_FD`) arrived in **v6.13**.
- **[verified]** `move_mount(2)` `MOVE_MOUNT_BENEATH` exists since 6.5
  (man7.org). Its restrictions are:
  - the target is not the filesystem root;
  - the top mount and its parent are in the caller's namespace;
  - the source has no overmounts;
  - the operation is refused when propagation would reach the top or the
    source.

  After a successful move, `from_dirfd` stays associated with the mount.

### Repository facts used by the design

- **[verified]** `deploy/lib/golden/build-golden.sh` reinstalls
  unconditionally:
  - binaries (`:562-564`);
  - `threetched` (`:569-572`);
  - the UI (`:581-586`);
  - the catalog (`:598-608`).
- **[verified]** Golden preparation changes content on every run:
  - `MANIFEST` records `golden-build: $STAMP` (`:513, 811-826`) and appends the
    `release.json` digest (`:839-842`);
  - `release.json` is regenerated with `--namespace release:$COMMIT-$STAMP` and
    `--generated-at` set to the current time (`:833-838`).
- **[verified]** The inventory collector
  (`crates/op-cozo-store/src/bin/inventory/mod.rs:215-268`) records, per entry:
  - `path`, `file_type` (`directory`, `regular` or `symlink`), `size` and
    `mode`;
  - `sha256` for regular files;
  - `target` as the literal symlink destination (`:258-267`).

  It records no owner. It does not hash `MANIFEST` (`excluded_to_avoid_graph_digest_recursion`)
  or sensitive paths (`protected_content_not_read`; `sensitive()` at
  `:136-142`). It skips `usr/share/op-dbus/graphs` (`:217-219`).
- **[verified]** `op_storage::objects::open_beneath`
  (`crates/op-storage/src/objects.rs:24-30`) opens with
  `RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS|RESOLVE_NO_MAGICLINKS`.
- **[verified]** `/etc/opdbus` mixes release policy with host secrets and keys:
  - `secrets/emqx-api-key` (`crates/op-plugins/src/state_plugins/emqx.rs:88-89`);
  - `decoy-signing-key` and `decoy-key-id` (`crates/op-identity/src/bin/op-decoy-issuer.rs:45-46`);
  - `decoy-trust.json` (`crates/op-grpc-bridge/src/oracle_assertion.rs:20`).
- **[verified]** These consumers already take a path setting:
  - `OP_GRANTS_SOURCE` (`op-grants-materializer.rs:142-144`);
  - `OP_MCP_AUDIENCE_POLICY_PATH` and `OP_MCP_TOOLSETS_PATH` (`mcp_policy.rs:112-117`);
  - `OP_WEB_STATIC_DIR` (`deploy/runit/op-web/run:30`);
  - `THREETCHED_BIN` and `THREETCHED_MANIFEST` (`tched-router/run:55-56, 70-71`);
  - `TCHED_ROUTER_BOOTSTRAP_CONFIG` and `TCHED_ROUTER_BOOTSTRAP_SKILLS` (`:51-52`);
  - `OP_MODEL_PROVIDERS_PATH` (`op-grpc-bridge/run:202`).
- **[verified]** These consumers hard-code `/opt/op-dbus/current/...`:
  - `incusd/run:16`
  - `opencode-door/run:7`
  - `paseo-door/run:6`
  - `threetched-fs/run:6-7`
- **[verified]** `tched-router/run:49` sets `PATH=/usr/local/bin:/usr/bin:/bin`.
  `notebook-sources-sync/run:13` derives its directory from `readlink -f "$0"`.
- **[verified]** The catalog has two copies:
  - the persisted, writable catalog `/var/lib/opdbus/plugin-blobs`, written by
    `opblob persist` and read by `build-golden.sh:55` as the source;
  - the release's shipped copy, `var/lib/opdbus/plugin-blobs`
    (`build-golden.sh:598-608`).

  Activation persists the shipped copy, then stages SHM
  (`local-release-activation.md:273-275`).
- **[verified]** `/usr/local/sbin/zeroclaw` (owner-installed) and
  `/var/lib/threetched` (its `config.toml`, `data`, `shared`, `.zeroclaw`,
  toolchains) exist. The release ships a separate pinned `threetched`
  executable (`build-golden.sh:566-572`). `tched-router` keeps its state in
  `/var/lib/tched-router` (`tched-router/run:50`).
- **[verified]** Prior incidents and owner rules:
  - OD-89 (`WISHLIST.md:219`): a cloned mount namespace kept an overlay busy.
  - `SIGNALS.md:510`: the owner rejected copy-into-upper because it hides
    upgrades.
  - `HANDOFF.md:19`: never empty or replace a mounted backing tree.
  - `HANDOFF.md:828`: about 72 releases a day.

## Terminology

- **Slot** — `@release-a` or `@release-b`. A read-only received subvolume.
- **Target** — a host directory that gets its own read-only overlay of
  `<slot>/<subpath>`.
- **Set** — every target of one release.
- **Rotation** — moving every target in the set from slot `from` to slot `to`.
- **Tag** — the overlay's `source` string,
  `opdbus-release:<slot>:<release-name>:<target-id>`. It is visible in
  mountinfo and identifies each mount without a journal.
- **Journal** — the intent record in `slots.json`, written before each step.
  Recovery treats the actual mount table and subvolume state as the truth; the
  journal tells it what was intended.
- **Inventory** — the existing collector output
  (`usr/share/op-dbus/graphs/release.json`), extended per R8.2. It is not a
  second deployment list.
- **Declared consumer** — a runit service listed in the inventory as reading a
  target.
- **Undeclared reader** — any other process.
- **Names kept distinct:**
  - `threetched` — the runtime executable;
  - `threetched-fs` — the FUSE executable;
  - `threetched_fs` — the upstream-backed plugin;
  - `tched_fs` — our protocol plugin.

## Requirements

### R1 — Target inventory and classification

**User story:** As the owner, I want every release destination classified, so
that only directories a release wholly owns get an overlay.

Classes:

- **O** — a wholly release-owned directory. It gets an overlay.
- **F** — a single file or a mixed directory. Not overlaid; reached through a
  dedicated release-owned directory.
- **N** — never touched.

Sources: `build-golden.sh:10-21` and `:536-842`, and
`local-release-activation.md:241-255`. "Host now" was read today.

| # | Golden path | Doc host destination | Host now | Class | Proposed target / how consumers reach it |
|---|---|---|---|---|---|
| 1 | `bin/<name>`, `nlm` | `/usr/local/bin/<name>` (doc:243) | mixed: 9 regular files, 22 symlinks, none from a release | F | `/usr/local/libexec/op-dbus/bin`. Run scripts use absolute paths. Operator access is an owner choice (O1). |
| 2 | `usr/bin/threetched` | `/usr/bin/threetched` (doc:244) | absent | F (file in `/usr/bin`) | `/usr/local/libexec/op-dbus/threetched/`, via `THREETCHED_BIN` |
| 3 | `usr/share/op-dbus/` | same (doc:245-247) | absent | O | overlay `/usr/share/op-dbus` |
| 4 | `libexec/3tched/` | `/usr/local/libexec/3tched/` (doc:248) | absent | O | overlay `/usr/local/libexec/3tched` |
| 5 | `sv/<svc>/{run,check,finish,log/run}` | `/etc/runit/sv/<svc>` (doc:249; absent); `/etc/sv/<svc>` on Devuan | — | F (holds `supervise/`) | overlay `/usr/local/libexec/op-dbus/sv`; fixed stubs in `/etc/sv/<svc>` (R1.6) |
| 6 | `etc/opdbus/{capability-grants,mcp-audience-policy,mcp-toolsets}.json`, `host-runtime-packages.txt`, declarations | `/etc/opdbus` (doc:250) | absent | F: mixed with host keys and secrets (see ground truth) | `/usr/share/op-dbus/etc/opdbus/` (inside row 3), via `OP_GRANTS_SOURCE` and `OP_MCP_*_PATH`. Grants ownership: O2. |
| 7 | `etc/op-dbus/{threetched-runtime.toml,model-providers.toml}`, `threetched-skills/default` | `/etc/op-dbus` (doc:251) | mixed | F | `/usr/share/op-dbus/etc/op-dbus/`, via `TCHED_ROUTER_BOOTSTRAP_*` and `OP_MODEL_PROVIDERS_PATH` |
| 8 | `etc/pam.d/op-dbus-elevate` | `/etc/pam.d/` (doc:252) | not checked | F | O1 |
| 9 | `etc/emqx/{emqx.conf,base.hocon,acl.conf}` | `/etc/emqx` (doc:253) | absent | F (TLS and runtime state, doc:259-261) | `/usr/share/op-dbus/emqx-etc/`; O1 |
| 10 | `usr/local/share/op-dbus/dashboard/{index.html,assets}` | same; "preserve other dashboard mounts" (doc:254) | absent | F (mixed) | overlay `/usr/local/share/op-dbus/dashboard-release`, via `OP_WEB_STATIC_DIR` |
| 11 | `opt/<emqx-install>`, `opt/emqx` link | `/opt/emqx` (doc:255) | `/opt/emqx` links to the dangling `current`; `/opt/emqx-6.2.2` holds `bin`, `data` | O, if task 0.4 confirms no writes under the install tree (`data_dir=/var/lib/emqx`, `deploy/emqx/emqx.conf:7`) | overlay `/opt/emqx`, lower `<slot>/opt/<install>` |
| 12 | `opt/opdbus/{opencode,paseo,nlm-cli-…}` | provider paths (doc:255) | `/opt/opdbus` holds one link into `current` | O | overlay `/opt/opdbus` |
| 13 | `opt/op-dbus/execution-python` | via `current` (`prepare-execution-python.sh:38`) | absent | O | overlay `/opt/op-dbus/execution-python` |
| 14a | `var/lib/opdbus/plugin-blobs` (shipped) | — | — | inside row 3 | staged by golden at `usr/share/op-dbus/plugin-blobs` |
| 14b | `/var/lib/opdbus/plugin-blobs` (persisted) | doc:273-274 | absent | N (writable; owned by `opblob`) | never a target; filled from 14a by `opblob persist` at activation |
| 15 | `var/lib/op-dbus/identity-cozo` | — | — | N (Cozo) | — |
| 16 | `share/op-dbus/warp-ssh`, `libexec/op-warp-ssh` | — | — | N (WARP) | — |
| 17 | `share/op-dbus/notebook-sync/scripts`, `sbin/` | — | — | resolved by task 0.4 | — |
| 18 | `etc/{nftables.conf,iptables/,ssh/,op-dbus/network.conf,op-dbus/openflow-static-flows.json}` | — | — | N (network and SSH) | — |
| 19 | `etc/environment.default`, `etc/pacman-hooks/*`, `etc/dbus-1/system.d/org.dbusmcp.conf` | — | not checked | F (D-Bus policy file); N (pacman hooks on Devuan) | O1 |
| 20 | `MANIFEST` | — | — | inspection only | — |
| 21 | `/usr/local/sbin/zeroclaw`, `/var/lib/threetched` | owner-installed | present | N | never a target, never written (R1.8) |

#### Acceptance criteria

1. WHEN a target set is defined THEN it SHALL contain only class-O directories
   approved by the owner (O1). It SHALL refuse any target that equals, or is an
   ancestor of, any of:
   - `/`, `/usr`, `/usr/bin`;
   - `/etc`, `/etc/op-dbus`, `/etc/opdbus`, `/etc/emqx`;
   - `/etc/sv`, `/etc/runit/sv/<svc>`;
   - `/usr/local/sbin`;
   - `/var/lib/threetched`;
   - `/var/lib/opdbus/plugin-blobs`.
2. WHEN a target is registered THEN it SHALL be:
   - absolute and normalized;
   - neither nested inside, nor containing, another target;
   - backed by a golden subpath that exists as a directory in the slot.
3. WHEN a destination is class F THEN the system SHALL NOT overlay it and SHALL
   NOT bind-mount over the file. The content SHALL be reached through a class-O
   directory.
4. WHEN a class-F consumer already takes a path setting THEN the system SHALL
   use that setting rather than add a symlink.
5. WHEN a symlink is proposed for any destination THEN the design SHALL state
   why neither a setting nor a fixed stub serves, and the owner SHALL approve
   it.
6. WHEN a runit service directory needs release definitions THEN its writable
   directory SHALL hold fixed stubs. A stub never changes between releases, and
   it SHALL:
   - forward every argument (`"$@"`; runit passes `finish` its exit code and
     signal);
   - preserve the service-directory cwd;
   - refuse to start while the boot readiness marker shows incomplete targets
     (R9.8).

   Run scripts that derive their location from `$0`
   (`notebook-sources-sync/run:13`) SHALL be checked against the exec'd path.
7. WHEN a class-N path appears in golden THEN the system SHALL NOT make it a
   target and SHALL leave its host counterpart untouched.
8. WHEN the release ships `threetched` THEN it SHALL ship it only at its
   class-O location. It SHALL NOT ship, overlay or alter `/usr/local/sbin/zeroclaw`
   or anything under `/var/lib/threetched`. Whether `tched-router` should run
   the owner-installed ZeroClaw instead of the release's `threetched` is outside
   this spec.
9. WHEN the inventory is first produced (task 0.4) THEN it SHALL record per
   destination:
   - existence and type;
   - parent mount and filesystem UUID;
   - writers and readers, by file and line;
   - the class, with evidence.

### R2 — Read-only layers, no upper

**User story:** As the owner, I want release content strictly read-only and
always tracking the active release.

#### Acceptance criteria

1. WHEN a target overlay is built THEN it SHALL have no `upperdir` or
   `workdir`, and SHALL be read-only, `nodev` and `nosuid`. It SHALL also be
   `noexec` where the inventory shows no executables under the target.
2. WHEN any target proposes an `upperdir` THEN the proposal SHALL be rejected
   unless it addresses all of the following and the owner approves:
   - copy-up detaching files from later releases;
   - whiteouts and opaque directories hiding new lower content
     (`SIGNALS.md:510`);
   - lower rotation under an existing upper being undefined unless
     `index=off,metacopy=off,redirect_dir=off,xino=off` held from the first
     mount;
   - no shared upper or workdir.
3. WHEN writable state is involved (credentials, grants state, supervision,
   Cozo, Qdrant, EMQX data, identity roots, the persisted catalog) THEN it SHALL
   stay in its existing location, outside every target.
4. WHEN golden is prepared THEN preparation SHALL fail if the tree contains:
   - `trusted.overlay.*` or `user.overlay.*` extended attributes;
   - 0/0 character devices;
   - opaque markers.
5. WHEN an overlay is built THEN it SHALL set `redirect_dir=off`, `index=off`
   and `metacopy=off` explicitly (task 1.2 confirms the set is accepted).

### R3 — Backing trees and constrained path resolution

#### Acceptance criteria

1. WHEN any overlay, attached or lazily detached, references a slot THEN the
   system SHALL NOT modify, rename, replace or delete that slot or
   `/var/lib/opdbus-release/base` (`HANDOFF.md:19`). Slot replacement SHALL
   preserve this rule; there is no release-retirement exception.
2. WHEN an incremental receive uses a slot's source as its parent THEN the
   slot itself SHALL NOT be written (task 1.6 confirms this).
3. WHEN a layer is opened THEN the system SHALL:
   - open `<subpath>` from the verified slot descriptor with
     `op_storage::objects::open_beneath` (`objects.rs:24-30`);
   - check that the descriptor is a directory on the slot's filesystem and
     subvolume;
   - pass that same descriptor to `fsconfig(FSCONFIG_SET_FD, "lowerdir+")`.

   No layer SHALL be re-resolved by path.
4. WHEN a target mount point is used THEN it SHALL be opened without following
   symlinks (`O_PATH|O_NOFOLLOW`, every component checked). Its
   (`st_dev`, mount ID) SHALL be checked against the registered parent mount,
   and it SHALL be passed to `move_mount` by descriptor.
5. WHEN the layer form is chosen THEN it SHALL be a single lower or
   `<subpath>` over `BASE`, according to task 1.2.

### R4 — Switch and consistency contract

**User story:** As a consumer, I want no switch to show me an empty directory,
and a stated contract for what I can see across several targets.

#### Acceptance criteria

1. WHEN a rotation starts THEN every target overlay of the new set SHALL be
   built and verified as a detached mount before any visible change. A failed
   build SHALL change nothing visible.
2. WHEN one target switches THEN a lookup there SHALL resolve to the old or the
   new overlay, never to the covered directory. This is a per-target property.
3. WHEN a set switches THEN the switch SHALL NOT be described or relied on as
   atomic across targets. The guarantee is:
   - declared consumers in the quiesce set Q (R8.3) are stopped before the
     first target moves, and started after the last target moves and the
     catalog is staged;
   - declared consumers outside Q read only content that is identical in both
     sets, as computed from the inventory.
4. WHEN an undeclared reader is active during a switch THEN the only guarantee
   SHALL be per open file: a file opened before its target moved stays the old
   generation; a file opened after it moved is the new one. Undeclared readers
   receive no cross-target guarantee.
5. WHEN targets switch THEN they SHALL move in one fixed recorded order. The
   `active` record SHALL be committed only after every target has moved, the
   catalog has been staged and health has passed.
6. WHEN activation or rollback runs THEN no step SHALL require a reboot.

### R5 — Durable intent, recovery and rollback

**User story:** As the owner, I want every interruption and every failure to
end in the previous complete set, or in a stopped and reported state, never in
a silent mix.

#### Acceptance criteria

1. WHEN any mount, subvolume or host-content step is about to run THEN its
   intent SHALL first be made durable in `slots.json`:
   - write to a temporary file, fsync it, rename it over `slots.json`, fsync
     the directory;
   - include the step name, operands, the expected tags, the subvolume IDs and
     `received_uuid`.

   Completion SHALL be recorded after the step. Recovery SHALL determine what
   happened from the mount table (by tag) and from subvolume state, using the
   journal only to know what was intended.
2. WHEN recovery finds a step's intent without its completion THEN it SHALL
   handle each case as follows:
   - **Build of the new set:** detached mounts do not survive the process, so
     there is nothing to undo.
   - **Beneath insertion, top not yet detached:** two tagged overlays at the
     target; the old one is visible.
     1. Ensure Q is down.
     2. Detach the top, which reveals the new overlay.
     3. Revert this target: build an overlay of `from`, place it beneath, detach
        the top.

     While Q is down, the momentary new view at this target changes content
     only for Q's inputs.
   - **Top detached, completion not recorded:** one overlay at the target,
     tagged `to`. Mark it as moved and continue the revert.
   - **Inactive-slot delete intended:** if the slot still holds the evicted
     `received_uuid`, re-establish the R6 safety gate before any delete, or
     abandon replacement without changing its contents.
   - **Slot name free, received not renamed in:** the slot name is absent, and
     the verified read-only received subvolume is at its journaled
     `release-<uuid>` name beside the slots, with the `received_uuid` and
     fingerprint the journal records. Complete the same-parent no-replace
     rename, then record the new slot. Never clear read-only to cross parents.
   - **Rename done, record not saved:** the slot exists with the pending
     `received_uuid`. Record it as the slot's release.
   - **Received subvolume missing or fingerprint mismatch:** record the slot as
     empty (`null`), so no stale record remains, and report it.
   - **`from` is `null` (first activation):** there is no previous set or
     catalog to revert to. Restore the recorded pre-activation state instead
     (R10.6).
3. WHEN `abort` runs THEN it SHALL NOT delete a verified received subvolume
   whose slot has already been deleted. It SHALL apply R5.2 first.
4. WHEN detaching the old top fails after the candidate was placed beneath it
   THEN the system SHALL:
   - retry the detach while the lock is held;
   - if it still fails, leave the target in its safe state: old overlay
     visible, candidate beneath, tag recorded;
   - mark the target `stacked`, keep `to` pinned, and refuse further rotations
     and reuse of `to` until the stack is resolved;
   - report it.

   It SHALL NOT lazily detach the candidate, because the old top may be mounted
   on the candidate and would be detached with it (task 1.3 measures this).
5. WHEN a forward switch fails at any step THEN the system SHALL:
   1. stop every member of Q that started on the candidate set;
   2. revert the moved targets in reverse order with the same beneath
      mechanism;
   3. restore the `from` release's catalog through its existing owner
      (`opblob persist`, then `stage-shm`), and verify that the live and
      persisted manifests match;
   4. return each member of Q to its recorded prior state (R8.6);
   5. leave `active` and `previous` unchanged.
6. WHEN a revert, the catalog restore, or old-generation health fails THEN the
   system SHALL leave the affected consumers stopped, keep the journal, record
   the exact unresolved state (targets per tag, catalog manifest, services
   down) and report it. It SHALL NOT retry in a loop or start consumers against
   an unverified set.
7. WHEN `op-release-launch rollback` runs THEN it SHALL rotate the full set to
   `previous` with the same intent, quiesce, catalog and health steps as a
   forward release.
8. WHEN a receive is interrupted before any slot step THEN slots SHALL be
   untouched. `abort` SHALL remove only the exact journaled staged receive
   and unreferenced source snapshot through the identity-checked lifetime
   gate; unrelated top-level entries SHALL remain untouched.
9. WHEN `boot` finds a journal THEN it SHALL mount the recorded `active` set
   (the last committed complete set) and leave the journal for the next guarded
   run.
10. WHEN deploy replaces the `previous` slot THEN rollback SHALL be reported as
   unavailable until the new release is activated (`op-release.rs:317`).

### R6 — Two-slot reuse safety

**User story:** As the owner, I want each release to rotate the overlays and
retain only active and previous, without replacing a backing slot that is
still in use.

The candidate replaces the inactive slot only after safe reuse is established.
After successful activation, the candidate is active and the former active
release is previous. A pinned inactive slot blocks replacement; it is not
renamed aside or deleted before its users are gone.

Process and namespace scans identify reasons to refuse reuse, but a clean
sequential scan alone is not proof that no references remain: a descriptor can
move between processes during the scan. Task 1.6 must establish the pre-delete
safety condition before destructive reuse is enabled. A post-delete cleanup
observation cannot authorize an earlier unsafe delete.

#### Acceptance criteria

1. WHEN `op-release` needs the top level of the root filesystem THEN it SHALL
   obtain it as a **detached** mount (`fsopen("btrfs")`, `fsconfig` source
   device and `subvolid=5`, `fsmount`) and use only that descriptor. It SHALL
   NOT be attached at any path. `op-release-launch:36-37` stops mounting
   `/run/opdbus-release/top` (a pinned-file change, O5). Task 1.6 verifies the
   descriptor and mount-namespace behavior before it is relied upon.
2. WHEN the inactive slot is considered for replacement THEN, under the
   transaction lock (R15), the system SHALL verify that it is not active and
   remove its own pathname exposure without changing the backing subvolume:
   - no overlay tagged with the slot is attached in the host namespace;
   - `/run/opdbus-release/slots/<slot>` is unmounted without `MNT_DETACH`
     or `MNT_FORCE`; a busy mount refuses replacement.

   It SHALL inspect mount namespaces for:
   - a Btrfs mount of `/<slot>` or below;
   - an overlay tagged with the slot, or whose options name it;
   - a mount whose device is in the slot's recorded set (R6.6);
   - an attached top-level mount (`subvolid=5`) of `d53eeac0…`.

   Any match or unreadable namespace refuses replacement. Repeated clean
   scans SHALL NOT be described as a synchronization barrier.
3. WHEN mounts are clear THEN the system SHALL also check for process and
   kernel-held references visible through:
   - `fd`, `cwd`, `root`, `exe` and `maps`;
   - io_uring `fdinfo`;
   - loop `backing_file`.

   A hit refuses with a list of holders. Unreadable required information also
   refuses. A clean result is necessary but is not sufficient for R6.10.
4. WHEN declared consumers are checked THEN every consumer that ran from the
   inactive slot SHALL have stopped or restarted since its recorded
   deactivation time, judged from runit's pid and start time. Otherwise
   replacement is refused. This check does not establish that transferred
   references held by other processes have been released.
5. WHEN safe reuse has been established under R6.10 and the incoming receive
   has been verified THEN the system SHALL:
   1. durably record the inactive-slot delete intent (R5.1);
   2. delete that unreferenced inactive subvolume by ID;
   3. confirm cleanup under R6.7;
   4. durably record the install intent, rename the verified incoming
      subvolume into that slot name, and record completion.

   Operations use the detached top-level descriptor. There is no intermediate
   retained generation or rename-aside path. A delete error stops replacement;
   recovery determines the actual subvolume state under R5.2. The active slot
   is never the replacement target.
6. WHEN any overlay is built THEN its device and the slot's subvolume device
   SHALL be recorded and kept until that slot's contents have been safely
   replaced and cleanup has completed.
7. WHEN an unreferenced inactive subvolume has been deleted THEN completion
   SHALL wait for its ID to clear from the deleted-subvolume list (R7.3).
   Pending cleanup SHALL be reported as cleanup, not as proof of an unseen
   process holder. If cleanup cannot be confirmed, stop and retain the
   transaction journal and verified incoming receive for recovery.
8. WHEN a holder is in a foreign mount namespace (Incus guest, sandbox, OD-89
   clone) THEN the system SHALL refuse, and SHALL NOT enter, signal or modify
   that namespace or process.
9. WHEN the only identified holders are declared consumers in the host
   namespace THEN the remedy SHALL be the owner's choice (O3):
   - narrowly scoped re-home restarts onto the active release, preserving
     wanted state (R8.6), followed by re-evaluation of the entire safety gate;
   - or refusal.

   Neither option changes the two-slot retention rule or permits deleting a
   referenced backing tree.
10. WHEN destructive slot reuse is attempted THEN the system SHALL establish,
   before deleting anything, that no attached or detached overlay or other
   holder references the inactive backing tree and that new references cannot
   invalidate that check before deletion. Task 1.6 must verify the enforcement
   used for this condition; negative scans or post-delete observations alone
   SHALL NOT satisfy it. If it cannot be established, refuse replacement and
   leave the active and previous backing contents unchanged. No exception
   authorizes deleting first and discovering a missed holder afterwards.

### R7 — Mount hygiene

#### Acceptance criteria

1. WHEN N ≥ 100 rotations have run THEN, in the host namespace:
   - each target SHALL show exactly one overlay, except a target recorded as
     `stacked` (R5.4);
   - there SHALL be at most two slot mounts;
   - no other mount SHALL reference a slot;
   - the `mountinfo` line count SHALL equal the count after rotation 1.
2. WHEN a rotation completes THEN no buried overlay SHALL remain.
3. WHEN a slot is deleted THEN its subvolume ID SHALL be confirmed to have
   cleared from the deleted-subvolume list after the cleaner runs.
4. WHEN activation commits THEN the retained release set SHALL consist only
   of active and previous: the candidate is active and the former active is
   previous. First activation has no previous release. Temporary incoming
   receive data SHALL NOT become a separately retained generation.

### R8 — Restart policy and service-state preservation

#### Acceptance criteria

1. WHEN a release is activated THEN the system SHALL NOT restart every enabled
   service. This replaces `op-release-launch:49-58`.
2. WHEN golden is prepared THEN the existing collector SHALL add, per entry:
   - `uid` and `gid` (owner);
   - `release_target`, the target the entry belongs to. This is a new field;
     the existing `target` keeps its meaning as the literal symlink
     destination;
   - `consumers[]`.

   Per service it SHALL add:
   - `deps[]`;
   - `starts[]`: the services its run script starts with `sv start`, taken from
     the `wait_dep` calls in its definition;
   - an optional `probe`.

   No second list SHALL be created.
3. WHEN the changed set is derived THEN an entry SHALL count as changed if any
   of the following differ between the active and candidate inventories:
   - presence;
   - `file_type`;
   - `mode`;
   - `uid` or `gid`;
   - `size`;
   - `sha256`;
   - the literal symlink `target`;
   - for directories, presence, mode and owner.

   An entry without a hash (`protected_content_not_read`, the `MANIFEST`
   exclusion) SHALL count as **changed**, never as equal. The collector's
   protected-content exclusions SHALL NOT be removed.

   Then:
   - C = the consumers of changed entries, plus every service whose
     `sv/<svc>/*` changed, plus the closure of their `deps[]`;
   - Q = the least fixed point of: Q₀ = C, then Qₙ₊₁ = Qₙ ∪ {s : s is linked in
     `/etc/runit/runsvdir/default`, and `starts[s]` or `deps[s]` intersects
     Qₙ}. The closure is transitive: if A starts B and B starts a member of C,
     both A and B are in Q. Without that, an auto-start chain could restart a
     member of C mid-switch.
4. WHEN Q is quiesced THEN it SHALL be quiesced with `sudo sv down` before the
   first target moves, in reverse dependency order, and every member SHALL be
   confirmed down. Otherwise the switch is aborted with nothing changed.
5. WHEN Q is started THEN it SHALL be started in dependency order, after the
   catalog is staged (R8.7). Each member SHALL pass `sv check` and its declared
   probe within a bound. Any failure triggers R5.5.
6. WHEN service state is preserved THEN, before quiesce, the system SHALL
   record for each member of Q the supervisor's **wanted** state, not only
   whether a process is running:
   - `supervise/status`: pid, paused flag, want (`u` or `d`), run or finish
     state;
   - the presence of a `down` file;
   - whether it is linked in `/etc/runit/runsvdir/default`.

   On restore:
   - want `u` → `sv up`;
   - want `d` and running (a `sv once` run) → `sv once`;
   - want `d` and not running → leave down.

   The `down` file and links SHALL never be changed. A paused member, or a
   member whose transitive `starts[]`/`deps[]` reach a service with want `d`,
   SHALL abort the plan before anything changes, and SHALL be reported,
   because starting it would change that service's wanted state through
   `wait_dep`.
7. WHEN the shipped catalog changes, or a revert crosses catalogs THEN the
   existing owner SHALL persist the target release's shipped catalog into
   `/var/lib/opdbus/plugin-blobs` and stage SHM. The live and persisted
   manifests SHALL be verified before any member of Q starts.
8. WHEN the active inventory is unreadable (first activation, or a corrupt
   record) THEN every declared consumer SHALL be in C.

### R9 — Boot re-mount and readiness

#### Acceptance criteria

1. WHEN `boot` runs at rcS (`S12`, after `S10mountall.sh`) THEN it SHALL only
   mount the active slot, then each target, and write the readiness marker. It
   SHALL NOT join the release cgroup, delete, receive, restart, run `opblob`,
   or recover a journal. It SHALL take the transaction lock (R15) while it
   runs.
2. WHEN `boot` mounts a target THEN it SHALL first verify that the parent
   mount's filesystem UUID and subvolume equal the registered values.
3. WHEN `boot` orders mounts THEN targets SHALL be mounted after their parent
   filesystems (fstab mounts, including those under `/var/lib`), ordered by
   path depth.
4. WHEN a journal is present THEN `boot` SHALL mount the committed `active`
   set (R5.9) and SHALL mark readiness `recovery-pending`. It SHALL NOT mark it
   `complete`.
5. WHEN `opdbus.release=off` is set THEN `boot` SHALL mount nothing and SHALL
   mark readiness `disabled`.
6. WHEN readiness is computed THEN the marker `/run/opdbus-release/ready` (on
   tmpfs, written atomically) SHALL hold exactly one state:
   - `complete <slot> <release> <catalog-sha256>`, only if:
     - every target of the committed set is mounted with the expected tag;
     - **the persisted catalog's `.manifest.json` SHA-256 equals the committed
       release's shipped catalog digest** recorded in the slot record;
     - no journal is present;
   - `transition <from> <to>`, during a live activation (R9.7);
   - `recovery-pending <reason>`, which covers a journal present, a catalog
     mismatch, missing targets, or an unresolved failure (R5.6);
   - `none`, with no committed release;
   - `disabled`.
7. WHEN a live activation runs THEN the launcher SHALL:
   1. before the first quiesce, write `transition <from> <to>` with one
      `q <svc>` line per member of Q;
   2. add an `allow <svc>` line for a member of Q only when that member is
      about to be started on the complete new set with the staged catalog;
   3. on commit, write `complete` for the new set;
   4. after a successful revert, write `complete` for `from`;
   5. on unresolved failure, write `recovery-pending`.
8. WHEN a release service stub runs THEN it SHALL start the service only if
   one of these holds:
   - the state is `complete`;
   - the state is `transition` and the service is not in Q (its inputs are
     identical in both sets, including the catalog, since catalog readers are
     in C by construction);
   - the state is `transition` and the service appears in an `allow` line.

   Otherwise it SHALL exit nonzero, so runit retries, and base-system services
   are unaffected. A catalog mismatch at boot — committed release A against
   candidate B's persisted catalog after an interrupted activation — therefore
   starts no release consumer.
9. WHEN readiness is `recovery-pending` THEN the next guarded launcher run of
   any verb SHALL first run recovery (R5.2) and restore the committed release's
   catalog (R8.7). There is no automatic recovery at boot: the guard is not yet
   loaded at rcS (`op-release-launch:24-27`), and no new service is added.

### R10 — First-activation preservation

#### Acceptance criteria

1. WHEN a target is first activated and its host path exists THEN, before
   anything is moved, the system SHALL durably write a preservation intent:
   - the source path and type, mode and owner;
   - per-file SHA-256 and symlink targets;
   - the destination on the same subvolume;
   - the restore steps.
2. WHEN preservation runs THEN it SHALL move (never copy) the original to the
   recorded destination, create an empty root-owned directory, fsync, record
   completion, and only then mount.
3. WHEN recovery finds intent without completion THEN it SHALL locate the
   original at the source or the destination by recorded type and hash. It
   SHALL complete or reverse the move, and SHALL never mount over unrecorded
   content.
4. WHEN the host path is a symlink (`/opt/emqx`, `/opt/opdbus/nlm-cli-…`,
   `/opt/op-dbus/current`) THEN the symlink itself SHALL be preserved, with its
   literal target recorded.
5. WHEN first activation is triggered THEN it SHALL run through the normal
   `./activate-golden.sh` → `op-release-launch release` path. It SHALL proceed
   only for targets listed in a root-owned approval record written by the owner
   (`/var/lib/opdbus-release/first-activation.json`, mode 0600). Without that
   record, `release` refuses any target that still needs preservation. No new
   operator command is added.
6. WHEN first activation fails at any step, or its health check fails (`from`
   is `null`), THEN the system SHALL restore the recorded pre-activation state.
   In order:
   1. stop the release services it started;
   2. detach every target it placed (each target then shows the empty
      directory it created);
   3. for each preserved item, remove the empty directory and rename the
      original back;
   4. restore the pre-activation persisted catalog and SHM state from the
      preservation record;
   5. remove stubs and links it added, and restore preserved service files;
   6. leave `active` as `null`;
   7. write readiness `none`.

   If any restoration step fails, the remaining consumers stay down, the
   journal stays, and readiness is `recovery-pending`.
7. WHEN first activation preserves state THEN the preservation intent SHALL
   also record:
   - the prior persisted catalog (`/var/lib/opdbus/plugin-blobs`: absent, or
     moved aside on the same subvolume);
   - the prior SHM catalog presence (`/dev/shm/opdbus/plugin-blobs`);
   - each `/etc/sv/<svc>` file the stubs replace;
   - each `runsvdir/default` link it adds. Which services to link is part of
     the owner's approval record.

### R11 — Guard, cgroup and pinned files

#### Acceptance criteria

1. WHEN this work is implemented THEN it SHALL NOT weaken, disable, bypass or
   re-scope the guard, its trusted map or its cgroup checks. It SHALL NOT
   change guard trust as a side effect.
2. WHEN mount operations run THEN they SHALL run in the host mount namespace
   from the release cgroup, except for `boot`. Slot delete, rename, snapshot
   and receive SHALL stay in the trusted release cgroup.
3. WHEN the claim "the guard does not mediate mounts" is relied on THEN it
   SHALL rest on task 0.3.
4. WHEN pinned artifacts change THEN the change SHALL be prepared as a reviewed
   diff with hashes. Pinned artifacts are:
   - `op-release`, `op-release-launch`, `/etc/init.d/op-release-activate`;
   - `bootstrap-check.sh`, `manifest.sha256`;
   - any new pinned artifact.

   The diff SHALL be applied only after owner approval (O5). Before any pinned
   file is replaced, the current pinned set and manifest SHALL be backed up and
   the backups verified against the current `manifest.sha256`.
5. WHEN the overlay module is needed THEN loading it SHALL be an explicit owner
   approval (O6). No step SHALL load it implicitly, including through module
   autoload on mount.

### R12 — Efficiency at 72 releases a day

#### Acceptance criteria

1. WHEN golden is prepared THEN a file whose content, mode and owner equal the
   staged copy SHALL be left untouched (same inode and extents). This replaces
   the unconditional installs at `build-golden.sh:562-564, 569-572, 581-586,
   598-608`. Obsolete files SHALL still be deleted.
2. WHEN a release is sent THEN it SHALL be incremental from the active
   release's source snapshot when that snapshot exists
   (`op-release.rs:283-292`). The mode and stream bytes SHALL be recorded.
3. WHEN deploy runs THEN it SHALL record wall time, and bytes where available,
   for each stage:
   - snapshot;
   - send/receive;
   - fingerprint;
   - pin scan;
   - slot delete;
   - cleaner completion;
   - overlay build and placement.

   This spec states no values.
4. WHEN two preparations from identical inputs run back to back THEN the second
   incremental stream SHALL contain data writes only for `MANIFEST` and
   `usr/share/op-dbus/graphs/release.json`, both of which embed `$STAMP` and
   the generation time (`build-golden.sh:513, 811-842`), plus the deliberate
   metadata change on `index.html` (`:584-586`). Nothing else.

### R13 — Preserved interfaces and non-goals

#### Acceptance criteria

1. WHEN deploying THEN `./activate-golden.sh` SHALL remain the single deploy
   command and `op-release-launch rollback` the single undo.
2. WHEN implementing THEN the system SHALL NOT revive `--live-only` or
   `--no-restart`, add a second deployment list, or add a parallel schema
   source.
3. WHEN a destination moves to a direct overlay or a class-O location THEN its
   per-file symlink binding SHALL be removed in the same change, with no shim.
4. Non-goals:
   - seed (`SDA_SEED`, SEEDING flag);
   - identity roots;
   - Incus;
   - Cozo, Qdrant and vector data;
   - Snowball;
   - EMQX data;
   - credentials and grants state;
   - runit supervision state;
   - network, OVS, WARP and Xray;
   - storage-guard bootstrap;
   - op-storage hourly sets;
   - owner-installed ZeroClaw.

5. WHEN implementation inputs are selected THEN the seven-hour Codex
   release-retirement work rejected by the owner SHALL NOT be reused,
   cherry-picked, renamed, refactored or used as an implementation baseline.
   Task 0.1 SHALL record the source provenance establishing an independent
   non-retirement baseline. An installed binary, old build receipt or historical
   handoff is not approval to reuse rejected work. Preserve unrelated work;
   this requirement does not authorize deleting or resetting other changes.

### R14 — Prerequisites and provenance

#### Acceptance criteria

1. WHEN Phase 2 begins THEN task 0.1 SHALL have established an eligible source
   baseline under R13.5 and recorded its provenance result: match, mismatch
   with reproducible inputs, or inconclusive. A binary match alone SHALL NOT
   override the owner's exclusion of rejected work.
2. WHEN any deploy is attempted THEN D9 SHALL be resolved:
   - golden is a subvolume on the filesystem `op-release` checks it against;
   - `release-send` is on the same filesystem;
   - the golden subvolume was created by `op-release` in the trusted release
     cgroup, not by `build-golden.sh` (design [I]);
   - the change does not alter `HOST_FS` or the Incus provisioning verb that
     uses it.
3. WHEN an [inferred] claim is acted on THEN it SHALL first be confirmed by its
   named task.

### R15 — Transaction lock

#### Acceptance criteria

1. WHEN any verb other than `status` runs THEN the launcher SHALL acquire one
   exclusive, non-blocking `flock` on `/var/lib/opdbus-release/lock` before
   planning. It SHALL hold it continuously, without release or re-acquire, as
   one transaction across:
   - planning and recording service state;
   - every `sv` action;
   - building and placing mounts;
   - catalog persist and stage;
   - health checks;
   - commit, revert and recovery.
2. WHEN `op-release` is run by the launcher THEN it SHALL receive the lock as
   an inherited descriptor at a fixed number. It SHALL verify that the
   descriptor's device and inode equal those of the lock file and that a
   `flock(LOCK_EX|LOCK_NB)` on that same open file description succeeds. If
   not, it SHALL refuse. It SHALL NOT open a second description of the lock
   file, because that would conflict with the launcher's own lock
   (`op-release.rs:109-114` today).
3. WHEN `boot` runs THEN it SHALL hold the same lock. At rcS no other holder is
   expected. If the lock is busy, `boot` mounts nothing and writes
   `recovery-pending`.
4. WHEN the lock holder dies THEN the kernel releases the `flock`, and the next
   holder SHALL run recovery (R5.2) before anything else.
5. WHEN `status` runs THEN it SHALL read without the lock and SHALL report
   whether a transaction is in progress.

