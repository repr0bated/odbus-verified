# 3tchedFS mount consolidation — requirements

**Feature Name**: 3tchedFS Mount Consolidation
**Version**: 1.0
**Date**: 2026-11-09
**Author**: Session agent (resuming Grok 4.7 session `01a1bcf4-6f0f-7680-b6d0-ca5a76110bf9`, turns 37–48)

---

## Introduction

### Feature Summary

Consolidate the fragmented 3tchedFS filesystem plugin, fuse schema plugin, and mount helper into a single unified filesystem that serves both the 3tched surface (our code) and the threetched surface (ZeroClaw binary) through one binary with a `--surface` flag.

### Problem Statement

The current codebase has three overlapping pieces that serve the filesystem purpose:

1. **`fuse.rs`** — the libfuse plugin (`PLUGIN_NAME = "fuse"`), a generated schema for the libfuse C library with filesystem projection methods (contract, protocol, invocation convention)
2. **`_3tched_fs.rs`** — the original 3tchedFS plugin (`PLUGIN_NAME = "_3tched_fs"`, inventory registered as `fuse`), commented-out as "replaced by fuse" but still contains contract types, schema generation, and dispatch logic
3. **`mount-3tchedfs`** — a shell helper that reads mount source/target to select `--exclude-plugin` vs `--include-plugin` and then checks view.json for the presence of a plugin ID, but checks for `_3tched_fs` in the source version while the installed version checks `tched_fs`

The session found that `fuse.rs` and `_3tched_fs.rs` share approximately 126 identical non-comment lines for the filesystem contract (ContractDescriptor, FilesystemContract, FilesystemProtocol, Invocation). The `--surface` flag concept was proposed to let one binary mount either the 3tched or threetched surface, but was never implemented or specified.

### Scope

**In scope:**
- One filesystem binary with `--surface` flag selecting between 3tched and threetched surfaces
- One filesystem plugin with consolidated schema
- Mount helper that correctly identifies and validates the mounted surface
- Naming consistency across all components

**Out of scope:**
- The libfuse type generation itself (that remains in `fuse.rs` as a separate concern)
- The threetched plugin itself (that is the ZeroClaw binary-backed plugin; this spec only concerns its mount/fuse projection)
- The existing `--fs-name` behavior (which filters plugins rather than selecting a surface)
- Any changes to the sealed blob catalog format or signing

### Architecture Decisions (from Grok session turns 37–48)

- **Two surfaces**: 3tched (our code) and threetched (ZeroClaw binary), each with one FUSE mount (3tched_fs and threetched_fs)
- **Fuse is NOT a surface** — it is the libfuse engine under both mounts
- **One filesystem binary** with a `--surface` flag: `--surface 3tched_fs` or `--surface threetched_fs`
- **One consolidated plugin** for the filesystem projection (contract, protocol, invocation)
- **Naming**: The consolidated plugin name shall be `threetched_fs` (user specified `threetched_fss` as an intermediate name, corrected to `threetched_fs`)

---

## Requirements

### R1 · One filesystem binary with --surface flag

**User Story:** As an operator mounting a 3tchedFS view, I want one binary to serve both the 3tched and threetched surfaces so that I don't need separate binaries for each mount.

**Acceptance Criteria:**

1. WHEN the filesystem binary receives `--surface 3tched_fs` THEN it SHALL mount the 3tched surface (our code surface)
2. WHEN the filesystem binary receives `--surface threetched_fs` THEN it SHALL mount the threetched surface (ZeroClaw binary surface)
3. WHEN the filesystem binary receives an unrecognized surface value THEN it SHALL refuse to start with a clear error naming the unrecognized value
4. WHEN no `--surface` flag is provided THEN the binary SHALL refuse to start with a usage message explaining the required surface

### R2 · One consolidated filesystem plugin

**User Story:** As a plugin consumer (bridge, mount helper, gRPC server), I want a single filesystem plugin with consistent contract metadata so that I don't need to check multiple plugin names.

**Acceptance Criteria:**

1. IF a filesystem plugin schema is requested THEN it SHALL identify as `threetched_fs` (not `_3tched_fs`, not `fuse`, not `tched_fs`)
2. WHEN the filesystem plugin's contract is read THEN it SHALL produce an immutable `FilesystemContract` describing projection rules for the selected surface
3. WHEN the filesystem plugin's protocol is read THEN it SHALL produce a `FilesystemProtocol` with static contract metadata (no live mount values)
4. WHEN any component checks view.json readiness THEN it SHALL look for the plugin ID `threetched_fs` (not `_3tched_fs`, not `tched_fs`, not `fuse`)
5. IF multiple filesystem plugin files exist (`fuse.rs`, `_3tched_fs.rs`, any other) THEN the consolidated schema SHALL be generated from exactly one source and published under `threetched_fs`

### R3 · Mount helper surface identification

**User Story:** As the mount helper, I want to correctly identify which surface is being mounted so that readiness checks validate the right plugin in view.json.

