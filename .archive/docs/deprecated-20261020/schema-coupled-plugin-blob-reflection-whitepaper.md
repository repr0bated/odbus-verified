# Schema-Coupled Plugin Object Blobs and Active gRPC Reflection

Status: working design, first implementation pass in `op-grpc-bridge`
Date: 2026-06-30

## Executive Summary

The plugin system is moving from a monolithic live schema catalog toward individual, immutable plugin object blobs. Each blob is created when a D-Bus plugin object is created. The blob couples four identities that must not drift:

- the `PluginSchema` identity
- the D-Bus object identity
- the generated gRPC service identity
- the gRPC reflection descriptor identity

The schema remains the single source of truth. The blob is not a second source. It is the frozen projection of the schema into the bridge runtime, carrying enough metadata for D-Bus dispatch, gRPC method discovery, tonic reflection, GUI rendering, MCP/tool exposure, and compliance/audit indexing.

The critical architectural correction is that `tonic_reflection::server::Builder` is static after build. It cannot add or remove services at runtime. Therefore active plugin discovery cannot be implemented by mutating tonic reflection directly. Instead, the bridge owns an active reflection catalog keyed by plugin object blobs. The reflection service reads this catalog when answering reflection requests.

In practical terms:

- D-Bus remains the authority and execution control plane.
- gRPC is the typed external and service-to-service transport.
- Clients discover callable gRPC methods through reflection.
- Reflection advertises active plugin object blobs, not the full universe of possible schemas.
- The old monolithic `/dev/shm/live-schema.json` shape becomes compatibility input only, not the target runtime model.

## Non-Negotiable Rules

1. `PluginSchema` is the source of truth.
2. A plugin object exists only if its schema can produce a valid object blob.
3. Every plugin object blob is created alongside its D-Bus object.
4. The blob must contain the D-Bus identity and the gRPC identity.
5. The blob must contain method metadata copied from `PluginSchema.methods`.
6. Reflection must advertise only callable mounted gRPC services.
7. No phantom reflection descriptors: if a method appears in reflection, the route must exist.
8. D-Bus passthrough is not the future public API.
9. Per-object blobs replace monolithic live schema as the runtime shape.
10. The Sled/shared memory path stores blobs as object artifacts, not a global polling database.

## Terminology

`PluginSchema`
: The canonical contract. Methods, side effects, capabilities, metadata, JSON rendering shape, and state schema derive from here.

Plugin object blob
: The frozen bridge-local projection of one plugin schema and one D-Bus object identity into one gRPC/reflection identity.

Active reflection catalog
: The bridge-owned runtime index of active blobs. It answers reflection list and descriptor lookups.

D-Bus object
: The authoritative local object at `/org/opdbus/v1/plugins/<name>`.

Generated gRPC method service
: A tonic service generated from plugin schemas at build time, currently under `operation.plugin.v1.<Plugin>PluginMethods`.

Descriptor set
: Encoded protobuf `FileDescriptorSet` bytes used by reflection clients to discover services, methods, and messages.

## Target Data Model

Each plugin object blob carries:

```text
PluginObjectBlob
  plugin_id
  schema_version
  schema_hash
  schema_json
  dbus
    bus_name
    object_path
    interface_name
  grpc
    package
    service_name
    descriptor_set
  methods[]
    schema_name
    grpc_name
    grpc_path
    subid
    required_capability
    side_effect
    idempotent
    args_schema
    returns_schema
```

This is deliberately redundant at the blob boundary. The schema is still authoritative, but the blob must be self-describing enough that reflection, GUI, compliance, and dispatch can reason about the same object without reassembling identity from scattered files.

## Blob Creation Pipeline

1. The plugin defines or returns `PluginSchema`.
2. The plugin is instantiated as a D-Bus object.
3. The bridge creates a `PluginObjectBlob`.
4. The bridge canonicalizes the schema JSON.
5. The bridge computes the schema hash.
6. The bridge derives D-Bus identity.
7. The bridge derives gRPC identity.
8. The bridge copies method metadata from `PluginSchema.methods`.
9. The bridge attaches descriptor bytes for the callable gRPC service surface.
10. The bridge inserts the blob into the active reflection catalog.
11. The reflection catalog rebuilds its service/symbol/file index.
12. gRPC reflection clients now see the active plugin service.

