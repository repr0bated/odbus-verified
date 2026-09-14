# Operation D-Bus Proto — Traffic Flow Diagrams

## 1. Full gRPC Traffic Flow

```mermaid
flowchart TB
    subgraph external["External Clients"]
        UI["Web UI / CLI"]
        LLM["LLM / Chat Client"]
        REMOTE["Remote Agents"]
    end

    subgraph mcp_layer["op-mcp :50051 (configurable)"]
        direction TB
        MCP_GRPC["McpService gRPC"]
        MCP_HTTP["HTTP/SSE Transport"]
        MCP_WS["WebSocket Transport"]
        MCP_STDIO["stdio Transport"]
        MCP_MODES["Modes: compact (5 lazy meta-tools)\nagents (cognitive)\nfull system tools"]
    end

    subgraph chat_layer["op-chat :50052"]
        direction TB
        CHAT_MCP["Chat MCP Server"]
        CHAT_ACTOR["ChatActor"]
        TOOL_LOADER["Tool Loader\n(D-Bus agent discovery)"]
        subgraph chat_agents["Agent Services (proto)"]
            AGENT_SVC["AgentService\nStartSession | Execute | BatchExecute"]
            MEMORY_AGENT["MemoryAgent\nRemember | Recall | Search"]
            SEQ_THINK["SequentialThinkingAgent\nStartChain | Think | Conclude"]
            CTX_MGR["ContextManagerAgent\nSave | Load | Export"]
            RUST_PRO["RustProAgent\nBuild | Test | Clippy | Fmt"]
            BACKEND_ARCH["BackendArchitectAgent\nAnalyze | Design | Review"]
        end
        subgraph chat_orch["Orchestration Services"]
            LIFECYCLE["AgentLifecycle"]
            EXECUTION["AgentExecution"]
            WORKSTACK["WorkstackService\n(workflow + rollback)"]
        end
    end

    subgraph cache_layer["op-cache [::1]:50051"]
        direction TB
        AGENT_REG["AgentService\nRegister | Execute | ExecuteStream\nFindByCapability | HealthCheck"]
        ORCH_SVC["OrchestratorService\nExecute | ExecuteStream | ExecuteAgents\nResolve | GetPatterns | PromotePattern"]
        CACHE_SVC["CacheService\nGetStep | PutStep | Invalidate\n(hash-based workstack cache + TTL)"]
        PATTERN["PatternTracker\n(workflow pattern learning)"]
        MCP_CACHE["McpService\nHandleRequest | ListTools"]
    end

    subgraph bridge_layer["op-grpc-bridge"]
        direction TB
        STATE_SYNC["StateSync Service\nSubscribe (stream) | Mutate\nGetState | BatchMutate"]
        PLUGIN_SVC["PluginService\nListPlugins | GetSchema (JSON Schema 2026)\nCallMethod | Get/SetProperty\nSubscribeSignals (stream)"]
        EVENT_CHAIN["EventChainService\nGetEvents | SubscribeEvents (stream)\nVerifyChain | GetProof\nProveTagImmutability\nGetSnapshot | CreateSnapshot"]
        OVSDB_MIRROR["OvsdbMirror (RFC 7047)\nListDbs | GetSchema | Transact\nMonitor (stream)\nGetBridgeState"]
        RUNTIME_MIRROR["RuntimeMirror\nGetSystemInfo | ListServices\nStreamMetrics (stream)\nListInterfaces | GetNumaTopology"]
        COMP_REG["ComponentRegistry\nRegister | Deregister | Discover\nWatch (stream) | Heartbeat"]
        SCHEMA_ENGINE["SchemaEngine\n(central state processor)"]
        CLIENT_POOL["GrpcClientPool\n(connection pooling\ndefault: 127.0.0.1:50051)"]
    end

    subgraph domain_services["Domain gRPC Services"]
        MAIL_SVC["MailService\n(operation.mail.v1)\nInbox | Send | ServerMgmt"]
        REG_SVC["RegistrationService\n(operation.registration.v1)\nMagic Link | WireGuard Provisioning"]
        PRIV_SVC["PrivacyNetworkService\n(operation.privacy.v1)\nwgcf + OVS + Xray\nContainer packet routing"]
    end

    subgraph services_layer["op-services"]
        SVC_MGR["ServiceManager\n(opdbus.services.v1)\nStart | Stop | Restart | Reload\nCreate | Delete | Enable | Disable\nWatchStatus (stream)"]
    end

    %% External → MCP
    UI -->|"HTTP/SSE\nWebSocket"| MCP_HTTP
    UI -->|"WebSocket"| MCP_WS
    LLM -->|"stdio"| MCP_STDIO
    LLM -->|"gRPC"| MCP_GRPC
    REMOTE -->|"gRPC"| MCP_GRPC

    %% MCP → Chat
    MCP_GRPC --> CHAT_MCP
    MCP_HTTP --> CHAT_MCP

    %% Chat internal
    CHAT_MCP --> CHAT_ACTOR
    CHAT_ACTOR --> TOOL_LOADER
    CHAT_ACTOR --> EXECUTION
    EXECUTION --> WORKSTACK

    %% Chat → Cache (agent pool, ports 50051-50060)
    CHAT_ACTOR -->|"gRPC pool\nports 50051-50060\ncircuit breaker"| AGENT_REG
    WORKSTACK -->|"orchestration"| ORCH_SVC

    %% Cache internal
    AGENT_REG --> CACHE_SVC
    ORCH_SVC --> CACHE_SVC
    ORCH_SVC --> PATTERN
    ORCH_SVC --> AGENT_REG

    %% Cache → Bridge
    AGENT_REG -->|"state queries"| CLIENT_POOL
    CLIENT_POOL -->|"gRPC"| STATE_SYNC
    CLIENT_POOL -->|"gRPC"| PLUGIN_SVC
    CLIENT_POOL -->|"gRPC"| EVENT_CHAIN

    %% Bridge internal
    STATE_SYNC --> SCHEMA_ENGINE
    PLUGIN_SVC --> SCHEMA_ENGINE
    EVENT_CHAIN --> SCHEMA_ENGINE

    %% Domain services
    CHAT_ACTOR -->|"gRPC"| MAIL_SVC
    CHAT_ACTOR -->|"gRPC"| REG_SVC
    CHAT_ACTOR -->|"gRPC"| PRIV_SVC

    %% Services
    SVC_MGR -.->|"dinit proxy"| RUNTIME_MIRROR

    %% Streaming indicators
    STATE_SYNC -.->|"stream: StateChange"| CLIENT_POOL
    PLUGIN_SVC -.->|"stream: Signal"| CLIENT_POOL
    EVENT_CHAIN -.->|"stream: ChainEvent"| CLIENT_POOL
    OVSDB_MIRROR -.->|"stream: OvsdbUpdate"| CLIENT_POOL
    RUNTIME_MIRROR -.->|"stream: MetricUpdate"| CLIENT_POOL
    COMP_REG -.->|"stream: RegistryEvent"| CLIENT_POOL

    classDef streaming fill:#e1f5fe,stroke:#0288d1,stroke-width:2px
    classDef server fill:#f3e5f5,stroke:#7b1fa2,stroke-width:2px
    classDef cache fill:#fff3e0,stroke:#e65100,stroke-width:2px
    classDef bridge fill:#e8f5e9,stroke:#2e7d32,stroke-width:2px
    classDef domain fill:#fce4ec,stroke:#c62828,stroke-width:1px

    class STATE_SYNC,PLUGIN_SVC,EVENT_CHAIN,OVSDB_MIRROR,RUNTIME_MIRROR,COMP_REG bridge
    class AGENT_REG,ORCH_SVC,CACHE_SVC,PATTERN,MCP_CACHE cache
    class MCP_GRPC,MCP_HTTP,MCP_WS,MCP_STDIO server
    class MAIL_SVC,REG_SVC,PRIV_SVC domain
```

