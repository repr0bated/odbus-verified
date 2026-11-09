# Tasks — 3tchedFS and threetchedFS

Status: SPECIFICATION ONLY. Dependency-ordered. Nothing in this list is
implemented, built, tested, sealed, or deployed by approval of the spec.
Tests, fmt, and Clippy run only after an explicit owner request.
Reseal and `./activate-golden.sh` are not tasks here.

Spec id: `3tchedfs-threetchedfs`
Design: `design.md`. Requirements: `requirements.md` criteria 1–23.

Each task: `implemented: no | built: no | deployed: no | tested: no`

## 1. Surface flag

- [ ] 1.1 Add the surface flag to the mount command
  - Add `--surface` with only `3tched` and `threetched` in
    `crates/_3tched-fs/src/main.rs`.
  - Missing or any other value refuses the process before mount.
  - Derived mount name is `3tched_fs` or `threetched_fs`.
  - Completion: the command rejects `chat` and an empty flag.
  - _Requirements: 1, 2, 3, 5_

- [ ] 1.2 Graft the binary surface only for `threetched`
  - Pass the flag into `Namespace` build in `crates/_3tched-fs/src/namespace.rs`
    and `crates/_3tched-fs/src/fuse_fs.rs`.
  - `threetched` loads `ThreetchedSurface`. `3tched` leaves it unset.
  - Completion: a `3tched` mount has no threetched command files.
  - _Requirements: 1, 2, 4_

## 2. One filesystem plugin

- [ ] 2.1 Keep `_3tched_fs` as the filesystem contract
  - Update `crates/op-plugins/src/state_plugins/_3tched_fs_contract.rs` so the
    contract names both mounts and the surface flag.
  - Do not change the plugin id `_3tched_fs`.
  - Do not edit `threetched.rs` into a filesystem plugin.
  - Completion: contract text matches the flag and both mount names.
  - _Requirements: 4, 5, 6_

- [ ] 2.2 Remove the second filesystem contract from `fuse`
  - Stop `crates/op-plugins/src/state_plugins/fuse.rs` publishing the
    `_3tched_fs` contract as its own plugin schema.
  - Leave libfuse engine code in `crates/_3tched-fs` in place.
  - Completion: one plugin schema owns the filesystem contract.
  - _Requirements: 4, 6_

## 3. Inodes

- [ ] 3.1 Hash surface plus path
  - Change `inode_for_path` in `crates/_3tched-fs/src/namespace.rs` so the
    input is `"{surface}:{path}"`.
  - Keep root inode `1` and the high-bit rule.
  - Do not write the map to `/dev/shm/opdbus/`.
  - Completion: the same path on two surfaces has two inodes; a repeated
    lookup in one mount returns the same inode.
  - _Requirements: 7, 8, 9, 12, 19_

- [ ] 3.2 Refresh without inode reuse
  - `refresh_for_inode` keeps the inode when the path is unchanged.
  - A removed path's inode is not given to a different path in that generation.
  - A derived collision still fails the projection.
  - Completion: body change does not change inode; collision still errors.
  - _Requirements: 9, 10, 11_

## 4. SHM authority

- [ ] 4.1 Keep live and frozen reads apart
  - View mounts read `/dev/shm/opdbus/state/<plugin>.json`.
  - Workspace mounts read the frozen object store.
  - `Backend::DirectShm` still refuses a frozen value.
  - Completion: a direct-SHM frozen read fails and does not return live JSON.
  - _Requirements: 13, 14, 15_

- [ ] 4.2 Keep non-files out of SHM
  - Do not write inode, generation, mount identity, instance uuid,
    unavailable-plugin set, or a second method receipt into SHM.
  - Call-file writes stay dispatch-only.
  - Catalog refresh reads `/dev/shm/opdbus/plugin-blobs` and does not reseal.
  - Completion: those facts remain namespace or receipt state only.
  - _Requirements: 16, 17, 22, 23_

- [ ] 4.3 Close the overlooked SHM cases
  - Remove the schema-template fallback for invalid live state. Fail the read.
  - Direct SHM refuses method files, capabilities, threetched command files,
    call files, and symlinks. Do not add those routes here.
  - One pointer is one path: do not project both `FileSource::Value` and
    `NodeKind::LiveFile` as two bodies.
  - Do not project `credentials/`, `capability-grants.json`,
    `.op-release-original-*-plugin-blobs`, or `/dev/shm/3tchedfs`.
  - Completion: requirements section 3 has no unmarked SHM fact.
  - _Requirements: 18, 19, 24_
## 5. Mount helper

- [ ] 5.1 Pass the flag from the host helper
  - Update `deploy/runit/libexec-3tched/mount-3tchedfs` to pass `--surface`.
  - Check `view.json` for the matching mount name.
  - Do not call `opblob`, `activate-golden.sh`, or `sv`.
  - Completion: helper source matches the flag; installed copy is unchanged
    until a later authorized release.
  - _Requirements: 1, 2, 3, 5_

## 6. Stop line

- [ ] 6.1 Do not seal or activate
  - No catalog reseal, no slot swap, no service restart in this spec.
  - A later owner request is required before build, test, or deploy.
  - Completion: this task stays a gate, not a command.
  - _Requirements: 6, 17_