Removal is the inverse:

1. The D-Bus object is removed or deactivated.
2. The bridge removes the plugin object blob from the active catalog.
3. The reflection catalog rebuilds its advertised service list.
4. Reflection no longer lists that plugin service.

The descriptor files can remain indexed while service advertisement changes. This lets `file_containing_symbol` and `file_by_filename` keep working for known generated descriptors while `list_services` reflects active objects.

## Why Not Monolithic Live Schema

The monolithic live schema file is useful as a bootstrap and compatibility artifact, but it is the wrong runtime primitive.

Problems with monolithic live schema:

- It blurs object identity and schema inventory.
- It makes active/inactive plugin state ambiguous.
- It encourages polling.
- It turns runtime discovery into catalog scraping instead of object registration.
- It does not naturally couple D-Bus object path, gRPC service name, reflection bytes, and method dispatch metadata.
- It makes partial updates risky because one file represents too many unrelated objects.

Per-object blobs solve this:

- one D-Bus object, one blob
- one schema hash, one blob identity
- one gRPC service identity, one blob
- one artifact to hash, audit, publish, remove, or replay

The eventual shared memory layout should look like object blobs, not a single mutable schema registry:

```text
/dev/shm/opdbus/plugin-blobs/
  zeroclaw.<schema_hash>.blob.json
  wireguard.<schema_hash>.blob.json
  persona.<schema_hash>.blob.json
```

The exact storage can later become binary, mmap-backed, or sled-backed. The important point is object granularity.

## Reflection Model

Tonic reflection has two separate concerns:

1. Descriptor availability
2. Service advertisement

Descriptor availability means the server can return `FileDescriptorProto` bytes for files, messages, services, and methods.

Service advertisement means `ListServices` returns the services clients should consider active and callable.

The current implementation direction separates these:

- descriptor files come from compiled descriptor sets
- active service names come from active plugin object blobs

This prevents reflection from listing unmounted or inactive services while still allowing the bridge to use generated descriptor files.

The blob helper now also synthesizes a plugin-level `FileDescriptorSet` directly from `PluginSchema.methods`. This closes the self-contained blob identity gap: the blob no longer needs an externally supplied descriptor set. The remaining implementation step is to make the mounted tonic service generation use the same request/response message synthesis, because reflection must never advertise a typed body that the mounted route does not accept.

## How To Call A Plugin Object

The runtime contract is object-first. A plugin exists as a D-Bus object at:

```text
/org/opdbus/v1/plugins/<plugin_id>
```

The canonical bus name for the plugin tree is:

```text
org.opdbus.v1.plugins
```

Typical call patterns:

### D-Bus method call

Inspect the object first:

```bash
busctl --system introspect org.opdbus.v1.plugins /org/opdbus/v1/plugins/unix_socket
```

Call a plugin method by object path and interface:

```bash
busctl --system call \
  org.opdbus.v1.plugins \
  /org/opdbus/v1/plugins/unix_socket \
  org.opdbus.v1.plugins.PluginV1 \
  createunixsocket \
  s a{sv}
```

The exact signature is derived from the plugin schema. For `createunixsocket`, the bridge accepts a structured payload with the container name and port list, and routes that through the `unix_socket` plugin.

### gRPC bridge call

The bridge exposes the typed `operation.v1.PluginService.CallMethod` RPC. The request contains:

- `plugin_id`
- `object_path`
- `interface_name`
- `method_name`
- `arguments`

Example shape:

```json
{
  "plugin_id": "unix_socket",
  "object_path": "/org/opdbus/v1/plugins/unix_socket",
  "interface_name": "org.opdbus.PluginV1",
  "method_name": "createunixsocket",
  "arguments": [
    { "name": "netmaker", "ports": [8081] }
  ],
  "actor_id": "netmaker-install",
  "capability_id": "netmaker.socket.registration"
}
```

### Reflection lookup

Use `grpcurl` against the bridge when you want the generated method surface:

```bash
grpcurl -plaintext 127.0.0.1:8090 list operation.plugin.v1.ZeroclawPluginMethods
```

