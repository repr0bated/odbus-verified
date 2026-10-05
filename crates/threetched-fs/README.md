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

Calls can use an existing identity-bound FUSE mount directly:

```sh
threetched-fs call demo get_status --mount /run/mount/3tchedFS --args '{}'
# Optionally require the mount's declared session to match an expected owner:
threetched-fs call demo get_status --mount /run/mount/3tchedFS --expected-session <session-id> --args '{}'
# Or use the exact mount-relative operation path discovered in the namespace:
threetched-fs invoke plugins/demo/operations/get_status/call --mount /run/mount/3tchedFS --args '{}'
```

Mounted mode reads the catalog and target method's sealed declarations through that
namespace only. It pins the kernel mount instance and declared session/catalog,
and fails if they change. It never opens an object store, SHM state or another
transport. Calls validate the mounted method's arguments, write its existing
`call` file once, sync, and read the terminal receipt or denial through the same
open handle. A successful write alone is insufficient; method-global
`result`/`error` files are not used. The command returns the complete operation
receipt. Mutations retain `--confirm-side-effects`. Request and receipt sizes
are bounded. `--mount` conflicts with direct transport, view, workspace and
store selectors. No MCP adapter is involved in mounted calls.
`invoke` accepts only the canonical `plugins/<encoded-plugin>/operations/<encoded-method>/call`
path relative to the selected mount. Absolute paths, shortcut symlinks,
noncanonical encodings and traversal are rejected. A call reads only its target
plugin's object and schema; unrelated catalog members are not opened.

The `threetched_fs` plugin (display name `3tchedFS`) at
`/org/opdbus/v1/plugins/threetched_fs` declares one
`get_protocol` method describing this filesystem invocation protocol. When the
mount's pinned catalog contains that plugin, `.3tchedfs.json` includes an
`invocation` reference to `/plugins/threetched_fs/data/invocation/.value.json`; the descriptor
remains defined by the plugin's sealed schema. Exposing the new plugin requires
catalog sealing and release activation. The `invoke` helper also works with
existing compatible identity mounts that do not yet contain that plugin.

## MCP server (the identity's own)

`threetched-fs mcp` serves one identity over MCP on stdio, using the reference
Rust SDK (`rmcp`). Any IDE that launches an MCP server can use it; the IDE
holds no credential, because the connection is the identity.

On a host or guest that already has this identity's FUSE mount, the server
uses that namespace for tools, resources and calls. The process opens only
the mount:

```json
{
  "mcpServers": {
    "opdbus": {
      "command": "threetched-fs",
      "args": ["mcp", "--mount", "/run/mount/3tchedFS",
               "--expected-session", "<session-id>"]
    }
  }
}
```

A schema-only guest image that has no mount still launches the server against
its captured view and the lungs socket:

```json
{
  "mcpServers": {
    "opdbus": {
      "command": "threetched-fs",
      "args": ["--store", "/usr/share/op-dbus/lungs-view", "mcp",
               "--state", "/usr/share/op-dbus/lungs-empty-state",
               "--lungs-socket", "/run/opdbus/lungs/lungs.sock"]
    }
  }
}
```

Inside a provisioned container, omit `--lungs-session`: the bridge derives the
identity from the socket's kernel peer process and the exact running Incus
container binding. The schema-only store and empty state directory are prepared
in the shared base; no private state or credential is captured there. `--view`
selects a view hash in that store, not a mounted FUSE path. Host FUSE callers
running as root in the bridge's own user and PID namespaces may additionally
supply `--lungs-session <session>`.

- **Tools**: every sealed method in the view, named `plugin__method`, with
  the sealed argument schema as its input schema. Read methods are marked
  read-only and mutations destructive, so the IDE confirms them. Arguments
  are validated before dispatch; each call runs as the identity under its
  own grants.
- **Resources**: `3tched://view`, and per plugin
  `3tched://plugins/<plugin>/schema` and `.../state` (the mount's `value.json`
  for `--mount`, live SHM state for a lungs-backed view, the staged value for
  a workspace), plus templates including
  `3tched://plugins/{plugin}/state/{+pointer}`.
- **Prompts**: `plugin` (one plugin's methods, tools and state fields) and
  `catalog` (every plugin, one line each).
- **Completions**: plugin ids for the `plugin` prompt and resource templates.
- **Toolsets** (the default): a connection starts with one `toolsets` tool
  and loads what it needs (`{"operation":"load","ids":["net"]}`); the list
  changes and the client is notified. Sets are derived from the sealed
  catalog, one per plugin, so there is no hand-kept manifest. Loading only
  shapes the list: the identity can still call any method it is granted.
  `--load <set>` preloads, `--all` lists every method.
- **EMQX**: `--announce-toolsets` (or `threetched-fs toolsets --publish`)
  feeds the manifest to the broker through the sealed
  `emqx.publish_toolsets` method, as this identity. The broker keeps it
  retained on `opdbus/toolsets`; only live identities may subscribe, and
  nobody may publish there directly.
- `--compact` replaces the per-method tools with four meta-tools
  (`list_plugins`, `search_tools`, `get_tool_schema`, `execute_tool`) for
  small-context clients.

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
