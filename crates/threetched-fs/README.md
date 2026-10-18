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
- File and directory handles share a 256-slot ceiling; kernel cleanup uses the
  exact handle and inode, while I/O retains opener UID checks. An operation
  rejected before dispatch remains terminal on that descriptor, including close.
- Atomic store publication with locking, `fsync`, and rename.
- Plugin operations dispatched only through `org.opdbus.v1.PluginV1.Call`.
- Explicit confirmation required for non-read operations.

The default store is `/dev/shm/3tchedfs`, so it is intentionally ephemeral
across reboot. Pass `--store <path>` to use another location.

The binary-backed `threetched` plugin uses the shared discovery implementation
in `threetched-surface.rs`. Its methods and capture evidence enter the plugin's
`PluginSchema`; `opblob` seals that contract. Production mounts project it from
the catalog through the ordinary namespace builder. No extra mount argument,
mount-time command discovery or separately written schema file is needed.
The `threetched_fs` plugin continues to describe the filesystem protocol itself.

Calls use the identity-bound lungs connection, canonical method admission,
exact per-command capabilities and the existing operation receipt. Queued calls
recheck the caller and pinned seal under the shared threetched runtime guard.
The backend verifies the installed binary and manifest against sealed digests,
uses the service's sealed config directory, clears inherited environment and
runs as the existing threetched service account. Declaring a capability grants
nothing. Help text supplies no side-effect policy, so discovered commands are
conservatively declared as mutations and non-idempotent.

Input, output and receipt buffers retain their ownership charges until dropped.
Child execution has a 600-second deadline and a combined 16 MiB stdout/stderr
limit; cancellation terminates and reaps the child process group. Timeout,
cancellation or output loss must not trigger automatic retries. Direct local
FUSE execution is retained only in isolated lifecycle test fixtures. These are
source guarantees awaiting the owner's build and activation, not a live receipt.

The upstream CLI projection discovers commands recursively from the selected
binary's `--help`. Each callable command path becomes a method: for example,
`widgets publish` becomes `widgets.publish`. The sealed discovery descriptor retains each command's actual
argv prefix and help declaration. A fresh capture discovers added or removed
commands and flags without maintaining a threetched command table in Rust.
The binary's `config schema` remains a separate configuration document; it is
not substituted for the arguments of the `config` command.

Inputs use the exact discovered flag spelling. Presence switches use booleans;
value-taking flags carry literal CLI tokens, and `arguments` carries positional
tokens in their declared order. Discovery records only constraints that the help
actually declares. It does not guess integer, path, duration or other semantic
types from a flag name or placeholder. threetched remains responsible for
validation that its help does not expose. Unknown flags and unsupported help
syntax fail explicitly. Discovery subprocesses have deadlines and output limits.

This is the advertised CLI surface. Installed threetched plugins extend its runtime
registries rather than its CLI command tree. Their package `manifest.toml`, WASM
payloads and optional `skills/` files are separate runtime inputs; a WASM tool's
parameter schema comes from its component exports. CLI help and configuration
schema do not supply that tool schema. Upstream installation can also provide
`web/dist` and the `zerocode` companion app. The threetched build recipe here stages
the core binary, while golden preparation separately records bootstrap config
and default skills. It does not establish a complete upstream installation or
an inventory of the identity's installed extensions. Those files and their
upstream loaders must be accounted for before claiming that broader surface.

## Build

Requirements:

- Linux with FUSE enabled and access to `/dev/fuse`
- `fusermount3` for explicit unmounts
- A current stable Rust toolchain

