---
name: grpc-expert
description: "Expert guidance for gRPC/Protocol Buffers work in the operation-dbus-proto (OP-DBUS) workspace — designing plugin methods, generating .proto/service definitions, wiring the op-grpc-bridge, or fixing plugins with missing gRPC methods. Use this whenever the user mentions gRPC, proto/protobuf, plugin methods, the gRPC bridge, tonic, reflection descriptors, 'missing plugins' in the gRPC diagnostic page, or the seal/freeze/hot pipeline — even if they don't say 'gRPC' explicitly (e.g. 'the plugin isn't showing up on the diagnostic page', 'add a method to plugin X', 'regen protos')."
metadata:
  version: 1.0.0
disable-model-invocation: false
---

# gRPC Expert — OP-DBUS

This workspace does **not** hand-write per-plugin `.proto` files. Proto generation is
automatic, driven entirely by each plugin's `PluginSchema`. If you remember one thing
from this skill: **fixing a plugin's gRPC surface means editing its Rust schema
function, not writing `.proto`.**

## The real pipeline (roll → seal → freeze → hot)

```
1. Plugin declares  → PluginSchema { methods: HashMap<String, MethodDecl>, signals: Vec<SignalDecl>, ... }
                       in crates/op-plugins/src/state_plugins/<plugin>.rs
2. Schema gets put   → op-projection's SchemaEngine publishes the catalog hash
                       (op_identity::schema_bridge::schema_catalog_hash())
3. Sealed by blob    → op-blob seals the plugin object into an immutable blob:
                       /dev/shm/opdbus/plugin-blobs/<plugin_id>.<schema_hash16>.blob
                       (op-blob is the ONLY writer — never write blobs elsewhere)
4. Descriptors       → TWO separate reflection layers exist, don't confuse them:
   for reflection       a) STATIC: crates/op-grpc-bridge/build.rs generates
                            plugin_methods.proto from every plugin's schema.methods,
                            compiles it with tonic_build into operation_descriptor.bin
                            (a protobuf FileDescriptorSet), served by tonic-reflection.
                         b) DYNAMIC: crates/op-grpc-bridge/src/dynamic_reflection.rs
                            (ActiveReflectionCatalog) hydrates live from the sealed
                            SHM blob catalog via hydrate_reflection_from_shm() —
                            this is the "frozen" descriptor set for what's actually
                            present right now, independent of what's compiled in.
5. Frozen            → the blob + its descriptors are immutable once sealed.
6. gRPC socket = hot → crates/op-grpc-bridge/src/grpc_server.rs registers the
                       generated per-plugin service (plugin_method_routes.rs,
                       add_routes()) on the tonic UDS bridge, dispatching each
                       RPC to call_generated_plugin_method_typed(plugin_id,
                       schema_name, ...) which routes through PluginService.CallMethod
                       — i.e. gRPC never bypasses D-Bus, it's a bridge, not a
                       second control plane.
```

A plugin shows green/connected on the gRPC diagnostic page **iff**:
- its `PluginSchema.methods` is non-empty (empty → `build.rs` silently skips it —
  see `collect_plugin_methods()`), **and**
- the plugin is sealed in the SHM blob catalog.

These are discovery prerequisites, not proof of execution. Canonical dispatch
also requires the method's complete declaration and plugin version to match the
compiled handler, argument validation, verified identity, and its exact grant.
An advertised method whose adapter is absent is unfinished work.

## Where to actually make changes

| Task | File |
|---|---|
| Add/fix methods for plugin `<name>` | `crates/op-plugins/src/state_plugins/<name>.rs`, inside `<name>_schema()` |
| Reusable input/output builders | `crates/op-plugins/src/state_plugins/plugin_scaffold_helpers.rs` |
| Regenerate the derived `.proto` + routes | Just rebuild — `build.rs` regenerates on every `cargo build -p op-grpc-bridge` (it `rerun-if-changed`s `../op-plugins/src/state_plugins`) |
| Wire a genuinely new (non-plugin-derived) service | `crates/op-grpc-bridge/proto/*.proto`, add to the `compile_protos` list + `rerun-if-changed` in `crates/op-grpc-bridge/build.rs`, then register in `crates/op-grpc-bridge/src/grpc_server.rs` |
| Inspect what's currently generated | `target/{debug,release}/build/op-grpc-bridge-*/out/plugin_methods.proto` (build artifact — never hand-edit) |

**Never** create files under a `/proto/plugin_methods/` directory or a
`plugin_methods_unified.proto` by hand — those paths don't exist in this repo and
aren't part of the real pipeline; that was a stale spec from an earlier session.
The single source of truth is `schema.methods`.