**Acceptance Criteria:**

1. WHEN the mount helper receives `3tchedFS` as the mount source AND `threetched` as the upstream plugin THEN it SHALL mount with `--surface 3tched_fs` and select all non-threetched plugins
2. WHEN the mount helper receives `threetched` as the mount source THEN it SHALL mount with `--surface threetched_fs` and include only the threetched plugin
3. IF the mount helper checks view.json readiness THEN it SHALL verify that `view.json.objects` contains the key matching the selected surface's plugin ID (`threetched_fs`)
4. WHEN the existing mount differs from the declared surface AND identity THEN the helper SHALL refuse to mount rather than silently continue
5. IF the helper is called with an unrecognized mount source or target THEN it SHALL fail with an error listing the expected mount sources

### R4 · Contract and protocol consolidation

**User Story:** As a developer maintaining the filesystem projection, I want a single copy of the contract types (ContractDescriptor, FilesystemContract, FilesystemProtocol, Invocation) so that changes are applied in one place.

**Acceptance Criteria:**

1. WHEN the filesystem contract types are defined THEN they SHALL exist in exactly one file, referenced by the consolidated `threetched_fs` plugin
2. IF existing contract code in `fuse.rs` and `_3tched_fs.rs` both define `ContractDescriptor` or `FilesystemContract` THEN only the consolidated definition SHALL be compiled into the final binary
3. WHEN `gen_fuse.py` regenerates `fuse.rs` THEN it SHALL not introduce duplicate contract definitions
4. WHEN the sealed plugin schema is generated THEN it SHALL include the contract and protocol methods under the `threetched_fs` plugin name

### R5 · Surface selection scope

**User Story:** As the filesystem, I want to know which surface I'm serving so that I can project the correct catalog of plugins and methods.

**Acceptance Criteria:**

1. WHEN `--surface 3tched_fs` is active THEN the filesystem SHALL expose the 3tched view: all plugins except threetched (equivalent to current `--exclude-plugin threetched` behavior)
2. WHEN `--surface threetched_fs` is active THEN the filesystem SHALL expose only the threetched plugin (equivalent to current `--include-plugin threetched` behavior)
3. WHEN the filesystem starts THEN it SHALL bind the FUSE subtype to the surface name (`3tched_fs` or `threetched_fs`)
4. IF the catalog does not contain the expected plugin for the selected surface THEN the mount SHALL fail with a clear error

### R6 · Naming consistency

**User Story:** As a developer or operator reading logs, source, or configuration, I want the filesystem to consistently use the name `threetched_fs` (or `3tched_fs`) so that I'm not confused by `_3tched_fs`, `tched_fs`, `threetched_fs`, and other variants.

**Acceptance Criteria:**

1. WHEN any source code in this repository references the consolidated filesystem plugin THEN it SHALL use `threetched_fs` (not `tched_fs`, not `threetched_fss`, not `threetched_fss_plugin`)
2. WHEN any source code references the 3tched surface mount THEN it SHALL use `3tched_fs` (not `3tchedFS` as a plugin ID, not `_3tched_fs`)
3. WHEN documentation or comments reference the filesystem THEN they SHALL use the corrected surface names consistently across the same document
4. IF any file outside this repository references the filesystem THEN it SHALL be updated as part of the release activation, not by this spec alone

### R7 · Non-functional requirements

**Acceptance Criteria:**

1. WHEN a mount is active THEN the filesystem SHALL respond to FUSE operations with no more than 50ms overhead compared to the pre-consolidation mount
2. WHEN the binary is rebuilt with a different surface flag THEN it SHALL produce an identical binary except for the surface selection code
3. WHEN any existing fstab or runit configuration refers to the 3tchedFS or threetched mount THEN the new binary SHALL be compatible with the existing mount paths (`/run/mounts/3tchedFS` and `/run/mounts/threetched`)

- [ ] All acceptance criteria in R1–R7 are met
- [ ] The consolidated binary passes FUSE smoke tests for both surfaces
- [ ] The mount helper validates both surfaces correctly
- [ ] No duplicate contract definitions remain in the compiled binary
- [ ] All plugin references use the corrected names
- [ ] Existing fstab and runit configurations work with the new binary

### Acceptance Metrics

- The mount helper readiness check completes in under 3 seconds
- The binary size increases by no more than 5% from the consolidation
- Both surfaces pass the same operational FUSE tests (getattr, readdir, open, read, write)

## R8 · Inode stability

**User Story:** As a FUSE client or kernel cache, I want stable inode numbers for each path across mount restarts so that the kernel's inode cache is not evicted unnecessarily.

**Acceptance Criteria:**

1. WHEN a path exists on either surface THEN the filesystem SHALL assign an inode number derived from a deterministic hash of the surface name and path, not from runtime allocation
2. IF the same path is accessed on a restart of the same surface mount THEN the inode number SHALL be identical to the pre-restart value
3. WHEN two different paths are compared THEN their inode numbers SHALL be different (no collisions)
4. IF the inode assignment function would collide with ROOT_INO (value 1) THEN it SHALL remap to the next available value without changing other inodes
5. WHEN the surface changes (3tched_fs vs threetched_fs) THEN paths with the same name SHALL have different inodes (surface name is part of the hash input)