The reflection service advertises only active plugin blobs. If a plugin is not mounted, it should not appear in `ListServices`.

## Why tonic-reflection Builder Is Not Enough

`tonic_reflection::server::Builder` accepts descriptor sets, indexes them, and returns a reflection service. Once built, that service has no supported mutation API.

This means it cannot:

- add a plugin service after server startup
- remove a plugin service after D-Bus object removal
- re-index per-object blobs as they become active
- represent active object lifetime

The bridge-owned `ActiveReflectionCatalog` fixes this by becoming the mutable layer. The reflection server reads from that catalog instead of from tonic's private immutable index.

## ActiveReflectionCatalog Responsibilities

The active reflection catalog owns:

- active plugin blobs
- descriptor sets
- filename index
- symbol index
- active service list

It must support:

- `upsert_blob(blob)`
- `remove_blob(plugin_id)`
- `list_services()`
- `file_by_filename(filename)`
- `symbol_by_name(symbol)`

It rebuilds indexes after blob add/remove. This is acceptable because plugin object lifecycle is low-frequency compared with request handling. If needed later, the index can become incremental.

## Generated gRPC Method Shape

The currently mounted first-pass generated method services use:

```proto
package operation.plugin.v1;

import "google/protobuf/struct.proto";

service ZeroclawPluginMethods {
  rpc GetState(google.protobuf.Struct) returns (google.protobuf.Struct);
}
```

This is intentionally pragmatic. It gives each plugin a real tonic service and each method a real reflected RPC path while avoiding an immediate explosion of generated request/response message types.

The second pass can make methods fully typed:

```proto
service ZeroclawPluginMethods {
  rpc RegisterAgent(RegisterAgentRequest) returns (RegisterAgentResponse);
}
```

That second pass should still be generated from `PluginSchema.methods`, not hand-authored proto files.

The blob descriptor synthesis has already moved toward the second-pass shape by deriving request and response messages from `MethodDecl.args` and `MethodDecl.returns`. `build.rs` still needs to generate matching tonic services before those richer descriptors become the externally advertised callable reflection contract.

## D-Bus Dispatch Model

The generated gRPC method handlers do not become the authority. They are transport handlers.

The handler flow is:

1. gRPC request arrives.
2. Ghostbridge identity/capability metadata is read.
3. Required method capability is checked against schema metadata.
4. Request payload is converted into schema-shaped JSON.
5. The bridge dispatches the method through the mutation engine.
6. The mutation engine calls the D-Bus object method.
7. The D-Bus result is converted back into gRPC response shape.

This keeps D-Bus as the execution authority while removing the need for external clients to call D-Bus passthrough.

## D-Bus Passthrough Position

D-Bus passthrough is a compatibility and debugging surface, not the target API.

The goal is not to remove D-Bus. The goal is to stop exposing raw D-Bus passthrough where a schema-generated gRPC method exists.

Target rule:

- If a plugin method exists in `PluginSchema.methods`, clients call the generated gRPC method.
- The bridge internally dispatches through D-Bus.
- Passthrough remains only for transition, emergency inspection, or objects not yet converted.

## Introspection Position

D-Bus introspection is useful for generation and audit, not as the runtime contract.

Good uses:

- detect methods that exist on D-Bus but are missing from schema
- detect schema methods that are not implemented on D-Bus
- bootstrap an auto plugin generator
- create migration reports
- verify object consistency during tests

Bad uses:

- replacing `PluginSchema` as the source of truth
- generating ad hoc runtime method contracts
- allowing untyped passthrough to become the main API again

The auto plugin generator should use introspection as evidence, then write/update schema-owned method declarations.

## Schemars Position

Schemars is the seed for state and input/output schema generation. It is not enough by itself to define the full plugin contract because methods, side effects, capability requirements, subids, and gRPC identity need operational metadata.

The right layering is:

```text
Rust state/input/output structs
  -> schemars JSON Schema
  -> PluginSchema methods/properties/metadata
  -> PluginObjectBlob
  -> D-Bus object + gRPC service + reflection
```

The old confusion came from helper files looking like schema definition files. The cleaner pattern is:

- plugin owns the schema contract
- schemars derives structural JSON schema
- helper/scaffold code processes output
- blob freezes the projection

## Compliance Metadata

