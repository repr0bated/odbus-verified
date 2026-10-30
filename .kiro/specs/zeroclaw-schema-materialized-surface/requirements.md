# Requirements — ZeroClaw schema-materialized surface

## Why this spec exists

The repository carries a hand-rolled Rust surface that re-states the ZeroClaw
binary: a `tched_router` plugin, an embedded 26k-line config contract, a routing
runtime, legacy dispatch, routes, and tests. Every ZeroClaw release forces a
human to re-edit that mirror so it matches the new binary. That copy is why a
ZeroClaw upgrade is a large, manual event.

This spec removes the reason for the copy. The surface becomes the binary's own
output, redistributed into locations 3tched already has:

1. The schema comes from the installed `/usr/bin/zeroclaw` binary.
2. 3tchedFS is extended so a `zeroclaw` directory **is** the tree that appears
   from `zeroclaw config schema` and `zeroclaw --help`. The directory and the
   sealed blob hold the binary's schema. A `call` there execs the installed
   binary.
3. That tree — the schema in 3tchedFS and the blob — is the replacement for the
   `tched_router` plugin. There is no regenerated Rust plugin.
4. Only after that tree works on the running host is the hand-rolled copy mapped
   and removed.

A ZeroClaw upgrade then becomes: replace the binary, regenerate the tree from
it, reseal. 3tched is what becomes extendible; a new binary widens 3tched.
Nobody rewrites a method table, and nothing is compiled from the schema into a
plugin.

This is a new spec. It is not an extension of any OpenWrt gateway-wiring spec,
and it does not reuse that spec's structure or scope.

## Ground truth captured during authoring (2026-10-08)

These are facts read off the running host, not assumptions:

- `/usr/bin/zeroclaw` → `/opt/op-dbus/current/usr/bin/zeroclaw`, reports
  `zeroclaw 0.8.4`. `/opt/op-dbus/golden/usr/bin/zeroclaw` also reports
  `zeroclaw 0.8.4` (same release).
- `zeroclaw config schema` emits a single JSON Schema document:
  `$schema`/`title`/`description`/`type`/`properties` plus a `$defs` map of
  **346** definitions; **84** top-level config sections under `properties`
  (`a2a`, `acp`, `agents`, `gateway`, `providers`, `security`, `skills`, …).
  Size ≈ 1.345 MB, ≈ 26,032 lines.
- `zeroclaw --help` lists the binary's own commands, including hyphenated
  `self-test`, plus `config`, `agent`, `gateway`, `acp`, `daemon`, `service`,
  `doctor`, `status`, `security`, `estop`, `cron`, `models`, `providers`,
  `channel`, `agents`, `channels`, `integrations`, `skills`, `browse`, `sop`,
  `migrate`, `auth`, `hardware`, `peripheral`, `memory`, `update`, `eval`,
  `completions`, `desktop`, `locales`, `quickstart`.
- Schema property keys are snake_case or a single word (`public_base_url`,
  `max_sessions`, `read_memory_from`). CLI command words are the binary's own
  (`self-test`, not `self_test`; `config`, not `config_get`).
- The running host mount is `/run/mount/3tchedFS` (format `3tchedfs/1`, catalog
  generation 1908, hash `9694872…`, 69 objects, bound to host session
  `414fbc56-…`). The running FUSE binary is
  `/opt/op-dbus/current/bin/threetched-fs`.
- In that mount, each object exposes `operations/<name>/` with four entries:
  `call`, `input.schema.json`, `output.schema.json`, `metadata.json`. The call
  file is the write surface (`call`); reads of it are capability-gated (observed
  `Permission denied` as the host operator for a `read` operation).
- `tched_router` is already a live object in that namespace
  (`/plugins/tched_router`), i.e. the hand-rolled plugin is sealed into the
  current catalog.
- The FUSE namespace builder (`crates/threetched-fs/src/namespace.rs`) already
  constructs the `operations/<name>/{call,input.schema.json,
  output.schema.json,metadata.json}` tree and the symlink mirror under
  `/operations/<name>/`. This spec feeds that layout from the binary's schema;
  it does not redesign the inode or call-file model.

## Terminology

- **Binary** — the installed `/usr/bin/zeroclaw` 0.8.4 release.
- **Schema** — the JSON output of `zeroclaw config schema` plus the command
  surface of `zeroclaw --help` (and relevant `<command> --help`).