---

## 2. Full D-Bus Traffic Flow

```mermaid
flowchart TB
    subgraph dbus_bus["System D-Bus"]
        direction LR
        BUS["org.freedesktop.DBus"]
    end

    subgraph mirror_svc["op-dbus-mirror (org.opdbus.v1)"]
        direction TB
        MIRROR_IF["MirrorV1 Interface\n/org/opdbus/v1\npublish_snapshot | reconcile\nget_stats | list_paths"]

        subgraph projected["Projected Objects"]
            OVSDB_OBJ["ProjectedObjectV1\n/org/opdbus/v1/ovsdb/{table}/{uuid}\njson_data property\nget_property(key)\nSignal: data_updated"]
            NONNET_OBJ["ProjectedObjectV1\n/org/opdbus/v1/nonnet/{db}/{table}/{uuid}\njson_data property\nget_property(key)\nSignal: data_updated"]
            ENT_OBJ["ProjectedObjectV1\n/org/opdbus/v1/state/{entity_id}\njson_data property"]
        end

        subgraph jsonrpc_if["JSON-RPC Interfaces"]
            OVSDB_IF["OvsdbV1 Interface\n/org/opdbus/v1/ovsdb\ntransact | get_schema | list_dbs\ncreate_bridge | delete_bridge\nadd_port | list_bridges | list_ports"]
            NONNET_IF["NonNetV1 Interface\n/org/opdbus/v1/nonnet\ntransact | get_schema | list_dbs"]
        end
    end

    subgraph data_sources["Data Sources (non-D-Bus)"]
        OVSDB_SOCK["OVSDB\n(JSON-RPC socket)\nOpen_vSwitch DB"]
        NONNET_DB["NonNet DB\n(JSON-RPC)"]
        ENT_SQLITE["Enterprise SQLite\n/var/lib/op-dbus/state.db\nlive_objects table"]
    end

    subgraph state_svc["op-state (org.opdbus)"]
        direction TB
        STATE_MGR["StateManager Interface\n/org/opdbus/state\napply_openflow_state(json)\nquery_state() -> JSON\napply_contract_mutation(json)"]

        subgraph plugin_hosts["Plugin D-Bus Hosts"]
            PLUG_HOST["PluginV1 Interface\n/org/opdbus/v1/plugins/{name}\nProperties: name, version, description\nMethods: get_state, get_schema"]
        end

        STATE_MANAGER_INT["StateManager (internal)\nPlugin aggregation\nSchema validation\nDiff + Apply"]
    end

    subgraph agent_svc["op-agents (org.dbusmcp.Agent.*)"]
        direction TB
        AGENT_IF["Agent Interface\n/org/dbusmcp/Agent/{Type}\nexecute(task_json) -> result_json\nrun_operation(op, path, args)\nagent_type | agent_id | name\noperations | status | metadata\nping"]
        AGENT_SIGNALS["Signals:\ntask_completed(task_id, success, result)\nstatus_changed(new_status)"]

        subgraph agent_types["Registered Agent Types"]
            PY_PRO["org.dbusmcp.Agent.PythonPro"]
            RUST_PRO_A["org.dbusmcp.Agent.RustPro"]
            DEBUGGER["org.dbusmcp.Agent.Debugger"]
            MORE_AGENTS["...other agents"]
        end
    end

    subgraph svc_mgr["op-services (org.opdbus.services)"]
        SVC_IF["Manager Interface\n/org/opdbus/services\nstart | stop | restart\nget_status | list_services\nSignal: service_state_changed"]
    end

    subgraph plugin_system["op-plugins"]
        direction TB
        REGISTRY["PluginRegistry\nBuild canonical schema\nCreate PluginCatalogDocument\nPersist to SqlitePluginCatalog\nExport PluginDbusHost"]
        SCHEMA_CAT["SchemaCatalog\n(in-memory indexed schemas)"]
        PUBLISHER["StatePublisher Trait\npublish_change(\n  plugin_id, path, change_type,\n  property, old_value, new_value,\n  tags, source\n)"]

        subgraph state_plugins["State Plugins (examples)"]
            SP_DINIT["dinit"]
            SP_OVSDB["ovsdb_bridge"]
            SP_PRIV["privacy_router"]
            SP_HW["hardware"]
            SP_DNS["dnsresolver"]
            SP_MORE["...30+ plugins"]
        end
    end

    subgraph model_store["op-dbus-model"]
        SQLITE_CAT["SqlitePluginCatalog\nPluginCatalogDocument:\n  schema + dbus_path\n  service_name + storage_path"]
    end

    %% Data source → Mirror
    OVSDB_SOCK -->|"JSON-RPC\nmonitor_db('Open_vSwitch')\nstream updates"| MIRROR_IF
    NONNET_DB -->|"JSON-RPC\nsubscribe() broadcast"| MIRROR_IF
    ENT_SQLITE -->|"SQLite read\nlive_objects"| MIRROR_IF

    %% Mirror publishes objects
    MIRROR_IF -->|"publish_ovsdb_snapshot()"| OVSDB_OBJ
    MIRROR_IF -->|"publish_nonnet_snapshot()"| NONNET_OBJ
    MIRROR_IF -->|"publish_enterprise_snapshot()"| ENT_OBJ

    %% Mirror → D-Bus
    OVSDB_OBJ -->|"Signal: data_updated\nPropertiesChanged"| BUS
    NONNET_OBJ -->|"Signal: data_updated\nPropertiesChanged"| BUS
    ENT_OBJ -->|"PropertiesChanged"| BUS

    %% State manager → D-Bus
    STATE_MGR -->|"register interface"| BUS
    PLUG_HOST -->|"register at\n/org/opdbus/v1/plugins/*"| BUS

    %% Agent services → D-Bus
    AGENT_IF --> BUS
    AGENT_SIGNALS --> BUS
    PY_PRO -->|"request name"| BUS
    RUST_PRO_A -->|"request name"| BUS

    %% Service manager → D-Bus
    SVC_IF -->|"service_state_changed"| BUS

    %% Plugin registration flow
    REGISTRY -->|"persist"| SQLITE_CAT
    REGISTRY -->|"update"| SCHEMA_CAT
    REGISTRY -->|"export PluginDbusHost"| PLUG_HOST
    REGISTRY -->|"publish schema"| PUBLISHER

    %% State plugins use DbusStatePluginBase
    SP_DINIT -.->|"DbusStatePluginBase\nconnect_dbus()"| BUS
    SP_OVSDB -.->|"proxy access"| BUS
    SP_HW -.->|"introspect"| BUS

    %% State manager aggregation
    STATE_MANAGER_INT -->|"query_current_state()\nper plugin"| REGISTRY
    STATE_MANAGER_INT -->|"validate against"| SCHEMA_CAT

    classDef dbus fill:#bbdefb,stroke:#1565c0,stroke-width:2px
    classDef signal fill:#ffecb3,stroke:#ff8f00,stroke-width:2px
    classDef source fill:#c8e6c9,stroke:#2e7d32,stroke-width:1px
    classDef plugin fill:#f3e5f5,stroke:#7b1fa2,stroke-width:1px
    classDef store fill:#fff3e0,stroke:#e65100,stroke-width:1px

    class BUS dbus
    class OVSDB_OBJ,NONNET_OBJ,ENT_OBJ,AGENT_SIGNALS signal
    class OVSDB_SOCK,NONNET_DB,ENT_SQLITE source
    class SP_DINIT,SP_OVSDB,SP_PRIV,SP_HW,SP_DNS,SP_MORE plugin
    class SQLITE_CAT,SCHEMA_CAT store
```

