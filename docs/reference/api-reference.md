# API & Technical Reference — operation-dbus-proto

> Generated 2026-07-02 from current workspace source. Contract types are quoted
> from `crates/op-state-store/src/plugin_schema.rs` and
> `crates/op-state/src/plugin.rs`. For the big picture, read
> [`docs/overview/architecture.md`](../overview/architecture.md) first.

## 1. The `PluginSchema` contract

`PluginSchema` (in `op-state-store`) is the single source of truth for every
plugin. D-Bus method signatures, gRPC shapes, MCP tool inputs, and UI renderers
are all derived from it.

```rust
pub struct PluginSchema {
    pub name: String,                        // plugin id; drives D-Bus path/interface
    pub category: String,                    // registry / renderer / compliance grouping
    pub version: String,
    pub description: String,
    pub display_name: Option<String>,        // UI-only; does not affect D-Bus path
    pub fields: HashMap<String, FieldSchema>, // plugin state fields
    pub dependencies: Vec<String>,           // other plugins this depends on
    pub example: Option<Value>,
    pub immutable_paths: Vec<String>,        // always-readOnly JSON paths
    pub tags: Vec<String>,                   // e.g. ["immutable"]
    pub dialect: String,                     // JSON Schema dialect (default 2020-12)
    pub mutation_index: Option<u64>,         // identity-sled mutation counter
    pub subids: HashMap<String, String>,     // field/tool name → OSCAL subid
    pub methods: HashMap<String, MethodDecl>,// callable capability surface
    pub signals: Vec<SignalDecl>,            // emitted signals
    pub guarantees: PluginCapabilities,      // rollback/checkpoint/verify/atomic
}
```

Key methods:

| Method | Purpose |
|---|---|
| `PluginSchema::builder(name)` | Start a `PluginSchemaBuilder` |
| `validate(&state)` | Validate a state `Value`, returns `ValidationResult { valid, errors, warnings }` |
| `generate_template()` | Produce a default/example state object |
| `to_json_schema()` | Emit JSON Schema 2020-12 (`readOnly`, `propertyDependencies`, immutability tags) |
| `with_methods` / `with_signals` / `with_guarantees` | Builder-style setters |
| `is_valid()` | True when `name` and `version` are non-empty |

### 1.1 `FieldType` and `FieldSchema`

```rust
pub enum FieldType {
    String, Integer, Float, Boolean,
    Array(Box<FieldType>),
    Object(HashMap<String, FieldSchema>),
    Enum(Vec<String>),
    OneOf(Vec<FieldType>),   // discriminated union → JSON Schema oneOf
    Any,
}

pub struct FieldSchema {
    pub field_type: FieldType,
    pub required: bool,
    pub description: String,
    pub default: Option<Value>,
    pub example: Option<Value>,
    pub constraints: Vec<Constraint>,
    pub read_only: bool,                       // unconditional immutability
    pub read_only_when: Option<ReadOnlyCondition>, // conditional via propertyDependencies
}
```

`Constraint` variants (serde tag `type`, `snake_case`): `Min { value }`,
`Max { value }`, `Pattern { regex }`, `OneOf { values }`,
`RequiresField { field }`, `Custom { validator }`.

`ReadOnlyCondition { property, value }` makes a field read-only when another
field equals a value, rendered as JSON Schema `propertyDependencies`.

> `FieldType::OneOf` is a discriminated union (a *type* choice) and is distinct
> from `Constraint::OneOf` (a *value-membership* constraint). Do not conflate.

### 1.2 `MethodDecl` — callable methods

Every `MethodDecl` is exposed as both a D-Bus method and a gRPC route.

```rust
pub struct MethodDecl {
    pub name: String,                        // D-Bus method / gRPC route name
    pub args: Value,                         // JSON Schema for input args
    pub returns: Option<Value>,              // JSON Schema for return value
    pub side_effect: SideEffect,             // Read | Mutation
    pub idempotent: bool,
    pub required_capability: Option<String>, // caller footprint must grant this
    pub subid: String,                       // OSCAL subid (mut.* must carry actor/capability)
}

pub enum SideEffect { Read, Mutation }       // serde: "read" | "mutation"
```

### 1.3 `SignalDecl` — emitted signals

```rust
pub struct SignalDecl {
    pub name: String,
    pub payload: Option<Value>,   // JSON Schema for payload
    pub subid: String,            // category must be `evt`
}
```

### 1.4 `PluginCapabilities` — guarantees block

```rust
pub struct PluginCapabilities {
    pub supports_rollback: bool,
    pub supports_checkpoints: bool,
    pub supports_verification: bool,
    pub atomic_operations: bool,
}
```

This is the single canonical definition in the workspace; `op-state`
re-exports it.

## 2. The `StatePlugin` trait

Defined in `crates/op-state/src/plugin.rs`. All state plugins implement it.

