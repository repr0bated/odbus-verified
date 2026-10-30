# OP-DBUS, 3tchedFS & Operator Console Technical Specification

This specification establishes the mandatory architectural invariants, implementation rules, and remediation standards across the OP-DBUS backend (`/srv/git/odbus`), the Golden Deployment pipeline (`deploy/`), the 3tchedFS FUSE projection (`/srv/3tchedFS`), and the Operator Console UI (`/srv/git/operation-dashboard-ui-07`).

---

## 1. Golden Deployment & Release Pipeline (`deploy/`)

### 1.1 Single-Build Constraint
- **Release Build**: All artifacts MUST be compiled from a single release pass:
  ```sh
  CXXFLAGS="-include cstdint" cargo build --workspace --release
  ```
- **Deployment Execution**: Releases MUST be published via `build-golden.sh` without ad-hoc binary copying.

### 1.2 Golden Subvolume Invariants
- **BTRFS Filesystem**: The destination (`/opt/op-dbus`) MUST reside on a BTRFS filesystem. Plain directory fallback is strictly prohibited.
- **Cryptographic MANIFEST**: Every golden subvolume MUST contain `MANIFEST` recording:
  - `golden-build` UTC timestamp
  - Git commit hash
  - Source path and init system identifier (`runit (sv)`)
  - Total binary count and SHA-256 hash for every staged executable.
- **Packaging Layout**:
  - `golden/bin/`: Executables
  - `golden/sbin/`: Control scripts & systemd shims
  - `golden/sv/<service>/`: Runit service definitions (`run`, `finish`, `log/run`)
  - `golden/etc/`: Environment defaults and pacman hooks.

### 1.3 Live Installation & Network Safety
- **Selective Binary Replacement**: The live installer MUST compare binaries (`cmp -s`) and only replace files that have changed.
- **Run Script Protection**: Existing host run scripts (`/etc/runit/sv/<svc>/run`) MUST NOT be overwritten if they differ from the repository version without explicit operator intervention.
- **Network-Critical Hold-Back List**: The installer MUST NEVER auto-restart:
  `ovs-vswitchd`, `ovsbr0-addr`, `ovsbr0-svc-addr`, `ovsbr0-uplink`, `uplink-dhcp`, `op-session-bus`, `opdbus-rundirs`, `dbus`.
  These services must be reported for deliberate manual restart (`sudo sv restart <svc>`).

---

## 2. 3tchedFS FUSE Projection Invariants (`/srv/3tchedFS`)

### 2.1 Dual SHM Authority Model
- **Schema Authority**: The schema contract MUST be read strictly from the sealed OPBLOB01 blob image (`/dev/shm/opdbus/plugin-blobs/<plugin>.<hash>.blob`).
- **Live Value Authority**: Pinned view mounts MUST read leaf scalar values (`NodeKind::LiveFile`) directly from `/dev/shm/opdbus/state/<plugin>.json`. Each `open()` syscall takes a single snapshot of the state file for that file descriptor.
- **Conformity Fallback**: If a live state file fails schema validation during capture, the daemon MUST fall back to `schema.template()` and emit a diagnostic warning.

### 2.2 Mount & Lifecycle Management
- **Mount Target**: The live production mountpoint is `/run/mount/3tchedFS`.
- **FUSE Cleanup**: All daemon mount invocations MUST pass `--auto-unmount` and `--allow-other` to prevent orphaned `ENOTCONN` mount corpses upon daemon restart.
- **Ephemeral Store**: The default backing store is `/dev/shm/3tchedfs` (tmpfs), ensuring all views and staged workspace layers are clean on boot.

### 2.3 Workspaces & COW Edits
- **Sparse Storage**: Workspaces MUST store only modified plugin objects.
- **Write-Time Validation**: Any staged write to a workspace MUST validate the full object against the plugin's JSON Schema before permitting the write.
- **Immutability Enforcement**: Any attempt to write to fields declared in `immutable_paths` or modify plugins declared as read-only MUST return `-EACCES` or `-EROFS`.