---

## 3. Combined Overlay: gRPC + D-Bus Bridge Points

This diagram shows where the two systems interconnect, with D-Bus in blue and gRPC in green.

```mermaid
flowchart TB
    subgraph external["External (gRPC/HTTP)"]
        UI["Web UI"]
        LLM["LLM Client"]
    end

    subgraph grpc_world["gRPC Domain (green)"]
        style grpc_world fill:#e8f5e9,stroke:#2e7d32

        MCP["op-mcp\ngRPC :50051\nHTTP/WS/stdio"]
        CHAT["op-chat\ngRPC :50052\nAgent orchestration"]
        CACHE["op-cache\ngRPC [::1]:50051\nAgent registry + cache"]

        subgraph grpc_services["gRPC-only Services"]
            MAIL["MailService"]
            REG["RegistrationService"]
            PRIV["PrivacyNetworkService"]
        end
    end

    subgraph bridge["op-grpc-bridge (THE BRIDGE)"]
        style bridge fill:#fff9c4,stroke:#f9a825,stroke-width:3px

        direction TB
        SCHEMA_ENG["SchemaEngine\n(central state processor)"]

        subgraph grpc_face["gRPC-facing Services"]
            SS["StateSync\nSubscribe/Mutate/GetState"]
            PS["PluginService\nListPlugins/GetSchema/CallMethod"]
            EC["EventChainService\nSubscribe/Verify/GetProof"]
            OM["OvsdbMirror\nTransact/Monitor"]
            RM["RuntimeMirror\nMetrics/Services/Interfaces"]
            CR["ComponentRegistry\nDiscover/Watch/Heartbeat"]
        end

        subgraph dbus_face["D-Bus-facing Components"]
            WATCHER["DbusWatcher\nMonitors:\n  /org/opdbus/v1\n  /org/opdbus/v1/ovsdb\n  /org/opdbus/v1/nonnet\nListens: PropertiesChanged\n         custom signals"]
            SYNC_ENG["SyncEngine\nBidirectional state sync"]
        end

        WATCHER -->|"process_dbus_change(\n  plugin_id, path,\n  change_type, value,\n  tags, source\n)"| SCHEMA_ENG
        SCHEMA_ENG --> SS
        SCHEMA_ENG --> PS
        SCHEMA_ENG --> EC
    end

    subgraph dbus_world["D-Bus Domain (blue)"]
        style dbus_world fill:#e3f2fd,stroke:#1565c0

        SYSTEM_BUS["System D-Bus"]

        subgraph dbus_publishers["D-Bus Publishers"]
            MIRROR["op-dbus-mirror\norg.opdbus.v1\nOVSDB + NonNet + Enterprise\nprojection"]
            STATE["op-state\norg.opdbus\nStateManager + PluginV1\nhosts"]
            AGENTS["op-agents\norg.dbusmcp.Agent.*\nPythonPro, RustPro, etc."]
            SERVICES["op-services\norg.opdbus.services\ndinit lifecycle mgmt"]
        end

        subgraph dbus_sources["Underlying Data Sources"]
            OVS["Open vSwitch\nOVSDB socket"]
            NONNET["NonNet DB"]
            SQLITE["Enterprise SQLite"]
            DINIT["dinit\n(service supervisor)"]
            RTNETLINK["rtnetlink\n(kernel networking)"]
        end
    end

    %% External → gRPC
    UI -->|"HTTP/WS"| MCP
    LLM -->|"gRPC/stdio"| MCP
    MCP --> CHAT
    CHAT -->|"agent pool\nports 50051-60\ncircuit breaker"| CACHE

    %% gRPC → Bridge
    CACHE -->|"gRPC client pool"| SS
    CACHE --> PS
    CHAT -->|"tool calls"| PS

    %% Bridge → D-Bus
    WATCHER ===|"zbus connection\nPropertiesChanged\ncustom signals"| SYSTEM_BUS

    %% Mutation path: gRPC → Bridge → D-Bus
    SS -->|"Mutate RPC"| SYNC_ENG
    SYNC_ENG -->|"D-Bus method call\nor property set"| SYSTEM_BUS

    %% D-Bus publishers → Bus
    MIRROR -->|"publish objects\ndata_updated signals"| SYSTEM_BUS
    STATE -->|"PluginV1\nplugin state + schema"| SYSTEM_BUS
    AGENTS -->|"task_completed\nstatus_changed"| SYSTEM_BUS
    SERVICES -->|"service_state_changed"| SYSTEM_BUS

    %% Data sources → Publishers
    OVS -->|"JSON-RPC\nmonitor"| MIRROR
    NONNET -->|"JSON-RPC\nsubscribe"| MIRROR
    SQLITE -->|"SQL read"| MIRROR
    DINIT -->|"dinitctl"| SERVICES
    RTNETLINK -->|"netlink"| RM

    %% Chat → Agents via D-Bus
    CHAT -->|"D-Bus introspection\nagent discovery"| SYSTEM_BUS
    CHAT -->|"execute(task_json)"| AGENTS

    classDef grpc fill:#c8e6c9,stroke:#2e7d32,stroke-width:2px
    classDef dbus fill:#bbdefb,stroke:#1565c0,stroke-width:2px
    classDef bridge_node fill:#fff59d,stroke:#f9a825,stroke-width:2px
    classDef source fill:#f5f5f5,stroke:#616161,stroke-width:1px

    class MCP,CHAT,CACHE,MAIL,REG,PRIV grpc
    class MIRROR,STATE,AGENTS,SERVICES,SYSTEM_BUS dbus
    class WATCHER,SYNC_ENG,SCHEMA_ENG,SS,PS,EC,OM,RM,CR bridge_node
    class OVS,NONNET,SQLITE,DINIT,RTNETLINK source
```