## Adding methods to a plugin (the actual workflow)

Look at `crates/op-plugins/src/state_plugins/proxy_server.rs` (`proxy_server_schema()`)
for the canonical pattern:

```rust
use op_state_store::{MethodDecl, PluginSchema, SideEffect};

schema.methods.insert(
    "start_proxy".to_string(),
    method_decl_from_schemars_with_output::<StartProxyInput, plugin_scaffold_helpers::AckOutput>(
        "start_proxy",
        SideEffect::Mutation,       // Read | Mutation
        false,                       // idempotent?
        "cap.network.proxy.start@v1",   // required_capability
        "mut.network.proxy.start@v1",   // subid — must follow the taxonomy in CLAUDE.md
    ),
);
```

- Define `StartProxyInput` (and a typed output struct, or reuse `AckOutput`) as
  `#[derive(schemars::JsonSchema)]` structs — the JSON Schema is derived, never
  hand-written as `serde_json::json!({...})` unless there's no existing helper.
- `method_decl_from_schemars_with_output::<Input, Output>` is the current
  (non-deprecated) helper; the single-generic `method_decl_from_schemars` is
  deprecated but still present for legacy callers.
- `subid` must be registered in the canonical registry per CLAUDE.md
  (`crates/op-plugins/src/state_plugins/oscal_subid_registry.rs`) — CI enforces
  uniqueness.
- For plugins with **zero** declared methods today (the ones missing from the
  diagnostic page), don't hand-author their methods one by one — see
  **The Auto-Creator Plugin** below. That's the actual deliverable: a plugin
  that finds these gaps and fills them itself, using real research per plugin.

## The Auto-Creator Plugin (an ongoing capability, not a one-off batch)

This is **not** a one-time task to fix ~11 named plugins. It's a standing
system capability: whenever the system encounters *any* missing plugin —
now, or a new one added in the future — it automatically generates that
plugin and submits it for **human review**, never auto-merges/auto-seals it
unreviewed. The current ~11 gaps are just the first real instances that
exercise this, not the scope.