### 2.4 Operation Dispatch (`threetched-fs call`)
- **Schema Pre-Validation**: Arguments MUST be validated against `method.args` with `jsonschema::Validator` prior to D-Bus invocation.
- **Side-Effect Gate**: Methods with `SideEffect::Mutation` or `SideEffect::External` MUST be rejected unless `--confirm-side-effects` is provided.
- **Transport**: Method calls MUST dispatch over `/run/opdbus/session-bus.sock` via `org.opdbus.v1.PluginV1.Call`.

---

## 3. OP-DBUS Backend Invariants (`/srv/git/odbus`)

### 3.1 Blob System (`op-blob`)
- **Single-Writer Rule**: `op-blob` is the exclusive writer for `/dev/shm/opdbus/plugin-blobs/`.
- **Constructor Validation**: `BlobRef::new` MUST validate `SECTION_SCHEMA_JSON` and `SECTION_MANIFEST_JSON` presence and verify UTF-8 encoding during construction.
- **Atomic Commits**: Catalog directory state is committed via `.manifest.json` carrying the BLAKE3 leaf-fold hash.

### 3.2 Plugin Framework (`op-plugins`)
- **Authority**: `PluginSchema` in Rust is the single source of truth. All external crates import schema types via `op_plugins`.
- **Auto-Creator Protocol (`auto_create.rs`)**: Reactive gap detection with grounded NotebookLM capability synthesis, gated strictly by **human review**.

### 3.3 Authoritative Mutation Engine (`op-grpc-bridge`)
- **Mutation Authority**: All state mutations from gRPC or D-Bus MUST flow through `MutationEngine`.
- **Audit Persistence**: Every mutation MUST append to the `EventChain` and attempt replication to `StreamingSnowball` (`/var/lib/opdbus/snowball`).
- **EMQX Audit Tap**: The EMQX exhook provider MUST return `ResponsedType::Ignore` on all hook events to preserve native broker ACLs.

### 3.4 Ingress Security & Identity Sleds (`op-identity`)
- **WireGuard Decoy Termination**: Human WireGuard connections MUST terminate only at the Oracle Decoy edge node.
- **OIA1 Tokens**: Decoy verifies `/32` routes and mints 300s TTL assertions presented via `x-oracle-identity-assertion-bin`.
- **Memory-Map Safety**: Any process mapping an `IdentitySled` file MUST verify `file.metadata()?.len() >= 152` before `mmap`.

---

## 4. Operator Console Frontend Specifications (`operation-dashboard-ui-07`)

### 4.1 Proxy & Transport Discipline
- **Dev Proxy Rules**: `vite.config.ts` MUST route `/operation.`, `/op_chat.`, `/assistant.`, `/op.mcp.v1`, `/grpc.reflection`, `/pair`, and `/admin/paircode` to `GRPC_TARGET`.
- **Transport Centralization**: All gRPC-Web calls MUST use `@protobuf-ts/runtime-rpc` via `getTransport()`. `resetTransport()` MUST abort all active controllers before creating a new instance.
- **Stream Subscriptions**: `subscribe()` in `src/grpc/client.ts` MUST supply `includeSchema: req.includeSchema ?? false`.

---

## 5. Prioritized Remediation Backlog

1. **[DEPLOY-MAJ-01]** In `deploy/runit/build-golden.sh`, replace `/tmp/golden-changed.$$` with `mktemp` to eliminate predictable temp file race conditions.
2. **[UI-CRIT-01]** Fix `vite.config.ts` dev proxy rules for complete gRPC route coverage.
3. **[UI-MAJ-03]** Add missing `events` to catalog entries in `src/json-render/catalog/catalog.ts`.
4. **[UI-MAJ-04]** Fix element ID collision in `src/json-render/spec-builders.ts`.
5. **[3TCHEDFS-MAJ-01]** Handle `BrokenPipe` / `SIGPIPE` in `threetched-fs tree` to prevent CLI panic when piped.