---

## 4. Detailed Sequence: State Mutation (gRPC → D-Bus → gRPC)

```mermaid
sequenceDiagram
    participant C as gRPC Client
    participant SS as StateSync<br/>(op-grpc-bridge)
    participant SE as SchemaEngine
    participant EC as EventChain
    participant W as DbusWatcher
    participant BUS as System D-Bus
    participant M as op-dbus-mirror
    participant DB as OVSDB / NonNet

    Note over C,DB: === MUTATION PATH (gRPC → D-Bus) ===

    C->>+SS: Mutate(plugin_id, path, op, value, capability_id)
    SS->>SE: validate against schema
    SE-->>SS: validation result
    SS->>BUS: D-Bus method call / property set
    BUS->>M: method dispatched
    M->>DB: JSON-RPC transact
    DB-->>M: result
    M->>BUS: update projected object + emit data_updated signal
    BUS-->>SS: method return

    Note over SS,EC: Record in event chain
    SS->>EC: record ChainEvent(actor_id, capability_id, decision=ALLOW, tags)
    EC-->>SS: event_id + event_hash + merkle_proof
    SS-->>-C: MutateResponse(success, proof)

    Note over C,DB: === NOTIFICATION PATH (D-Bus → gRPC) ===

    BUS->>W: PropertiesChanged signal
    W->>W: extract plugin_id from path
    W->>W: zvariant_to_json(value)
    W->>SE: process_dbus_change(plugin_id, path, PropertySet, prop, value, tags)
    SE->>SE: update internal state
    SE-->>C: stream: StateChange(plugin_id, path, property, new_value, tags)
```

