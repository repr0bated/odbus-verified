# Requirements — 3tchedFS and threetchedFS

Status: REQUIREMENTS APPROVED FOR DESIGN. This document still authorizes no
implementation, seal, activation, grant change, or service restart.

Spec id: `3tchedfs-threetchedfs`
Continued from Grok session `01a1bcf4-6f0f-7680-b6d0-ca5a76110bf9` (model
`grok-4.7`). Original request: spec `3tched_fs` and `threetched_fs`, include
inode support, and review SHM for overlooked representation.

## 0. Terminology

- **Surface** — one projected system. `3tched` is our code. `threetched` is the
  ZeroClaw-derived binary surface. These are not spellings of one name.
- **Mount** — the FUSE namespace of one surface. `3tched_fs` mounts `3tched`.
  `threetched_fs` mounts `threetched`.
- **Engine** — libfuse under both mounts. It is not a surface and not a third
  filesystem plugin.
- **Flag** — the runtime choice of which surface a mount projects. One engine,
  two mounts, selected by that flag.
- **Inode** — the stable `u64` identity of one path in a mount. Root is `1`.
  Other inodes are derived from surface plus path. They are not a second store.
- **SHM** — live catalog and state already committed under `/dev/shm/opdbus/`.
  The mount reads it. It does not reseal it.

## 1. User stories

1. As an identity, I want the mount named for its surface, so a filesystem call
   is not labeled as chat.
2. As an operator, I want one engine and a surface flag, so `3tched_fs` and
   `threetched_fs` are not two filesystems and libfuse is not a third.
3. As a caller, I want a stable inode per path, so lookup, refresh, and
   collision behave the same on both mounts.
4. As a reader of live state, I want every SHM representation either projected
   as a file or explicitly excluded, so an overlooked blob is not a silent gap.

## 2. Acceptance criteria

### Surfaces and mounts

1. WHEN a mount is started with the `3tched` flag THEN the system SHALL project
   only the `3tched` surface at `3tched_fs`.
2. WHEN a mount is started with the `threetched` flag THEN the system SHALL
   project only the `threetched` surface at `threetched_fs`.
3. IF the flag is missing or names any other surface THEN the system SHALL
   refuse to mount.
4. WHEN either mount is running THEN the system SHALL use the same libfuse
   engine for both.
5. WHEN a path, method file, or plugin id is exposed THEN the system SHALL NOT
   name that transport `chat` or `chat_fs`.
6. WHEN the filesystem plugins are consolidated THEN the system SHALL leave the
   `threetched` binary contract as the binary surface, not as a second
   filesystem.

### Inodes

7. WHEN a path is first projected THEN the system SHALL assign one inode derived
   from surface plus path, with root inode `1`.
8. WHEN the same path is looked up again in the same mount generation THEN the
   system SHALL return the same inode.
9. WHEN two different paths derive the same inode THEN the system SHALL fail
   that projection and SHALL NOT hide one path behind the other.
10. WHEN live state changes but the path does not THEN the system SHALL keep
    the inode and refresh the file body.
11. IF a path is removed from the projection THEN the system SHALL NOT reuse
    its inode for a different path in that mount generation.
12. WHEN an inode is requested THEN the system SHALL resolve it from the mount
    namespace, not from a SHM inode table.

### SHM representations

13. WHEN a view mount reads plugin state THEN the system SHALL read the
    committed object at `/dev/shm/opdbus/state/<plugin>.json` for that plugin
    and pointer.
14. WHEN a workspace mount is frozen THEN the system SHALL read the frozen
    object, not the live SHM value.
15. IF a direct-SHM read asks for a frozen value THEN the system SHALL refuse
    that read rather than return live state as if it were frozen.
16. WHEN a call file is written THEN the system SHALL validate the write against
    the sealed method schema and dispatch it as the mount identity. The call
    body SHALL NOT be stored as SHM state.
17. WHEN the sealed catalog at `/dev/shm/opdbus/plugin-blobs` changes generation
    THEN the system SHALL refresh the projection from that catalog and SHALL
    NOT reseal it from the mount.
18. WHEN a SHM fact has no file THEN the system SHALL record it in the inventory
    below. A fact not projected and not excluded is unfinished.
19. IF sealed schema and SHM state disagree THEN the system SHALL fail the read
    or refresh and SHALL NOT substitute a schema template or empty object.
20. WHEN the requested plugin is absent from the sealed catalog THEN the system
    SHALL omit or refuse that plugin and SHALL NOT invent a schema for it.
21. WHEN the mount identity is unbound THEN the system SHALL refuse a call-file
    write.
22. IF the fact is the inode map, state generation counter, mount identity,
    instance uuid, unavailable-plugin set, or a symlink target THEN the system
    SHALL keep it in the mount namespace and SHALL NOT write it to SHM.
23. IF the fact is a method receipt THEN the system SHALL keep the existing
    operation receipt and SHALL NOT store a second copy as a SHM file.
