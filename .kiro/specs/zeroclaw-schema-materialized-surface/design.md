# Design — ZeroClaw schema-materialized surface

## Overview

The design has one spine: **the installed binary prints its own schema and
commands; that print is redistributed into locations 3tched already has — the
`zeroclaw` directory in the mount and the sealed blob — and a `call` there execs
the binary.** Nothing is compiled from the schema into a Rust plugin. The
materialized form is the FUSE tree and the blob, not a `PluginSchema`.

```
/usr/bin/zeroclaw 0.8.4
   │  zeroclaw config schema   (JSON Schema: 346 $defs, 84 top-level sections)
   │  zeroclaw --help          (command surface incl. hyphenated self-test)
   ▼
[A] 3tchedFS `zeroclaw` directory  ← IS the schema tree; built from the print
   │    operations/<command>/{call,input.schema.json,                 FIRST
   │                          output.schema.json,metadata.json}       PROOF
   │    a `call` write execs /usr/bin/zeroclaw <command> <args>       on host
   │    proven on /run/mount/3tchedFS via
   │    /opt/op-dbus/current/bin/threetched-fs
   ▼
[B] blob seal (op-blob → /dev/shm/opdbus/plugin-blobs/)
   │    the form that materialized in [A] is what is sealed;
   │    the blob holds the binary's schema + commands
   ▼
[C] blast survey → removal of the hand-rolled copy (tched_router + satellites)
   │    survey is provisional until [A] exists; then stay/remove against the tree
   │    — executed only AFTER the host proof, not in this spec
   ▼
[D] Container, guest systemd unit, guest socket — LAST (sequence note only)
```

The arrows are the fixed order (R6). `[A]` is built and sealed, in that
direction — schema into the tree, then the tree's schema into the blob (R3.2).
`[A]` must work on the host before `[C]` starts; `[D]` is last and is not tasks
in this revision.

3tched is the thing that becomes extendible. A new binary widens 3tched: its
schema redistributes into a wider `zeroclaw` tree. No Rust is regenerated.

## How the live surface actually works (observed, load-bearing)

The design is pinned to what the running system already does. The `zeroclaw`
directory slots into the existing shape; it does not invent one.

### The call-file model (leave it alone — R2.4)

`crates/threetched-fs/src/namespace.rs` builds, per object:

```
/plugins/<name>/
  object.blob.json   object.hash   schema.json   value.json   capabilities.json
  data/              (live state; I/O-gated)
  operations/<command>/
    call                 ← write surface; the caller's canonical call
    input.schema.json    ← the command's argument schema
    output.schema.json   ← the command's result schema
    metadata.json        ← {name,args,returns,side_effect,idempotent,
                            required_capability,subid}
/operations/<name>/<command> → symlink ../../plugins/<name>/operations/<command>
```

`FileSource` already enumerates exactly these leaves (`MethodInput`,
`MethodOutput`, `MethodMetadata`, and the `call`). The builder loops a source of
`(command → {input,output,metadata})` and emits them. **The design feeds that
builder the binary's schema directly: the `zeroclaw` object's commands, input
schemas, output schemas, and metadata are read from `zeroclaw config schema` and
`zeroclaw --help`.** No inode or call-file change (R2.4).

The scaffolding that populates these files is a schema redistributor: it reads
the binary's print and writes it into the leaves. It does not translate the
print into a `MethodDecl` and it does not restate commands.

### The seal path (reuse — R3.4)

The direction is one-way: read the binary's schema → build the `zeroclaw` tree
from it → seal **that tree's schema** into the blob via the existing `op-blob`
sealer and SHM catalog (`/dev/shm/opdbus/plugin-blobs/`). The seal does not seal
a Rust plugin and let the mount be a projection of it (R3.2). The sealed blob's
content is the binary's schema and command surface.

The live mount is downstream of the seal and is the **proof**, not a second
source. `.3tchedfs.json` records `source_catalog_hash`,
`source_catalog_generation`, and `declared_catalog_dir=/dev/shm/opdbus/plugin-blobs`;
after the seal, the running FUSE binary re-projects the catalog and the
`zeroclaw` object becomes visible in `/run/mount/3tchedFS`. That visibility is
how the host proof reads back the tree (R4) — the schema flows binary → tree →
blob, and the mount reflects the blob. Nothing reads the surface back out of the
mount as an authority.

### Identity enforcement (preserve — R2.5)

The mount is bound to one identity (`mount_identity.host_session`,
`local_call_owner_uid`, `allowed_operator_uid`). Reading a `read`-operation
`call` file was already refused for the operator (`Permission denied`), proving
the capability boundary is live at the FUSE layer. The `zeroclaw` operations
inherit the same gating; the `call` write stays the canonical, gated call.

## [A] The `zeroclaw` directory — the materialized form

### What it is