---

## 5. Detailed Sequence: Agent Orchestration Flow

```mermaid
sequenceDiagram
    participant UI as Web UI / LLM
    participant MCP as op-mcp<br/>(multi-transport)
    participant CHAT as op-chat<br/>ChatActor
    participant POOL as AgentPool<br/>(ports 50051-60)
    participant CACHE as op-cache<br/>:50051
    participant ORCH as Orchestrator
    participant BUS as System D-Bus
    participant AGENT as op-agents<br/>D-Bus Agent

    UI->>MCP: tool call (HTTP/gRPC/stdio)
    MCP->>CHAT: route to ChatActor

    Note over CHAT,AGENT: === DIRECT D-Bus AGENT PATH ===
    CHAT->>BUS: introspect org.dbusmcp.Agent.*
    BUS-->>CHAT: agent metadata
    CHAT->>BUS: execute(task_json) on Agent interface
    BUS->>AGENT: dispatch method
    AGENT-->>BUS: result JSON
    BUS-->>CHAT: return
    AGENT->>BUS: Signal: task_completed(id, true, result)

    Note over CHAT,CACHE: === ORCHESTRATED CACHE PATH ===
    CHAT->>POOL: ExecuteStream (gRPC, circuit breaker)
    POOL->>CACHE: AgentService.Execute
    CACHE->>CACHE: check WorkstackCache (hash lookup)

    alt Cache HIT
        CACHE-->>POOL: cached result + latency=0
    else Cache MISS
        CACHE->>ORCH: OrchestratorService.Execute
        ORCH->>ORCH: resolve capabilities
        ORCH->>ORCH: build workstack (multi-agent chain)
        loop For each agent step
            ORCH->>CACHE: AgentService.Execute(step)
            CACHE-->>ORCH: step result
        end
        ORCH->>CACHE: PutStep (cache result, TTL)
        ORCH->>ORCH: PatternTracker.record(workflow)
        ORCH-->>CACHE: final result
    end

    CACHE-->>POOL: result + stats
    POOL-->>CHAT: ExecuteStream chunks
    CHAT-->>MCP: response
    MCP-->>UI: result
```