24. IF the SHM object is `credentials/`, `capability-grants.json`, a
    release-original plugin-blob copy, or `/dev/shm/3tchedfs` THEN the system
    SHALL NOT project it.

## 3. SHM inventory

Reviewed against `crates/_3tched-fs/src/namespace.rs`, `source.rs`, and the
live `/dev/shm/opdbus` directory names. File bodies were not read.

### Projected

| SHM or mount fact | File |
| --- | --- |
| `/dev/shm/opdbus/state/<plugin>.json` | live value / `.value.json` on a view mount |
| `/dev/shm/opdbus/plugin-blobs/<plugin>.<hash>.blob` | `schema.json`, `object.blob.json`, method schema files |
| view manifest | `view.json` |
| workspace locator | `workspace.json` on a frozen mount |
| object hash | `object.hash` |
| required capabilities | `capabilities.json` |
| method input, output, metadata | `operations/<method>/*.json` |
| threetched command schema, input, output, metadata | same shape, only when the flag is `threetched` |
| mount metadata | `.3tchedfs.json` |
| admitted call | `call`, not stored |

### Excluded

| Fact | Why it is not a file |
| --- | --- |
| inode map | derived from surface plus path |
| `/dev/shm/opdbus/state/.manifest.json` | generation counter, not a second state authority |
| `/dev/shm/opdbus/plugin-blobs/.manifest.json` | refresh input; generation is not a new file |
| mount identity, instance uuid, unavailable-plugin set | namespace only |
| symlink target string | namespace only |
| method receipt | existing receipt, no second SHM copy |
| `/dev/shm/opdbus/credentials/` | secret store, never projected |
| `/dev/shm/opdbus/capability-grants.json` | grants stay out of this spec |
| `/dev/shm/opdbus/.op-release-original-*-plugin-blobs` | old catalog copy, not the live catalog |
| `/dev/shm/3tchedfs` | ephemeral, not durability |

### Overlooked, must be closed

The Grok session (turns 37–48) and this audit of `namespace.rs` found the
following representations that either lack a file projection, overlap with
existing projections, or were not explicitly decided during the requirements
phase. Each item is mapped to the acceptance criterion that resolves it.

1. **Invalid live state fallback to schema template.** `requirements.md`
   requirement 19 already forbids substituting a schema template or empty
   object. The design task must remove or gate the fallback.

2. **Direct SHM coverage gap.** Direct SHM (`Backend::DirectShm` in
   `namespace.rs:994–1176`) covers directory, static JSON, object hash, and
   live value. It does NOT cover method files, capabilities, threetched
   command files, call files, or symlinks. Those stay mount-only. Direct SHM
   SHALL refuse them.

3. **Duplicate pointer: `FileSource::Value` vs `NodeKind::LiveFile`.** Both
   variants point at one live value. One path SHALL have one inode and one
   body. The pair must not project two bodies for one pointer.

4. **`FileSource::Workspace` vs frozen workspace read.** `FileSource::Workspace`
   is a projected file. A workspace mount reads frozen objects from the object
   store. Both paths must coexist without overwriting each other — the frozen
   object is not projected as a second body.

5. **`FileSource::View` vs view manifest.** `FileSource::View` projects the
   complete view manifest as a file. `view.json` on a view mount IS that
   projection. No second file is needed.

6. **Mount identity stored in namespace, not SHM.** `MountIdentity` enum
   (`namespace.rs:14–24`) lives in the mount namespace and is NOT written to
   SHM. This is already correct.

7. **Instance uuid stored in namespace, not SHM.** `Namespace::instance_uuid`
   lives in the namespace. No SHM projection is needed.

8. **Unavailable-plugin set stored in namespace, not SHM.** Already in the
   exclusion list.

9. **Catalog generation counter lives in `state/.manifest.json` and is a
   refresh input, not a second state authority.** Already in the exclusion
   list.

10. **Method receipt is already covered** by `requirements.md` requirement 23.
    No second SHM copy.

11. **Credentials/ is already covered** by `requirements.md` requirement 24.
    Secret store, never projected.

12. **Capability grants are already covered** by `requirements.md` requirement 24.

13. **Release-original plugin-blob copies** are already covered.

14. **`/dev/shm/3tchedfs`** ephemeral, not durability — already covered.

15. **Symlink target string** lives in namespace, not SHM — already covered.

Items 6–15 were identified by the audit but are already addressed in existing
exclusion rows. The open items are 1, 2, 3, and 4. All others are confirmed
closed by the existing exclusion list.

## 4. Constraints

- D-Bus remains the control plane. The mount is the projection, not a new API.
- The sealed blob remains the plugin. The mount does not reseal.
- No new SQLite. No grant or identity change is in this spec.
- `3tched` and `threetched` stay distinct names. The flag selects the surface.
- Tests, fmt, and Clippy are out of this phase.

## 5. Decision

Requirement 6 is decided: one filesystem plugin with a surface flag. The
`threetched` binary contract stays its own surface plugin. libfuse stays the
engine. `fuse` is not a third filesystem plugin.