The materialized form **is** this directory tree in 3tchedFS and the blob that
backs it. There is no `crates/op-plugins/src/state_plugins/zeroclaw.rs`, no
generated `PluginSchema`, no `MethodDecl`. The files under
`plugins/zeroclaw/operations/<command>/` are the binary's schema, placed where
3tched already keeps per-object operation schemas.

The schema is read from the installed binary at materialization time and flows
into the tree. It is **not** a checked-in copy of `zeroclaw config schema` — a
captured copy in the repository would be the 26,032-line contract again (R1.1,
R1.4). The capture is a runtime input to the redistributor, held only long
enough to build the tree and seal the blob; the authoritative copy that persists
is the one inside the sealed blob, which came from this binary on this run.

```
/run/mount/3tchedFS/plugins/zeroclaw/
  schema.json        ← zeroclaw config schema (the binary's document)
  value.json  capabilities.json  object.hash  object.blob.json
  operations/<command>/
    input.schema.json   ← the command's argument schema (from the binary)
    output.schema.json  ← the command's result schema (from the binary)
    metadata.json       ← the binary's own fields for the command
    call                ← write → exec /usr/bin/zeroclaw <command> <args>
/run/mount/3tchedFS/operations/zeroclaw/<command> → symlink
```

`<command>` is the binary's command word, preserved verbatim: `status`,
`self-test`, `config`, `agent`, `gateway`, `doctor`, `security`, `estop`, …
(R1.3, R2.2). No `config_get`/`config_set`.

### Where the operation fields come from (no inferred taxonomy — R2.6)

`metadata.json`'s `side_effect`, `required_capability`, `subid`, `idempotent`
are populated **only** from what the binary's schema and help actually declare.
The design does **not** classify commands into read/mutation/diagnostic groups
and assign capabilities and subids from the group. Where the binary does not
declare such a field, the field is absent or carries a neutral,
non-authoritative value — it is not synthesized into a new method table. The
authority for the surface is the binary's print, nowhere else.

### Call semantics (exec, no plugin dispatch — R2.3)

A write to `plugins/zeroclaw/operations/<command>/call` execs
`/usr/bin/zeroclaw <command> <args>`:

- the arguments are checked against the operation's `input.schema.json` (the
  binary's own schema) before exec;
- the binary runs under the caller's verified identity and capability grant;
- the result is folded into the operation receipt and resulting state, read back
  from committed state.

The exec is wired at the FUSE call layer for the `zeroclaw` object. It is **not**
routed back through the plugin dispatch path, and there is **no** adapter that
restates the command as a `MethodDecl` and dispatches it as a plugin method
(R2.3). The `call` write is the canonical gated call; what it does once admitted
is exec the installed binary.

### Build-before-seal (R3.1, R3.2)

The `zeroclaw` tree is built from the schema first; the schema content that
materializes in the tree is then handed to the sealer. The mount is not a
downstream projection of a sealed Rust plugin — it is the tree the binary's
print produced, and the blob is sealed from that.

## [B] Blob seal

Unchanged mechanism (R3.4): `op-blob seal-shm` writes the `zeroclaw` schema into
a blob in `/dev/shm/opdbus/plugin-blobs/`, using the service's schema-shaping
environment as the deploy docs require. The sealed blob holds the binary's
schema and command surface (R4.3). Upgrade = replace binary → regenerate the
`zeroclaw` tree → reseal (R1.5). No second schema authority is introduced: the
embedded `const CONFIG_SCHEMA` in `tched_router_contract.rs` is removed by `[C]`,
leaving the binary's live print as the only authority.

## Host proof (R4)

Performed against the already-running mount and FUSE binary, no container:

1. Build/seal the `zeroclaw` tree from `/usr/bin/zeroclaw`.
2. Confirm the live catalog generation/hash advanced and `plugins/zeroclaw`
   appears in `/run/mount/3tchedFS` with `operations/<command>/` leaves whose
   `metadata.json`/`input.schema.json`/`output.schema.json` are the binary's
   output.
3. Perform one gated `call` through the host mount (a safe command first, e.g.
   `status` or `self-test`) and confirm it execs the installed binary and
   returns via the receipt and resulting state, identity enforcement intact.
4. Record the invocation and observations before any removal (R4.5).

## [C] Blast survey → removal (surveyed here; removed after the host proof)

The table below is a **blast survey**, not the design and not the decision
authority (R5.2). It records the hand-rolled surface found during authoring. It
stays provisional until the FUSE `zeroclaw` tree exists; then each row is marked
**stay** or **remove** against that tree. The removal is **not executed in this
spec** (R5.5).

Every surface carrying ZeroClaw behavior is a **remove** row — none are folded
into a generator (R5.1). A row is **stay** only if it is identity/capability
enforcement with no ZeroClaw behavior, or a layer separate from the mirror
(R5.3).