Every method blob should carry at minimum:

- `subid`
- `required_capability`
- `side_effect`
- `idempotent`
- schema hash
- object path
- gRPC path

Future compliance metadata should include:

- `uuid`
- `control_refs`
- `statement_refs`
- `control_source`
- `actor_id` requirements for mutations
- `capability_id` requirements
- event hash linkage
- Snowball session ledger references

The blob is the right place to attach this because it is where schema, D-Bus identity, gRPC method identity, and reflection identity meet.

## First Implementation Pass

The first pass should prove the full path with representative plugins:

- Zeroclaw
- Wireguard
- Keypair
- Identity
- Persona
- Qdrant
- Netmaker
- Cozo

Minimum success criteria:

- each plugin has a valid `PluginSchema`
- each plugin has methods in `PluginSchema.methods`
- each plugin can produce a `PluginObjectBlob`
- each blob has a schema hash
- each blob has D-Bus identity
- each blob has gRPC identity
- each blob has method metadata
- active reflection advertises each active blob service
- generated gRPC method routes dispatch through D-Bus/mutation engine
- reflection does not advertise inactive blobs

## Second Implementation Pass

The second pass should increase type fidelity:

- generate per-method request messages from `method.args`
- generate per-method response messages from `method.returns`
- remove generic `google.protobuf.Struct` where schemas are stable enough
- enrich blob metadata with compliance controls
- write blobs to the object-level shared memory layout
- add D-Bus introspection audit checks
- add test fixtures for all high-value plugins

## Risks

Descriptor drift
: Reflection advertises a method whose route is not mounted. This is the worst client-facing failure and must be guarded by tests.

Schema drift
: D-Bus implementation has methods not represented in `PluginSchema.methods`.

Blob drift
: Blob metadata no longer matches the schema hash or D-Bus object path.

Monolithic fallback creep
: Old `/dev/shm/live-schema.json` remains treated as runtime truth instead of compatibility input.

Passthrough creep
: Clients keep using D-Bus passthrough even after generated gRPC methods exist.

Over-generation
: Generated proto services expose methods for plugins that are not active. Active reflection must filter service advertisement.

## Required Tests

Blob tests:

- blob contains canonical schema bytes
- blob hash is stable for the same schema
- blob D-Bus path matches plugin id
- blob gRPC service name matches generated service
- blob methods match `PluginSchema.methods`

Reflection tests:

- inactive plugin service is not listed
- inserting blob lists service
- removing blob removes service
- `file_by_filename` returns descriptor bytes
- `file_containing_symbol` returns descriptor bytes for active service

Dispatch tests:

- generated gRPC method reaches mutation engine
- capability denial happens before dispatch
- method args are converted correctly
- method result is converted correctly

Introspection audit tests:

- D-Bus method missing from schema is reported
- schema method missing from D-Bus is reported
- method signature mismatch is reported

## Current Code Landmarks

`crates/op-grpc-bridge/build.rs`
: Generates `operation.plugin.v1.<Plugin>PluginMethods` services from loaded plugin schemas.

`crates/op-grpc-bridge/src/plugin_object_blob.rs`
: Generic plugin object blob model and blobification helper.

`crates/op-grpc-bridge/src/zeroclaw_object_blob.rs`
: First plugin-specific blob consumer.

`crates/op-grpc-bridge/src/dynamic_reflection.rs`
: Active reflection catalog and v1 reflection service.

`crates/op-grpc-bridge/src/grpc_server.rs`
: Registers plugin method blobs and mounts the reflection service.

`crates/op-grpc-bridge/src/callable_reflection.rs`
: Static callable descriptor snapshot from compiled tonic descriptor bytes.

## Final Architecture Statement

The plugin schema is the seed. The D-Bus object is the authority. The plugin object blob is the frozen runtime identity. The generated gRPC service is the callable transport. Active reflection is the discovery surface. The Sled/shared memory layer stores object blobs, not a monolithic schema database.

That gives the system a clean chain:

```text
PluginSchema
  -> PluginObjectBlob
  -> D-Bus object authority
  -> generated gRPC methods
  -> active reflection discovery
  -> GUI/MCP/client typed calls
  -> Snowball/compliance audit trail
```
