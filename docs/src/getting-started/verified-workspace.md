# Verified workspace

Use this guide to prepare and check a local OP-DBUS Rust checkout. It covers
build and test prerequisites; running the Artix control plane still requires
separately provisioned host services and identity state.

## Prepare the environment

Work from the repository root containing `Cargo.toml` in an isolated Linux
development environment. Install the system tools required by the workspace:

```text
build-essential ca-certificates git curl pkg-config cmake clang llvm
libclang-dev protobuf-compiler libssl-dev libdbus-1-dev
jq gzip tar patch binutils
```

The portable UI build also requires Node.js 22.22.2 or newer and npm. The native
container artifact builders use `readelf`, `ldd`, and the GNU loader from the
build host; `binutils` supplies `readelf` and `strings`.

Use rustup with the baseline compiler and components:

```bash
rustup toolchain install 1.97.1 --profile minimal \
  --component rustfmt --component clippy
export RUSTUP_TOOLCHAIN=1.97.1
export CXXFLAGS="-include cstdint"
export CARGO_BUILD_JOBS=2
export CARGO_TERM_COLOR=never
export RUST_BACKTRACE=1
```

The helper installs nothing and does not start services, deploy binaries,
seal or persist the plugin catalog, or change host network and identity state.
It checks the current checkout in place. No API keys, production credentials,
or live endpoints are needed for compilation.

## Export a clean source closure

After the source and verification gates are ready, export a reviewable source
closure into a new directory outside this checkout:

```bash
scripts/export-verified-source.sh /srv/git/odbus /srv/git/odbus-verified
```

The exporter selects package roots from Cargo metadata, includes their current
untracked source and fixtures, and copies the required root files plus `src/`,
`examples/`, `deploy/`, `docs/`, `scripts/`, `.cargo/`, and `vendor/` when present.
It includes `AGENTS.md` and the four mandatory skill files. It rejects an
existing destination, missing Cargo target sources, dangling symlinks, and
symlinks that resolve outside the source checkout. It excludes inactive crates,
the retired Waypipe and standalone adapter crates, build output, caches, selected historical directories,
environment files, and common key/blob/database file extensions. These filename
rules do not establish that every remaining file is safe or belongs in a release;
review the selected inputs before admitting them to the fresh repository. The
export is source input only; it does not prove a clean build, runtime behavior,
deployment readiness, or a complete external dependency closure.

The production UI's development checkout is the sibling repository
`/srv/git/3tched.com`. The export does not depend on that checkout:
a portable archive of its committed source, exact content hashes and build
helper are included under `vendor/vercel-json-render-ui/`. The pin records
the captured production commit. Rebuild and
verify the UI without deploying it:

```bash
vendor/vercel-json-render-ui/build-assets.sh \
  --npm-cache /var/cache/op-dbus-build/npm \
  --output-dir target/ui-artifacts/vercel-json-render-ui
```

The helper runs `npm ci`, type checking, lint, tests and the Vite build with
base `/` and the production `https://10.0.0.3:8090` door, then emits an asset
archive and provenance manifest. Golden preparation validates and stages
`index.html` and `assets/` at the dashboard root, preserving sibling mounts. It never
consults the external checkout, invokes its rsync deployment script or changes
live dashboard files.

The tched_router native source pin, base hashes, patches and build helper are
included under `vendor/tched-router/`.
Build its reviewable candidate and manifest without installing it:

```bash
vendor/tched-router/build-native.sh \
  --cache-dir /var/cache/op-dbus-build/tched-router \
  --output-dir target/native-artifacts/tched-router
```

The helper defaults to the checked-in, digest-pinned `gzip -n` source archive,
so a fresh export needs neither a sibling checkout nor network access. It also
accepts a clean checkout at the exact pin with `--source-dir`, or a previously
verified uncompressed deterministic Git archive with `--source-archive`. Only
if the vendored archive is absent does it reuse a verified cache or fetch the
exact tag. It rejects dirty/wrong source, base-file drift,
patch fuzz, lockfile drift, an unexpected binary version and absence of the
versioned stateless-generation protocol. Output is
`target/native-artifacts/tched-router/{zeroclaw,manifest.json}`; the helper
never installs `/usr/bin/zeroclaw` or changes a service. Cargo registry/cache,
system packages and host services remain separately provisioned inputs.

Runtime generation also requires operator provisioning outside source control:
pair one client in the native gateway, write that raw token only to
`/etc/op-dbus/secrets/tched-router-runtime-token` as root with mode `0400` or
`0600`, and retain only the gateway-managed token hash in native configuration.
The bridge runit service reads this protected file and connects to the pinned
loopback endpoint `http://127.0.0.1:8082`; no token is minted by repository
scripts.

## Workspace checks

Run the checks from the repository root, preserving the first failing command
and its output:

```bash
sh scripts/check-workspace.sh preflight
cargo fetch --locked
sh scripts/check-workspace.sh smoke
sh scripts/check-workspace.sh verify
```

`preflight` checks tools, Linux, OpenSSL and Cargo workspace metadata. `smoke`
checks the workspace and runs the `op-core` and `op-plugins` library tests.
`verify` runs formatting, all-target/all-feature Clippy with warnings denied,
and the complete workspace test suite. A smoke pass does not replace full
verification.

The worker must allow isolated temporary Unix sockets for socket lifecycle and
Incus mock transport tests. If the environment disables D-Bus or local sockets,
record the exact test and error; do not weaken authentication, substitute
production state, or report full verification as passed.

All path dependencies and the patched `vendor/rovs-openflow` are in this
repository. `protoc` and `libclang` must be available on `PATH`. `op-ml` is
disabled in workspace membership. The active web UI is maintained in a separate
repository; the retired in-repository npm trees are not backend prerequisites.

## Operational boundaries

Do not import host `.env` or MCP configuration. Do not start application
daemons, invoke host service managers, run deployment scripts, or alter live
network, service, catalog, or identity state as part of workspace preparation.
Host services use runit, and releases use the golden btrfs send/receive
workflow described in `AGENTS.md`; neither is part of these checks.

Use current source and `AGENTS.md` as architecture authority. Read recent
relevant `SIGNALS.md` rows before non-trivial work, and use `WISHLIST.md` for
task status. Archived conversations, retired dashboards, and historical
receipts are evidence only and are not active source instructions.

## Diagnostics

| Failure | Check |
| --- | --- |
| Missing `protoc` or `libclang` | Install the protobuf compiler and libclang development package, then confirm both tools are visible in the shell. |
| Cozo/RocksDB C++ errors | Confirm `CXXFLAGS=-include cstdint` reaches Cargo and keep native compilation jobs low. |
| Exit 137 or disk exhaustion | Reduce `CARGO_BUILD_JOBS` to `1`, preserve the log, and reuse the build cache. |
| Registry, download, or TLS failure | Preserve the `cargo fetch --locked` error and check worker egress and CA certificates. |
| D-Bus or Unix-socket permission failure | Record the exact test and environment error; use an environment permitting isolated fixtures. |
| Startup requests catalog or identity state | Stop at workspace checks; runtime validation belongs in a separately provisioned environment. |