```rust
#[async_trait]
pub trait StatePlugin: Send + Sync {
    fn metadata(&self) -> PluginMetadata { /* default from name()/version() */ }
    fn schema(&self) -> Option<PluginSchema> { None }   // return your PluginSchema here
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn is_available(&self) -> bool { true }
    fn unavailable_reason(&self) -> String { /* default */ }

    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff>;
    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult>;
    async fn verify_state(&self, desired: &Value) -> Result<bool>;
    async fn create_checkpoint(&self) -> Result<Checkpoint>;
    async fn rollback(&self, checkpoint: &Checkpoint) -> Result<()>;
    fn capabilities(&self) -> PluginCapabilities;
}
```

Supporting types:

- `StateDiff { plugin, actions: Vec<StateAction>, metadata: DiffMetadata }`
- `StateAction::{ Create, Modify, Delete, NoOp }`
- `ApplyResult { success, changes_applied, errors, checkpoint }`
- `Checkpoint { id, plugin, timestamp, state_snapshot, backend_checkpoint }`
- `PluginMetadata { name, version, description, author, license, dependencies, dbus_services, feature_schemas, object_schemas }`

Plugins self-register via `inventory::submit!` (a `PluginReg`) co-located with
their definition — there is no central dispatch list. Adding a plugin means
adding its `pub mod` in `state_plugins/mod.rs` and a `submit!` in that module.

## 3. D-Bus object model

D-Bus is the only control plane. Bus name: `org.opdbus.v1`.

### 3.1 Object path hierarchy

```
/org/opdbus/
├── state                              ← StateManager interface
└── v1/                                ← MirrorV1 interface
    ├── ovsdb/                         ← OvsdbV1 JSON-RPC interface
    │   ├── Bridge/{uuid}              ← ProjectedObjectV1
    │   ├── Port/{uuid}                ← ProjectedObjectV1
    │   ├── Interface/{uuid}           ← ProjectedObjectV1
    │   └── ...per OVSDB table
    ├── nonnet/{db}/{table}/{uuid}     ← ProjectedObjectV1
    ├── state/{entity_id}              ← ProjectedObjectV1 (enterprise)
    └── plugins/
        ├── <plugin>/                  ← PluginV1 (one per registered plugin)
        └── ...70+ plugins

/org/dbusmcp/Agent/<Type>              ← Agent interface (per agent type)
/org/opdbus/services                   ← services.v1.Manager interface
```

### 3.2 Interfaces

| Interface | Path | Selected members |
|---|---|---|
| `PluginV1` | `/org/opdbus/v1/plugins/<name>` | Properties: `name`, `version`, `description`; methods: `get_state`, `get_schema`, plus each `MethodDecl` |
| `MirrorV1` | `/org/opdbus/v1` | `publish_snapshot`, `reconcile`, `get_stats`, `list_paths` |
| `ProjectedObjectV1` | projected object paths | `json_data` property, `get_property(key)`, signal `data_updated` |
| `OvsdbV1` | `/org/opdbus/v1/ovsdb` | `transact`, `get_schema`, `list_dbs`, `create_bridge`, `add_port`, `list_bridges`, `list_ports` |
| `StateManager` | `/org/opdbus/state` | `apply_openflow_state(json)`, `query_state()`, `apply_contract_mutation(json)` |
| `Agent` | `/org/dbusmcp/Agent/<Type>` | `execute(task_json)`, `run_operation(op, path, args)`, `ping`; signals `task_completed`, `status_changed` |
| `services.v1.Manager` | `/org/opdbus/services` | `start`, `stop`, `restart`, `get_status`, `list_services`; signal `service_state_changed` |

`PluginSchema.methods` — not D-Bus XML introspection — is the authority for
method shapes, capability requirements, and dispatch routing. Introspection is
read-only metadata.

### 3.3 Discovering and calling a plugin

```bash
# Introspect the plugin object
busctl --system introspect org.opdbus.v1.plugins /org/opdbus/v1/plugins/<plugin_id>
```

For typed RPC, call `operation.v1.PluginService.CallMethod` with the plugin id,
object path, interface name, method name, and structured args.

## 4. gRPC surface (op-grpc-bridge)

> **Full per-proto contract reference:** see [`proto/README.md`](./proto/README.md) for
> documentation of all 26 project-owned `.proto` files across 8 crates, with per-RPC
> request/response and streaming markers.

The bridge is bidirectional D-Bus ↔ gRPC with a central `SchemaEngine`.

| Service | Methods (selected) |
|---|---|
| `StateSync` | `Subscribe` (stream `StateChange`), `Mutate`, `GetState`, `BatchMutate` |
| `PluginService` | `ListPlugins`, `GetSchema` (JSON Schema), `CallMethod`, `Get/SetProperty`, `SubscribeSignals` (stream) |
| `EventChainService` | `GetEvents`, `SubscribeEvents` (stream), `VerifyChain`, `GetProof`, `ProveTagImmutability`, `GetSnapshot`, `CreateSnapshot` |
| `OvsdbMirror` (RFC 7047) | `ListDbs`, `GetSchema`, `Transact`, `Monitor` (stream), `GetBridgeState` |
| `RuntimeMirror` | `GetSystemInfo`, `ListServices`, `StreamMetrics` (stream), `ListInterfaces`, `GetNumaTopology` |
| `ComponentRegistry` | `Register`, `Deregister`, `Discover`, `Watch` (stream), `Heartbeat` |

