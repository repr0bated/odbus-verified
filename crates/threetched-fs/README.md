# 3tchedFS

3tchedFS is a standalone, schema-driven FUSE projection for sealed OP-DBUS
plugin blobs. It captures a coherent plugin catalog into immutable,
content-addressed views and provides sparse copy-on-write workspaces for typed
edits.

## Source invariant

Two SHM trees are read:

```text
/dev/shm/opdbus/plugin-blobs/.manifest.json       ← catalog manifest (schema hashes)
/dev/shm/opdbus/plugin-blobs/<plugin>.<hash>.blob ← sealed OPBLOB01 (schema contract)
/dev/shm/opdbus/state/<plugin>.json              ← live present-state values
/dev/shm/opdbus/state/.manifest.json             ← state generation counter
```

The sealed blob is the schema authority (types, methods, constraints, immutability).
The state file is the value authority (live runtime data written by MutationEngine).
At capture time, each state file is validated against its sealed schema; if it
does not conform (e.g. `net.json` is `[]` where the schema expects an object),
the schema-derived template is used as fallback.

For read-only view mounts, scalar leaf files under `data/` are served live from
`/dev/shm/opdbus/state/<plugin>.json` — each `open()` reads the SHM state file
once and snapshots the bytes for that file handle. Reopening gets fresh state.
No polling, no watching, no caching. The schema (types, methods, immutability)
remains pinned from the sealed blob at capture time.

Workspace mounts use frozen COW objects from the captured view and do not read
live state.

## Guarantees

- Deterministic object and view identities using BLAKE3 over canonical JSON.
- Immutable views pinned by content hash.
- Sparse COW workspaces that store only changed plugin objects.
- Schema validation before every staged write.
- Enforcement of read-only fields, immutable paths, and immutable plugins.
- Stable path-derived FUSE inode numbers.
- Atomic store publication with locking, `fsync`, and rename.
- Plugin operations dispatched only through `org.opdbus.v1.PluginV1.Call`.
- Explicit confirmation required for non-read operations.

The default store is `/dev/shm/3tchedfs`, so it is intentionally ephemeral
across reboot. Pass `--store <path>` to use another location.

## Build

Requirements:

- Linux with FUSE enabled and access to `/dev/fuse`
- `fusermount3` for explicit unmounts
- A current stable Rust toolchain

```sh
cd /srv/3tchedFS
cargo build --release
```

The executable is `target/release/threetched-fs`.

### Generated typed consumer

The `typed_use_generated` example is opt-in because it imports
`generated/plugin_types.rs`, a local artifact derived from a selected sealed
catalog. Ordinary example builds do not require that artifact. From the
workspace root, prepare it with the existing generator before enabling
`generated-example`:

```sh
cargo run --target x86_64-unknown-linux-gnu -p threetched-fs --example typed_specialize -- --catalog /path/to/sealed/catalog --use-example crates/threetched-fs/examples/typed_use_generated.rs
cargo build --target x86_64-unknown-linux-gnu -p threetched-fs --features generated-example --example typed_use_generated
```

The generator selects the example's methods and types from that catalog; it
does not seal a new schema. Enabling this feature (including through
`--all-features`) requires generation first. Keep generated output local to
its checkout; regeneration binds it to the selected catalog and source tree.

## Quick start

Capture and activate a coherent view:

```sh
./target/release/threetched-fs capture
./target/release/threetched-fs verify
./target/release/threetched-fs views
```

Inspect the projected tree without mounting:

```sh
./target/release/threetched-fs tree
./target/release/threetched-fs inspect --plugin adc
```

Create and mount a writable workspace:

```sh
./target/release/threetched-fs workspace new --name experiment
mkdir -p /tmp/3tchedfs
./target/release/threetched-fs mount /tmp/3tchedfs --workspace experiment
```

Scalar files contain JSON values. Strings therefore include JSON quotes:

```sh
printf 'true\n' > /tmp/3tchedfs/plugins/adc/data/configured
cat /tmp/3tchedfs/plugins/adc/data/configured
./target/release/threetched-fs verify --workspace experiment
```

Unmount, then either commit or abort:

```sh
fusermount3 -u /tmp/3tchedfs
./target/release/threetched-fs workspace commit experiment
# or, before commit:
./target/release/threetched-fs workspace abort experiment
```

An immutable view can be mounted with `--view <view-id>`, or with no target to
use the current view. View mounts read live state from
`/dev/shm/opdbus/state/` on every `open()` — mutations by the MutationEngine are
immediately visible without remounting. The `--state` flag overrides the state
directory (default `/dev/shm/opdbus/state`).

`--auto-unmount` is opt-in because libfuse may require
`user_allow_other` in `/etc/fuse.conf`; explicit `fusermount3 -u` works without
that host setting.

## Direct workspace staging

FUSE is optional. A JSON pointer can be staged directly:

```sh
./target/release/threetched-fs workspace stage experiment adc \
  --pointer /configured --json true
```

Use `--file <path>` for a complete JSON value. Object and array containers are
also writable through their projected `.value.json` file.

## Operations

Operation inputs are validated against the JSON Schema sealed into the selected
view or workspace before D-Bus dispatch:

```sh
./target/release/threetched-fs call adc get_status --args '{}'
```

Mutation and external operations additionally require
`--confirm-side-effects`. The default bus address is
`unix:path=/run/opdbus/session-bus.sock`.

## Namespace

```text
/
├── .3tchedfs.json
├── view.json
├── workspace.json                         # workspace mounts only
├── plugins/<plugin>/
│   ├── object.blob.json
│   ├── object.hash
│   ├── schema.json
│   ├── capabilities.json
│   ├── data/...
│   └── operations/<method>/
├── operations/<plugin>/<method>           # symlinks
├── relationships/dependencies/<plugin>/   # symlinks
└── capabilities/<capability>/...           # symlinks
```

Objects become directories, arrays use zero-padded numeric entries, and
scalars become JSON files. Path components are percent-encoded
deterministically.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the data model, COW
protocol, and trust boundaries.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```