Consistent with this workspace's "reactive, not polled" principle (CLAUDE.md):
this should be triggered by an actual arrival/lookup that reveals the gap
(e.g. a D-Bus/gRPC call or reflection lookup against a plugin whose
`schema.methods` is empty, or a plugin name that doesn't resolve at all),
not a cron-style sweep re-scanning all plugins on a timer.

Build it as a plugin itself (same `PluginSchema`/`MethodDecl` pattern as every
plugin in `crates/op-plugins/src/state_plugins/`), whose job is to:

1. **Detect a gap on encounter** — when something references a plugin that's
   missing or has `schema.methods.is_empty()` (same emptiness check
   `build.rs::collect_plugin_methods` already uses to skip plugins), that's
   the trigger — not a periodic scan of the whole registry.
2. **Research that specific gap** — drive deep research via the **NotebookLM
   MCP** (once wired up) to determine what capabilities/operations the
   missing plugin should realistically expose, given its existing state
   struct (if any), documentation, and prior art from comparable plugins
   already in the registry. This replaces guessing or generic Get/List/Watch
   stubs with a grounded, plugin-specific proposal.
3. **Synthesize typed schemas** — turn the research output into real Rust
   input/output structs (`#[derive(schemars::JsonSchema)]`) and `MethodDecl`
   entries — per the "no `AckOutput`" rule above, every synthesized method
   needs its own meaningful typed request/response, not a generic ack.
4. **Submit for human review — do not auto-apply.** The output is a proposed
   plugin/schema (e.g. a diff/PR against `<name>_schema()`, or a new
   `crates/op-plugins/src/state_plugins/<name>.rs` for a wholly new plugin),
   held for a human to approve before it's merged. Only after human approval
   does it enter the normal pipeline (build → proto → reflection → sealed
   blob → hot). The auto-creator must never seal a blob or register a plugin
   into the live catalog on its own. It never emits `.proto` directly and
   never writes to `/proto/plugin_methods/` — the single source of truth
   stays `schema.methods`, authored by a human-approved change.

In short: this is a **standing, reactive meta-plugin that drafts schemas for
whatever plugin is found missing, research-backed via NotebookLM, gated by
human review** — not a fixed batch job for a specific list of plugins.

## JSON Schema → proto mapping

The shared mapper lives in `crates/op-blob/src/json_schema_proto.rs`; the bridge
uses it for generated native messages. Structured objects become typed nested
messages; typed string-key maps retain their value types. Scalars preserve
presence, including explicit false, zero and empty strings. Unsigned 64-bit
contracts use `uint64`. Nulls, arbitrary JSON, unions and nested arrays use the
appropriate standard `Value`/`Struct`/`ListValue` dependencies. Arbitrary numbers
inside `Value` remain subject to protobuf's double-precision limit.

Two existing wire layouts have distinct numbering rules: generated native
bridge fields use stable FNV-1a numbers; sealed per-method messages retain their
sorted sequential numbering. Preserve both layouts. Canonical schema map keys
and declared transport method names are separate identities, not interchangeable
aliases. Consumers register the original sealed descriptors and never reseal,
regenerate, or hash them as an existence check.

## Known project-specific tensions vs. generic gRPC best practice

Be aware of these before applying textbook advice unmodified:

- **Do not use `AckOutput { success: bool }` for new/generated methods.**
  It exists in the codebase (see `proxy_server.rs`) as a legacy shortcut for
  "no meaningful return value," but every method a plugin exposes should have
  a real, dedicated typed **input** struct and typed **output** struct that
  reflect what the operation actually consumes and returns — not a generic
  bool. This matters especially for the auto-creator (below): when it
  synthesizes methods for a previously-empty plugin, it must generate a
  specific `<Method>Input` and `<Method>Output` schemars struct per method,
  never fall back to `AckOutput`. Errors still go through `tonic::Status` /
  D-Bus error replies, never a `success: false` field.
- **Enums are flattened to `string`, not real proto `enum`.** JSON Schema
  `enum` currently maps to proto `string` (see table above), so the classic
  `FOO_STATUS_UNSPECIFIED = 0` convention isn't applied by the generator today.
  If asked to add real proto enums, that requires changing
  `json_schema_type_to_proto` in `crates/op-grpc-bridge/build.rs`, not just the
  plugin schema.
- **Streaming comes from state in SHM.** The owner reaffirmed this on
  2026-09-13. Methods update authoritative SHM state before notifications;
  existing state subscriptions read that state. Current generated method RPCs
  being unary does not require another streaming method/service. Preserve
  private principal/session scope. Method arguments, event payloads and
  generator buffers are not independent state authorities, and buffered unary
  responses are not a substitute for SHM state streaming.
- **One generated file per bridge crate**, not the "1-1-1" file-per-message
  convention — `plugin_methods.proto` bundles every plugin's messages/services
  in one generated file. That's fine; it's a build artifact, not hand-maintained
  source, so the usual "small modular .proto files" guidance doesn't apply to it.

## Generic protobuf/gRPC conventions (still apply to hand-written `.proto`)

For any `.proto` you *do* hand-write (domain services under
`crates/op-grpc-bridge/proto/`, not plugin-derived ones):

- `snake_case` field names, `CamelCase` message/service/RPC names, package names
  lowercase (this repo uses `operation.plugin.v1`-style packages).
- Reserve field numbers for removed fields; never reuse them.
- Prefer `optional` for fields that may be absent; avoid letting one field's
  value change the semantic meaning of another.
- Real proto `enum`s (where you control the generator, e.g. hand-written
  services) always start with `..._UNSPECIFIED = 0`.
- Use gRPC status codes (`tonic::Status`) for error paths; reserve response
  message fields for actual data.
- TLS is mandatory in this workspace (per CLAUDE.md zero-trust transport) —
  never wire a new gRPC service as plaintext, even over UDS.
- Server reflection (`tonic-reflection`) must stay wired for every new service
  so it's discoverable by `op-cognitive-mcp` and the gRPC diagnostic page —
  don't add a service that bypasses reflection registration.

## Quick diagnostic checklist

If a plugin isn't showing up / isn't callable over gRPC:

1. Does `crates/op-plugins/src/state_plugins/<name>.rs`'s `<name>_schema()`
   actually populate `schema.methods`? (Empty → `build.rs` skips it entirely.)
2. Is the plugin registered via `inventory::submit! { PluginReg::new(...) }` in
   its own file (self-registration — check `crates/op-plugins/src/default_registry.rs`
   only if the plugin doesn't show up in `available_plugins()` at all)?
3. Did `cargo build -p op-grpc-bridge` actually rerun? (`build.rs` only reruns on
   changes under `../op-plugins/src/state_plugins` or `build.rs` itself.)
4. Is the plugin actually **sealed** in the SHM blob catalog
   (`/dev/shm/opdbus/plugin-blobs/`)? Static compiled-in proto support and the
   *dynamic* "hot" reflection/routing are two different gates — a plugin can
   compile fine and still be invisible if it was never sealed.