- **Materialized form** — the schema redistributed into 3tchedFS: the `zeroclaw`
  directory tree that appears from the binary's output, plus the sealed blob
  that holds the same schema. It is **not** a Rust `PluginSchema`/`MethodDecl`
  artifact. It is the replacement for the `tched_router` plugin.
- **3tchedFS `zeroclaw` directory** — a `zeroclaw` entry in the existing
  3tchedFS namespace whose files are the binary's schema and whose `call` execs
  the installed binary.
- **Scaffolding** — the thin code that moves the binary's schema into the
  `zeroclaw` directory and the blob. It redistributes schema; it does not
  restate commands as a method table.
- **The copy / the mirror** — the hand-rolled Rust surface that repeats the
  binary (the `tched_router` plugin and its satellites).

## Requirements

### R1 — Schema is read from the installed binary, never hand-frozen or recompiled

**User story:** As the maintainer, I want the surface to come from the binary
that is actually installed, so that the surface and the binary can never drift
and nothing re-enters the compile.

#### Acceptance criteria
1. WHEN the surface is materialized THEN the system SHALL obtain it by invoking
   the installed `/usr/bin/zeroclaw` (`config schema` and `--help`) at
   materialization time as a runtime input, not from a checked-in frozen schema
   file. A stored copy of `zeroclaw config schema` in the repository is itself
   the embedded contract and SHALL NOT be introduced; the only schema that
   persists is the one inside the sealed blob.
2. The materialized form SHALL record the binary version string
   (`zeroclaw 0.8.4`) and the schema's identifying content so a later run can
   detect that the binary changed.
3. Property keys SHALL be preserved exactly as the binary prints them
   (snake_case or single word). Command names SHALL be preserved exactly,
   including hyphenated forms such as `self-test`. No renamed names such as
   `config_get`/`config_set` SHALL be introduced.
4. The spec SHALL NOT introduce a hand-authored or hand-maintained parallel
   schema, and SHALL NOT compile the schema into a generated Rust
   `PluginSchema`/`MethodDecl` artifact. A frozen hand-copied schema and a
   regenerated plugin are both out of scope.
5. A ZeroClaw upgrade SHALL be: replace the binary, regenerate the `zeroclaw`
   tree and blob from it, reseal — with no hand-editing of a method table and no
   recompiled plugin.

### R2 — 3tchedFS is extended so the `zeroclaw` directory IS the schema tree

**User story:** As a caller inside an identity's 3tchedFS namespace, I want a
`zeroclaw` surface whose files and operations are the binary's own schema, so
that I discover arguments, results, and errors from the namespace rather than
inventing a client.

#### Acceptance criteria
1. The extension SHALL be a `zeroclaw` directory inside the existing 3tchedFS
   namespace, populated from `zeroclaw config schema` and `zeroclaw --help`,
   following the existing object layout
   (`operations/<name>/{call,input.schema.json,output.schema.json,
   metadata.json}` plus the existing `/operations/zeroclaw/` symlink mirror).
2. Each operation name SHALL be the binary's own command word, preserved
   verbatim (e.g. `status`, `self-test`, `config`). Each operation's
   `input.schema.json`, `output.schema.json`, and `metadata.json` SHALL be the
   binary's schema for that command, not a hand-written method declaration.
3. A write to an operation's `call` file SHALL exec the installed binary
   `/usr/bin/zeroclaw <command> <args>`, and SHALL return the binary's result
   through the operation receipt and resulting state. The call SHALL NOT be
   routed back through plugin dispatch, and no adapter SHALL restate the command
   as a `MethodDecl`.
4. The extension SHALL leave the existing inode allocation and call-file layout
   alone. Codex owns that layer; this spec feeds it the binary's schema. It
   SHALL NOT redesign inode allocation or the call-file model.
5. Identity and capability enforcement SHALL be preserved on every `zeroclaw`
   operation exactly as for existing objects (local filesystem access does not
   remove the permission boundary; the `call` write is the caller's canonical,
   capability-gated call).
6. The system SHALL NOT infer a method taxonomy. It SHALL NOT assign
   `side_effect`, `required_capability`, or `subid` from command groups.
   Whatever those fields carry SHALL come from the binary's schema itself, not
   from an invented mapping.

### R3 — The FUSE tree is built from the schema before the seal

**User story:** As the maintainer, I want the schema to materialize in the FUSE
tree first and the seal to carry that tree, so the mount is not a projection of
a Rust plugin.

#### Acceptance criteria
1. The `zeroclaw` directory SHALL be built from the binary's schema, and the
   form that materializes in that tree SHALL be what is passed to the blob
   sealer.