| Hand-rolled file / surface | Lines | Role | Survey disposition |
|---|---|---|---|
| `op-plugins/.../tched_router.rs` | 3147 | the plugin itself | remove |
| `op-plugins/.../tched_router_contract.rs` | 26134 | embedded frozen `config schema` (dead code) | remove |
| `op-plugins/.../tched_router_generation.rs` | 794 | surface generation glue | remove |
| `op-plugins/.../tched_router_legacy_methods.rs` | 290 | legacy method table | remove |
| `op-plugins/.../tched_router_legacy_methods_tests.rs` | 213 | tests of the above | remove |
| `op-plugins/examples/dump_tched_router_schema.rs` | 39 | dumps embedded schema | remove |
| `op-grpc-bridge/src/tched_router_runtime.rs` | 4150 | routing runtime mimicking binary | remove |
| `op-grpc-bridge/src/tched_router_generation_native.rs` | 1291 | native generation | remove |
| `op-grpc-bridge/src/tched_router_legacy_dispatch.rs` | 361 | legacy dispatch | remove |
| `op-grpc-bridge/src/tched_router_object_blob.rs` | 71 | object-blob glue | remove |
| `op-grpc-bridge/src/tched_config_engine.rs` | 443 | config engine over embedded schema | remove |
| `op-grpc-bridge/src/tched_config_dispatch.rs` | 119 | config dispatch | remove |
| `op-grpc-bridge/src/tched_own_agent_route.rs` | 163 | agent route (ZeroClaw behavior) | remove |
| `op-grpc-bridge/src/grpc/tched_router.proto` | — | derived proto | remove |
| `op-grpc-bridge/tests/tched_router_backing.rs` | 1609 | backing tests mimicking binary | remove |
| `op-grpc-bridge/tests/tched_router_integration.rs` | 349 | integration mimicking binary | remove |
| `op-grpc-bridge/tests/tched_config_proto_contract.rs` | 310 | proto contract | remove |
| `op-web/src/handlers/tched_router.rs` | 786 | web handler (ZeroClaw behavior) | remove |
| `op-web/src/tched_router_routes.rs` | 207 | web routes (ZeroClaw behavior) | remove |
| `op-plugin-lint/examples/zeroclaw-*.json/.md` | — | captured schema fixtures | remove |
| `op-grpc-bridge/src/tched_config_secret_policy.rs` | 490 | secret policy | review → **stay** only if pure capability policy with no ZeroClaw behavior; else remove |
| `op-grpc-bridge/src/tched_router_admission.rs` (+tests) | 123+138 | admission | review → **stay** only the identity/capability admission that is not ZeroClaw-specific; else remove |
| `op-grpc-bridge/src/tched_catalog_binding.rs` | 293 | catalog binding | review → **stay** if generic catalog, not ZeroClaw mirror |

Rows that **stay** by nature (not surveyed for removal): the FUSE crate
(`crates/threetched-fs/*` — inode/call-file layer, R5.3), the `op-blob` sealer,
identity/capability enforcement, unrelated plugins, and the deploy runit service
that owns the pinned native binary.

The ~40k-line concentration in this pile is the measured cost of the copy and
the target of the cleanup. The survey counts the blast radius; the FUSE tree,
once it exists, decides each row.

## [D] Container, guest systemd unit, guest socket — sequence note only

Starts only after the host FUSE `call` path works (R6.1.4). The guest path
reuses the same `zeroclaw` directory and blob over the guest's lungs socket; it
adds no second schema and no second method table. In this revision it is a
sequencing note, not a set of tasks.

## Error handling

- **Binary absent/updated:** the materialized form records `zeroclaw 0.8.4` and
  a schema digest; a mismatch at seal time is surfaced, not silently accepted.
- **Invalid call arguments:** rejected against the operation's
  `input.schema.json` before exec; no partial exec.
- **Capability denial:** the `call` write is refused at the FUSE/identity layer
  exactly as existing operations; recorded as a permission boundary, never
  routed around.
- **Binary nonzero exit:** captured into the output envelope and the receipt;
  surfaced as the operation's error, not swallowed.

## Verification strategy

- **Schema fidelity:** the `zeroclaw` tree's `schema.json` equals
  `zeroclaw config schema`; the operation set equals the `zeroclaw --help`
  command set, names preserved verbatim.
- **No regenerated plugin:** confirm no `PluginSchema`/`MethodDecl` artifact and
  no `state_plugins/zeroclaw.rs` was added; the surface exists only as the FUSE
  tree + blob.
- **Host proof:** the R4 steps on `/run/mount/3tchedFS` — presence, schema
  match, one gated `call` with receipt.
- **Seal consistency:** live and persisted catalog hashes match after reseal;
  `plugins/zeroclaw` present; `plugins/tched_router` still present until `[C]`.
- **Removal safety (deferred to execution, not this spec):** after `[C]`, build
  + `/verify`; confirm `zeroclaw` tree unchanged and `tched_router` gone.
- **Standard gate:** `/verify` (`cargo fmt --check` + `cargo clippy -D warnings`
  + `cargo test`) via the Luna Cargo dispatch, per repo policy.