### 4.1 Reflection

gRPC reflection advertises only **active** plugin object blobs. Two descriptor
layers:

- **Build-time** (`operation_descriptor.bin`): `google.protobuf.Struct`-typed
  routes compiled by `tonic_build` — Rust-level dispatch.
- **Runtime** (`PerMethodGrpcServices`): field-typed descriptors derived from
  `MethodDecl.args/returns` — reflection fidelity for `grpcurl`, MCP clients,
  Postman.

Both layers are required. Runtime freeze happens in
`op-grpc-bridge/src/grpc_server.rs::freeze_plugin_method_reflection()`, which
reads `/dev/shm/live-schema.json` at startup. Per-method typed descriptors are
generated in `op-grpc-bridge/src/plugin_grpc_gen.rs`.

## 5. MCP gateway surface

| Service | Bind | Audience |
|---|---|---|
| `op-cognitive-mcp` | `:3003` (WireGuard `100.90.37.254`) | Universal external gateway (NotebookLM, Droid, Cursor, Codex, Junie, Gemini CLI). Memory tools, code RAG, gRPC `CognitiveToolService`, auth. |
| `op-mcp` | default `[::1]:50051` | Unified MCP server; transports stdio / HTTP-SSE / WebSocket / gRPC; modes compact / agents / full |
| `compact-mcp` | `127.0.0.1:11436` | Loopback / chatbot only — never expose externally |
| `op-gateway` | (routed) | WireGuard-authenticated MCP router |

Rules: do not create new shim services, do not point external clients at
`op-assistant-grpc` directly, do not expose compact-mcp beyond loopback.

## 6. Port & service summary

| Port | Crate | Services | Transport |
|---|---|---|---|
| `[::1]:50051` | op-cache | AgentService, CacheService, OrchestratorService | gRPC |
| `[::1]:50051` | op-mcp (default) | McpService (compact/agents/full) | gRPC |
| `0.0.0.0:50052` | op-chat | Chat MCP server | gRPC |
| `50051-50060` | op-chat pool | Per-agent connections | gRPC |
| `:3003` | op-cognitive-mcp | Cognitive gateway (MCP + gRPC) | HTTP/SSE + gRPC |
| `127.0.0.1:11436` | compact-mcp | Chatbot loopback MCP | HTTP |
| system bus | op-dbus-mirror | MirrorV1, ProjectedObjectV1, OvsdbV1, NonNetV1 | D-Bus |
| system bus | op-state | StateManager, PluginV1 (per plugin) | D-Bus |
| system bus | op-agents | Agent (per type) | D-Bus |
| system bus | op-services | services.v1.Manager | D-Bus |
| bridge | op-grpc-bridge | StateSync, PluginService, EventChainService, OvsdbMirror, RuntimeMirror, ComponentRegistry | gRPC ↔ D-Bus |

> Ports are configurable and reflect current defaults; verify against the live
> service configuration and the port table in
> [`../architecture-flow.md`](../architecture-flow.md).

## 7. OSCAL subid reference

Format: `<category>.<component-type>.<subject>.<verb>[.<facet>][@vN]`

| Category | Classifies |
|---|---|
| `src` | authoritative source / ingress / source-of-truth store |
| `prj` | D-Bus projection or mirror publication step |
| `sch` | schema / contract / vocabulary / control-mapping artifact |
| `mut` | write-path operation that changes effective state |
| `obs` | read / query / enumeration / discovery path |
| `evt` | emitted signal / audit-chain event / proof |
| `exp` | consumer-facing render (MCP tool, UI, gRPC bridge view) |

Rules: `uuid` is machine identity and never replaced by `subid`; `subid` is an
OSCAL `prop` value (never in `remarks`); compliance mappings live in metadata
arrays, never in the `subid` string; `mut.*` records must carry `actor_id` +
`capability_id`; `evt.*` records must carry `event_id`/`event_hash`; `subid` is
immutable per subject (`@vN` on material change); uniqueness is enforced in CI.
Full definition and routing tags: [`../subid-taxonomy.md`](../subid-taxonomy.md).

## 8. Schema config projections (cognitive-mcp boundary)

Per `.kiro/specs/voyage-plugin-cognitive-mcp-boundaries`:

- `embedding_model` is the single Voyage config authority: provider, API-key
  env precedence (`COGNITIVE_MCP_VOYAGE_API_KEY` → `VOYAGE_API_KEY` →
  `VOYAGE_API_KEY_RUST`), endpoint URL derivation (public vs `al-` MongoDB
  prefix), active model id, dimensions (default 1024), and the `VoyageModel`
  enum.
- Consumers read runtime config from the plugin's D-Bus projection at
  `/dev/shm/opdbus/projections/<plugin>` (e.g. `.../embedding_model`,
  `.../cognitive_mcp`), falling back to env vars only when the projection is
  absent (bootstrap race).
- `op-cognitive-mcp` reads projections but never writes them; writing
  `/dev/shm/opdbus/projections/*` is `op-grpc-bridge`'s domain.