2. The seal SHALL NOT seal a Rust plugin first and let the mount appear as a
   projection of it. The order SHALL be: schema → `zeroclaw` tree → seal that
   tree's schema into the blob.
3. The sealed blob SHALL hold the binary's schema and command surface, so the
   blob is the release's own schema and commands.
4. The seal path SHALL remain the existing `op-blob` sealer and SHM blob catalog
   (`/dev/shm/opdbus/plugin-blobs/`); this spec adds no new sealer and no
   parallel schema directory.

### R4 — First proof is on the running host

**User story:** As the maintainer, I want the replacement proven on the host
that is already running before any code is removed, so the removal rests on a
working tree and not on a plan.

#### Acceptance criteria
1. The first proof SHALL be performed against the running host mount
   `/run/mount/3tchedFS` using the running FUSE binary
   `/opt/op-dbus/current/bin/threetched-fs`.
2. The proof SHALL show the `zeroclaw` operations present in the host mount with
   schemas and metadata that are the installed binary's own output.
3. The proof SHALL show at least one `call` through the host mount that execs
   the installed binary and returns through the operation receipt and resulting
   state, with identity/capability enforcement intact.
4. A container SHALL NOT be required for this proof.
5. The host proof SHALL be recorded (what was invoked, what was observed) before
   any removal step begins.

### R5 — The hand-rolled copy is surveyed, then removed against the tree

**User story:** As the maintainer, I want every hand-rolled piece that only
exists to mimic the binary identified and removed, replaced by the `zeroclaw`
tree, so the upgrade cost collapses to regenerate-and-reseal.

#### Acceptance criteria
1. The hand-rolled surface that repeats the binary SHALL come out, not be folded
   into a generator. The `tched_router` plugin, `tched_router_contract.rs`, the
   runtime, the routes, and the mimic tests SHALL be removal rows, not
   fold-into-generator rows.
2. The blast survey of that surface (the design's candidate table) SHALL remain
   provisional until the FUSE `zeroclaw` tree exists; it SHALL NOT decide the
   replacement. AFTER the tree exists, each surveyed row SHALL be marked **stay**
   or **remove** against that tree.
3. A row SHALL be marked **stay** only where it is identity or capability
   enforcement with no ZeroClaw behavior in it, or a layer genuinely separate
   from the binary mirror (the FUSE inode/call-file layer, the blob sealer,
   unrelated plugins). Every surface carrying ZeroClaw behavior SHALL be
   **remove**.
4. The method table, routes, runtime, and mimic tests SHALL be removed only
   after the host proof, and the `tched_router` plugin SHALL be removed with
   them, replaced by the `zeroclaw` tree and blob.
5. The spec SHALL only survey the removal. The removal itself SHALL wait on the
   host proof and SHALL NOT be performed inside this spec document.

### R6 — Order of work is fixed

**User story:** As the maintainer, I want the replacement to exist and be proven
before anything is removed, and the container to come last.

#### Acceptance criteria
1. The order SHALL be:
   1. Schema from the installed binary → `zeroclaw` tree built from that schema
      in 3tchedFS → prove on the running host.
   2. The form that materializes in that tree is what is passed to the blob
      seal.
   3. Remove the surveyed hand-rolled copy, including the plugin, against the
      tree.
   4. Container, guest systemd unit, and guest socket come after the host FUSE
      call path works. In this revision they are a sequence note only, not
      tasks.
2. No removal step SHALL begin before the host proof is recorded.
3. The container, guest systemd unit, and guest socket SHALL NOT be a
   precondition for steps 1–3.

## Out of scope

- Reimplementing ZeroClaw, or starting from the existing Rust.
- A regenerated Rust `PluginSchema`/`MethodDecl` artifact built from the schema.
- Inferring a method taxonomy (`side_effect`/`required_capability`/`subid` from
  command groups).
- Reusing `gen_tched_router_config_surface --import-json` or any entry that
  freezes a captured schema into an embedded contract.
- Inventing or renaming command names (`config_get`/`config_set` and the like).
- A frozen, hand-copied schema.
- Starting from the container; guest unit/socket as tasks in this revision.
- Rewriting or redesigning the FUSE inode/call-file layer.
- Extending the OpenWrt gateway-wiring spec.
- The btrfs seed, overlay, and send/receive work.
- Performing the deletion within this spec. The spec surveys the removal; the
  removal waits on the host proof.