## R9 · SHM representation coverage

**User Story:** As a filesystem consumer traversing the mount, I want every piece of data that exists in the SHM state tree to be accessible through a file in the mount, and I want any gaps to be explicitly documented rather than silently missing.

**Acceptance Criteria:**

1. WHEN the filesystem mounts as 3tched_fs THEN every FileSource variant that applies to the 3tched surface SHALL be projected as a file at the correct path within the mount
2. WHEN the filesystem mounts as threetched_fs THEN every FileSource variant that applies to the threetched surface SHALL be projected as a file at the correct path within the mount
3. IF a FileSource variant exists in the namespace code but is not projected as a file THEN the requirement SHALL be documented as an explicit exclusion with a reason (e.g., "frozen values unavailable on DirectShm backend")
4. WHEN any SHM-backed file is read THEN the data comes from the authoritative SHM source (ObjectStore at /dev/shm/opdbus/state), not from a cached copy
5. WHEN the sealed blob catalog at /dev/shm/opdbus/plugin-blobs is consulted THEN the filesystem SHALL read the sealed blob directly, never derive content from the on-disk directory listing
6. IF the DirectShm backend cannot provide a representation (e.g., frozen workspace values) THEN the filesystem SHALL return an error (ENODATA or ENOENT) rather than silently omitting the path

### Explicitly known SHM gaps (from Grok session research):

- **Frozen workspace values** — explicitly unavailable on DirectShm backend; must return error rather than hide the path
- **Catalog blobs** — /dev/shm/opdbus/plugin-blobs/ contains sealed blobs but these are not FileSource variants; their schemas are accessed through the sealed object path (e.g., `object.blob.json`)
- **Generation metadata** — `Namespace::generation` and `Namespace::live_generation` are tracked but not represented as standalone FileSource projections
- **Method I/O and metadata** — MethodInput, MethodOutput, and MethodMetadata FileSource variants require the sealed blob's method schema; these are projected in the 3tched namespace but must be validated for both surfaces

### Inode implementation notes (for design phase):

- Current inode assignment (`namespace.rs:1234`) uses blake3 hash of the path, sets the high bit, and avoids ROOT_INO=1
- Inodes are deterministic: hash(path) → fixed u64, not allocated from a pool
- No inode table is stored in SHM; inodes are computed on-the-fly from the path
- The consolidated filesystem SHALL preserve this approach; it SHALL NOT introduce a second inode table in SHM
---

## Constraints and Assumptions

### Technical Constraints

- The existing libfuse type generation (`fuse.rs`) SHALL continue to be generated from libfuse headers by `gen_fuse.py`
- The threetched binary plugin (ZeroClaw) SHALL continue to be a separate plugin; this spec only consolidates the filesystem projection layer
- The sealed blob catalog format SHALL not change; the plugin ID `threetched_fs` replaces the historical `_3tched_fs` only in the new consolidated build
- The mount paths `/run/mounts/3tchedFS` and `/run/mounts/threetched` SHALL remain unchanged

### Assumptions

- The host has `/dev/fuse` available
- The operator UID (`human-lungs-call-owner-uid`) remains the same
- The identity files (`human-lungs-session`, `human-lungs-call-owner-uid`) remain in `/etc/op-dbus/`
- The threetched plugin remains in the sealed catalog with its existing blob

---

## Glossary

| Term | Definition |
|------|------------|
| **Surface** | A named filesystem view: `3tched_fs` (our code) or `threetched_fs` (ZeroClaw binary) |
| **Mount** | A FUSE mount of a surface, bound to a mount point (`/run/mounts/3tchedFS` or `/run/mounts/threetched`) |
| **3tchedFS** | The filesystem project name; not a mount point or plugin ID |
| **threetched** | The ZeroClaw binary surface; also used for the plugin and the mount |
| **fuse** | The libfuse engine, a library used by both surfaces; not a surface itself |
| **`--surface`** | The flag that selects which surface the filesystem binary shall mount |
| **`--fs-name`** | The existing flag that filters plugin selection; separate from `--surface` |
| **view.json** | The JSON file written to the mount root describing which plugins are visible |

---

## Success Criteria

### Definition of Done

- [ ] All acceptance criteria in R1–R9 are met
- [ ] The consolidated binary passes FUSE smoke tests for both surfaces
- [ ] The mount helper validates both surfaces correctly
- [ ] No duplicate contract definitions remain in the compiled binary
- [ ] All plugin references use the corrected names
- [ ] Existing fstab and runit configurations work with the new binary

### Acceptance Metrics

- The mount helper readiness check completes in under 3 seconds
- The binary size increases by no more than 5% from the consolidation
- Both surfaces pass the same operational FUSE tests (getattr, readdir, open, read, write)
