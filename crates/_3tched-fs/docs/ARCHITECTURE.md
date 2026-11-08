# 3tchedFS architecture

## Data flow

```text
sealed OPBLOB01 catalog          /dev/shm/opdbus/state/<plugin>.json
        │ capture + SHA-256       │ live present-state (written by MutationEngine)
        ▼                          ▼
typed plugin objects              live state values
        │ canonical JSON + BLAKE3  │ validated against sealed schema at capture
        ▼                           │ fallback to schema template if invalid
immutable view manifest ───────────────► read-only FUSE mount (live reads)
        │
        └──► sparse workspace overlay ─► writable FUSE mount (frozen COW)
                         │
                         └── commit ────► new immutable view
```

Plugin execution is deliberately separate:

```text
selected view/workspace schema
        │ validate operation arguments and side-effect policy
        ▼
org.opdbus.v1.PluginV1.Call on the OP-DBUS session bus
```

No filesystem write invokes a plugin operation.

## Source capture

`SourceCatalog` reads the catalog manifest and state generation before and after
a capture. A capture is accepted only when both are identical; otherwise it retries
up to four times.

For every manifest entry it:

1. Derives the expected blob filename from plugin ID and schema-hash prefix.
2. Checks the `OPBLOB01` magic, version, section table, integer ranges, and
   section bounds.
3. Requires exactly one schema section.
4. Recomputes the schema section's SHA-256 digest.
5. Requires the digest to match both the sealed header and full manifest hash.
6. Deserializes the complete sealed `PluginSchema`.
7. Reads the live state file from `/dev/shm/opdbus/state/<plugin>.json`.
8. Validates the state value against the sealed schema; falls back to the
   schema-derived template if the state does not conform.

The sealed blob is the schema authority. The state file is the value authority.
Both are read atomically (catalog manifest + state generation guard against
mid-capture mutations by the MutationEngine).

## Live FUSE reads

For read-only view mounts, scalar leaf files under `data/` use `NodeKind::LiveFile`
instead of pre-baked `NodeKind::File`. On `open()`, the FUSE layer reads
`/dev/shm/opdbus/state/<plugin>.json` once, parses it as JSON, extracts the value
at the node's JSON pointer, and snapshots those bytes into the file handle. The
handle is stable until `release()`; reopening reads fresh state.

This means mutations by the MutationEngine (which atomically writes state files
via temp+rename) are immediately visible through the mount without remounting.
No polling, no inotify, no D-Bus subscription — the read is on-demand at `open()`.

`getattr` without an open handle also reads SHM fresh for size reporting. TTL is
zero and `FOPEN_DIRECT_IO` is set, so the kernel does not cache file data or
attributes across opens.

Only catalog-known plugin IDs are accepted — the namespace is built from the
sealed blob manifest, so arbitrary paths into SHM are impossible.

Workspace mounts do not use live reads; they use frozen COW objects from the
captured view.

## Typed object model

A stored object contains:

- format version
- plugin ID
- source schema hash
- complete `PluginSchema`
- live state value (from `/dev/shm/opdbus/state/<plugin>.json`, or schema
  template fallback if the state file does not validate)

The validator handles strings, integers, floats, booleans, arrays, objects,
enums, unions, and unconstrained JSON. It applies field constraints and rejects
changes to read-only fields, conditional read-only fields, immutable JSON
pointers, or plugins tagged `immutable`.

The current schema adapter may reduce multiple discriminated-union variants to
the same `FieldType` shape. For this reduced representation, union validation
requires at least one matching branch. The original sealed method JSON Schemas
remain available and are used directly for operation argument validation.

## Object store

The default layout is:

```text
/dev/shm/3tchedfs/
├── .lock
├── current
├── objects/<blake3>.blob
├── views/<blake3>.json
├── workspaces/<name>.json
└── snapshots/<name>.ref
```

Objects and view bodies are serialized as canonical JSON and addressed by their
BLAKE3 digest. A view maps every plugin ID to one immutable object hash and
records the source catalog identity and optional parent view.

All store mutations take an exclusive advisory lock. Files are written to a
unique sibling temporary file, synced, renamed atomically, and followed by a
parent-directory sync.

## Copy-on-write protocol

A workspace records:

- immutable base view ID
- changed plugin ID to object-hash mappings
- optional committed view ID

Reading resolves a staged object first and otherwise falls through to the base
view. A write copies one plugin value, replaces a JSON-pointer target, validates
the complete object against its schema and previous value, publishes the new
object, and atomically updates the sparse map.

Commit overlays the sparse map onto the base map and creates a new immutable
view. Activating a commit uses a compare-and-swap-style guard: the workspace
base must still be the current view. `--no-activate` permits an immutable branch
without advancing `current`.

Named snapshots are immutable references to existing view IDs, not data copies.

## Filesystem projection

`Namespace` is built from one pinned view or workspace. It never follows the
mutable `current` pointer after mount construction.

Projection rules:

- object → directory
- array → directory with zero-padded index names
- scalar → JSON file
- object/array container value → `.value.json`
- dependency, operation, and capability relation → symlink

Path components use byte-wise percent encoding. Inodes are deterministic hashes
of absolute projected paths, with collision detection during namespace
construction.

Immutable-view files use mode `0444`. Schema-writable workspace files use
`0644`; all directories use `0555`. Mount options disable device files, setuid,
execution, and access-time updates.

Writable handles buffer a complete replacement. `FUSE_ATOMIC_O_TRUNC` keeps
shell redirection on one handle, and data is parsed as JSON and staged only at
flush, fsync, or release. Invalid or immutable replacements are rejected and do
not publish an object.

Creating, deleting, renaming, linking, chmod, and chown are intentionally
unsupported.

## Operation dispatch

An operation must exist in the selected object's sealed `methods` map. Its
arguments are compiled and validated with the sealed draft 2020-12 JSON Schema.
Non-read side effects require explicit caller confirmation.

Dispatch uses a native `zbus` connection to:

```text
destination: org.opdbus.v1.plugins
path:        /org/opdbus/v1/plugins/<plugin>
interface:   org.opdbus.v1.PluginV1
method:      Call
```

The D-Bus boundary remains responsible for caller identity and authorization.
3tchedFS exposes the method's required capability in metadata and the call
receipt but does not bypass or replace bus policy.

## Trust boundaries and limitations

- `/dev/shm/opdbus/plugin-blobs` is trusted only after structural and hash
  verification (schema authority).
- `/dev/shm/opdbus/state` is read live for view mounts (value authority).
  State files are written atomically by the MutationEngine (temp+rename);
  3tchedFS reads them on-demand at `open()` with no caching.
- Only plugin IDs from the sealed catalog manifest are accepted as paths into
  the state tree — arbitrary SHM paths are impossible.
- `/dev/shm/3tchedfs` is ephemeral by default and is not a durability layer.
- A captured value is the live state from `/dev/shm/opdbus/state/<plugin>.json`,
  validated against the sealed schema. If the state does not conform, the
  schema-derived template is used as fallback.
- Descriptor-set and additional sealed sections remain in the source blob;
  version 1 projects the canonical `PluginSchema`, which already contains the
  fields, methods, relationships, subids, and guarantees used by the namespace.
- `--allow-other` and `--auto-unmount` depend on the host FUSE policy.