---

## 6. Port & Service Summary

| Port | Crate | Services | Transport |
|------|-------|----------|-----------|
| `[::1]:50051` | op-cache | AgentService, CacheService, OrchestratorService | gRPC |
| `[::1]:50051` | op-mcp (default) | McpService (compact/agents/full modes) | gRPC |
| `0.0.0.0:50052` | op-chat | Chat MCP Server | gRPC |
| `50051-50060` | op-chat pool | Per-agent connections (rust_pro, backend_architect, seq_thinking, memory, ctx_mgr, python_pro, debugger, mem0, search, deploy) | gRPC |
| system bus | op-dbus-mirror | MirrorV1, ProjectedObjectV1, OvsdbV1, NonNetV1 | D-Bus |
| system bus | op-state | StateManager, PluginV1 (per plugin) | D-Bus |
| system bus | op-agents | Agent interface (per agent type) | D-Bus |
| system bus | op-services | services.v1.Manager | D-Bus |
| (bridge) | op-grpc-bridge | StateSync, PluginService, EventChainService, OvsdbMirror, RuntimeMirror, ComponentRegistry | gRPC ↔ D-Bus |

---

## 7. D-Bus Object Path Hierarchy

```
/org/opdbus/
├── state                              ← StateManager interface
└── v1/                                ← MirrorV1 interface
    ├── ovsdb/                         ← OvsdbV1 JSON-RPC interface
    │   ├── Bridge/{uuid}              ← ProjectedObjectV1
    │   ├── Port/{uuid}               ← ProjectedObjectV1
    │   ├── Interface/{uuid}           ← ProjectedObjectV1
    │   └── ...per OVSDB table
    ├── nonnet/                        ← NonNetV1 JSON-RPC interface
    │   └── {db_name}/{table}/{uuid}   ← ProjectedObjectV1
    ├── state/{entity_id}              ← ProjectedObjectV1 (enterprise)
    └── plugins/
        ├── dinit/                     ← PluginV1
        ├── ovsdb_bridge/             ← PluginV1
        ├── privacy_router/           ← PluginV1
        ├── hardware/                 ← PluginV1
        └── ...30+ plugins

/org/dbusmcp/Agent/
├── PythonPro                          ← Agent interface
├── RustPro                            ← Agent interface
├── Debugger                           ← Agent interface
└── ...per agent type

/org/opdbus/services                   ← services.v1.Manager interface
```