```sh
cd /srv/git/odbus-verified
CXXFLAGS="-include cstdint" cargo build --workspace --release
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

## Add a threetched feature from the binary

This procedure applies to a CLI feature advertised by the selected binary.
The discovery adapter supplies declarations; **opblob seals the methods**.
Commands below describe the next authorized preparation run. They have not
been run for this integration while the owner's build hold is in effect.

1. **Make the feature exist upstream.** If the selected threetched binary already
   advertises the command and its flags in `--help`, no method table or argument
   struct needs adding here. If it does not, implement or obtain it in the
   pinned upstream source first. Update `vendor/tched-router/source.pin`, the
   source archive, reviewed patches and their digests together when changing
   that upstream input. A runtime WASM extension with only exported tool schemas
   is a different surface; its tools cannot be inferred from CLI help.

2. **Build the pinned threetched candidate.** From the repository root, have the
   assigned Cargo worker run the existing recipe on the GNU host toolchain:

   ```sh
   vendor/tched-router/build-threetched.sh \
     --cache-dir /var/cache/op-dbus-build/tched-router \
     --output-dir target/threetched-artifacts/tched-router
   ```

   The result is `threetched` plus `manifest.json`. The helper verifies source
   and patch provenance and checks the binary; it does not install or activate
   it. Inspect the actual new command's `--help` and `config schema`. If the
   configuration contract changed, replace the sole UPSTREAM IMPORT block in
   `tched_router_contract.rs` with the verified binary output and record its
   provenance. That configuration document does not define CLI flag types.

3. **Select capture inputs separately from installed paths.** Use the same
   environment for workspace build and sealing, so compiled compatibility
   evidence and the sealed declarations agree:

   ```sh
   export THREETCHED_CAPTURE_BIN="$PWD/target/threetched-artifacts/tched-router/threetched"
   export THREETCHED_CAPTURE_MANIFEST="$PWD/target/threetched-artifacts/tched-router/manifest.json"
   export THREETCHED_BIN=/usr/bin/threetched
   export THREETCHED_MANIFEST=/usr/share/op-dbus/threetched-manifests/tched-router.json
   export TCHED_ROUTER_CONFIG_DIR=/var/lib/tched-router
   export COGNITIVE_QDRANT_URL=http://10.200.0.2:6334
   ```

   Both capture selectors are required together and must be absolute. They
   select preparation artifacts. The installed paths and service context are
   recorded in the contract; runtime dispatch never uses the capture selectors.

4. **Build the workspace and review discovery.** After the owner permits the
   build, the assigned worker runs from the repository root:

   ```sh
   CXXFLAGS="-include cstdint" cargo build --workspace --release
   ```

   Use the host's `x86_64-unknown-linux-gnu` toolchain. The shared parser walks
   the binary's help tree and derives exact command paths, flag spellings,
   literal-token arguments and explicitly advertised constraints. Unsupported
   syntax, ambiguous names or conflicting transport field names stop
   preparation. Fix discovery against real help evidence; do not invent a
   handwritten argument schema. Each method gets a deterministic exact
   capability, and no grants are added by discovery or sealing.

5. **Seal an isolated release catalog with the freshly built opblob.** Choose a
   new preparation directory while retaining the prior catalog for comparison:

   ```sh
   threetched_release_catalog=$(mktemp -d /var/tmp/threetched-catalog.XXXXXX)
   target/release/opblob seal-plugins "$threetched_release_catalog"
   target/release/opblob catalog "$threetched_release_catalog"
   ```

   Keep the environment from step 3, including the service's other schema-shaping
   settings. This seals the workspace's full plugin set. Review additions,
   removals, method declarations and capability changes before release. Using
   `seal-shm` instead targets the live SHM catalog; use that only during an
   explicitly authorized catalog update, followed by `opblob persist`.

6. **Inspect the sealed feature through the existing projection.** This reads
   the prepared catalog and creates a disposable schema-only view:

   ```sh
   threetched_review=$(mktemp -d /var/tmp/threetched-review.XXXXXX)
   mkdir "$threetched_review/state"
   target/release/threetched-fs --store "$threetched_review/store" capture \
     --schema-only --catalog "$threetched_release_catalog" --state "$threetched_review/state"
   target/release/threetched-fs --store "$threetched_review/store" inspect \
     --plugin threetched > "$threetched_review/threetched.json"
   jq '.schema.methods | keys' "$threetched_review/threetched.json"
   ```

   For the chosen method, inspect `.schema.methods[method]` for exact inputs,
   output, capability and mutation policy. Its captured help and argv are under
   `.schema.example.surface.commands[method]`; its binary and manifest digests
   are in `.schema.example`. Command paths become dotted IDs, for example
   `widgets publish` becomes `widgets.publish`. That is an illustration, not
   an assumed installed command. A flag value uses literal tokens such as
   `{"--label":["release"]}` only when that exact flag is advertised. Use the
   sealed declaration to construct the real input.

7. **Prepare and activate the reviewed release.** Set
   `OP_PERSISTED_CATALOG_SOURCE` to the isolated catalog when running the
   existing golden preparation. Golden now verifies that the sealed threetched
   descriptor matches the candidate binary and manifest. Transfer the prepared
   read-only filesystem snapshot with btrfs send/receive and follow
   [local release activation](../../docs/src/getting-started/local-release-activation.md).
   Golden preparation does not seal, deploy, activate or restart services.
   Review or grant only the chosen method's declared capability through the
   existing identity policy process when authorized; discovery is not a grant.

8. **Verify through the identity's mounted namespace.** Confirm
   `plugins/threetched/schema.json` lists the method, then read its
   `operations/<encoded-method>/input.schema.json`, `output.schema.json` and
   `metadata.json`. The corresponding `call` file must exist. Use
   `threetched-fs describe --mount <mount> --expected-session <session> --plugin threetched`
   to discover the current surface. Invoke the selected method through that
   same mount using `threetched-fs call threetched <method> --mount <mount>
   --expected-session <session> --args-file <arguments.json> --confirm-side-effects`.
   Verify its terminal operation receipt and the authoritative resulting state.
   Also verify denial with an ungranted test identity and rejection of unknown
   flags in a disposable fixture. A successful build, mount, or local write is
   not proof of successful feature execution. Do not automatically retry an
   unknown outcome; use the declared receipt-recovery operation.

For an upgrade, repeat candidate capture, workspace build, sealing and release
review. New or removed advertised commands and flags are discovered again from
that binary. Rebuilding alone does not replace a sealed catalog, and restarting
the bridge does not reseal it.

## Catalog-pinned specialization and cognitive naming

`examples/typed_specialize.rs` derives `generated/plugin_types.rs` and
`docs/typed-specialization-report.json` from a manifest-pinned sealed catalog,
not from a second schema definition. The checked-in capture records catalog
generation 1494 and its original names and hashes. It is historical evidence,
not the replacement cognitive release contract.

After compiling and sealing the cognitive source cutover, regenerate these
artifacts from that matching catalog before using `examples/typed_use_generated.rs`
with the replacement release. Do not mechanically rename a captured plugin ID
while retaining its old schema hash. Runtime filesystem discovery continues to
derive plugin paths and operation contracts from the selected authoritative
catalog; this source change does not mutate an existing mounted namespace.
