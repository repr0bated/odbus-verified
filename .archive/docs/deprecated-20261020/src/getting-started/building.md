# Building the Workspace

All Cargo commands run from the repository root. For Linux packages, toolchain
setup and cloud-agent validation, follow the
[verified workspace guide](verified-workspace.md).

```bash
CXXFLAGS="-include cstdint" cargo build --locked --workspace
```

For a faster incremental check on a single crate:

```bash
cargo check -p <crate>
```

## Frontend

The active UI is a separate repository. The `crates/src/` and
`crates/op-web/ui/` trees are retired. Do not run their npm tasks as backend
build prerequisites. op-web serves files from `OP_WEB_STATIC_DIR` (default
`/usr/local/share/op-dbus/dashboard`) using `ServeDir`; it does not embed a
Vite build through RustEmbed.

## Release builds

```bash
CXXFLAGS="-include cstdint" cargo build --locked --workspace --release
```

The C++ flag is required by native Cozo/RocksDB compilation with modern GCC.
Compilation does not deploy services. Host releases require the explicit
catalog preparation and prepared-filesystem btrfs send/receive process in
`AGENTS.md`; do not run that process as development environment setup.
