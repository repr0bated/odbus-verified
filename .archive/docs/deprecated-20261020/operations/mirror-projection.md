# D-Bus Mirror Projection & gRPC Bridge

## Architecture

```
                    ┌─────────────────────────┐
                    │      ovsdb-server        │ ← Source of truth (RFC 7047)
                    │    /run/openvswitch/     │
                    └───────────┬─────────────┘
                                │ JSON-RPC (unix socket)
                                ▼
┌──────────────────────────────────────────────────────────┐
│                     op-dbus-mirror                        │
│                                                          │
│  reconcile() ─┬─ mirror_ovsdb()      → /org/opdbus/v1/ovsdb/{table}/{uuid}
│               ├─ mirror_nonnet()     → /org/opdbus/v1/nonnet/{db}/{table}/{uuid}
│               └─ mirror_enterprise() → /org/opdbus/v1/... (from state.db)
│                                                          │
│  Live updates:                                           │
│    • OVSDB monitor(Open_vSwitch) → PropertyChanged       │
│    • NonNet broadcast::Receiver  → PropertyChanged       │
│    • 60s periodic full reconcile                         │
└──────────────┬───────────────────────────────────────────┘
               │ D-Bus (org.opdbus.v1)
               ▼
┌──────────────────────────────────────────────────────────┐
│                    op-grpc-bridge                         │
│                                                          │
│  Services:                                               │
│    • StateSync          (generic state sync)             │
│    • PluginService      (plugin schema/methods)          │
│    • EventChainService  (audit trail + Merkle proofs)    │
│    • OvsdbMirror        (RFC 7047 native gRPC)           │
│                                                          │
│  DbusWatcher monitors:                                   │
│    • /org/operation/*                                    │
│    • /org/opdbus/v1/*                                    │
│    • /org/opdbus/v1/ovsdb/*                              │
│    • /org/opdbus/v1/nonnet/*                             │
└──────────────────────────────────────────────────────────┘
```

## D-Bus Object Tree

Every object implements `org.opdbus.ProjectedObjectV1`:
- Property: `JsonData` (full row as JSON string)
- Method: `GetProperty(key) → value`
- Signal: `DataUpdated` (emitted on reconciliation change)

### OVSDB Subtree (`/org/opdbus/v1/ovsdb/`)

1:1 projection of Open_vSwitch database tables. Each OVSDB row becomes a
D-Bus object keyed by UUID.

```
/org/opdbus/v1/ovsdb/
├── Open_vSwitch/{uuid}     # Root config row
├── Bridge/{uuid}           # Bridge rows
├── Port/{uuid}             # Port rows
├── Interface/{uuid}        # Interface rows (RFC 7047 §3.2)
├── Controller/{uuid}       # OpenFlow controller connections
├── Manager/{uuid}          # OVSDB manager connections
├── Flow_Table/{uuid}       # Flow table config
├── QoS/{uuid}              # QoS policies
├── Queue/{uuid}            # Traffic queues
├── Mirror/{uuid}           # Port mirroring
├── NetFlow/{uuid}          # NetFlow export
├── sFlow/{uuid}            # sFlow export
├── IPFIX/{uuid}            # IPFIX export
└── SSL/{uuid}              # SSL/TLS config
```

JSON-RPC passthrough at `/org/opdbus/v1/ovsdb` via `org.opdbus.OvsdbV1`:
- `transact(operations) → results`
- `get_schema() → schema`
- `list_dbs() → databases`
- `dump_db() → full_dump`
- `create_bridge(name)`, `delete_bridge(name)`
- `add_port(bridge, port)`, `list_bridges()`, `list_ports(bridge)`

### NonNet Subtree (`/org/opdbus/v1/nonnet/`)

Plugin and application state stored in the NonNet JSON-RPC database.
Uses the same RFC 7047-style protocol (transact, get_schema, list_dbs).

```
/org/opdbus/v1/nonnet/
└── {db_name}/
    └── {table_name}/
        └── {uuid}          # Row as ProjectedObject
```

JSON-RPC passthrough at `/org/opdbus/v1/nonnet` via `org.opdbus.NonNetV1`:
- `transact(request) → response`
- `get_schema() → schema`
- `list_dbs() → databases`

### Enterprise Subtree (dynamic)

Objects from `/var/lib/op-dbus/state.db` (SQLite). Paths are stored in
the `live_objects` table. Service names from `namespace_services` are
claimed on the bus for namespace ownership.

### Control Interface (`/org/opdbus/v1`)

Root control via `org.opdbus.MirrorV1`:
- `Reconcile()` — force immediate full sync
- `GetStats() → JSON` — projection statistics
- `ListPaths() → [paths]` — all registered object paths

## Reconciliation Model

**OVSDB is the database. There is no desired-vs-current diff.**

The reconciliation loop mirrors reality into D-Bus:

1. **Initial**: `reconcile()` dumps all tables, registers D-Bus objects
2. **Live OVSDB**: `monitor_db("Open_vSwitch")` pushes row changes → update objects
3. **Live NonNet**: `NonNetDb::subscribe()` pushes updates → update objects
4. **Periodic**: Every 60s, full `reconcile()` catches missed updates

When an object already exists, `register_projected_object()` compares data
and emits `DataUpdated` signal if changed.

## gRPC Services

### StateSync (generic)

Bidirectional state sync for any D-Bus object:
- `Subscribe(filters) → stream StateChange` — watch for changes
- `Mutate(request) → response` — apply changes (via D-Bus write path)
- `GetState(plugin, path) → state` — snapshot
- `BatchMutate(requests) → responses` — transactional

### OvsdbMirror (RFC 7047 native)

gRPC projection of OVSDB management protocol:
- `ListDbs() → databases` — RFC 7047 §4.1.1
- `GetSchema(db) → schema` — RFC 7047 §4.1.2
- `Transact(db, ops) → results` — RFC 7047 §4.1.3
- `Monitor(db, tables) → stream updates` — RFC 7047 §4.1.5
- `Echo(payload) → payload` — RFC 7047 §4.1.11
- `DumpDb(db) → full_dump`
- `GetBridgeState(filter) → Bridge→Port→Interface hierarchy`

The `GetBridgeState` RPC returns the full hierarchy as typed proto messages
(`OvsdbBridge` → `OvsdbPort` → `OvsdbInterface`), not raw JSON.

### PluginService

Plugin introspection and operations:
- `ListPlugins()`, `GetSchema(plugin)`
- `CallMethod(plugin, path, interface, method)`
- `Get/SetProperty(plugin, path, interface, property)`
- `SubscribeSignals(plugin) → stream Signal`

### EventChainService

Audit trail with cryptographic verification:
- `GetEvents(range, filters)`, `SubscribeEvents(filters)`
- `VerifyChain(range) → valid/errors`
- `GetProof(event_id) → Merkle proof`
- `ProveTagImmutability(tag)`
- `Get/CreateSnapshot(plugin)`

## RFC 7047 Schema in Plugin System

The `ovsdb_bridge` plugin schema (in `schema_contract.rs`) models the full
Bridge→Port→Interface hierarchy per RFC 7047 §3.2:

```
Bridge
├── name: string (pattern: [a-zA-Z_][a-zA-Z0-9_-]*)
├── datapath_type: "" | "system" | "netdev"
├── fail_mode: "standalone" | "secure" | null
├── stp_enable: boolean
├── mcast_snooping_enable: boolean
├── other_config: map<string, string>
└── ports: Port[]
    ├── name: string
    ├── tag: integer (0-4095) | null
    ├── trunks: integer[]
    ├── vlan_mode: "native-tagged" | "native-untagged" | "access" | "trunk" | null
    ├── bond_mode: "balance-slb" | "balance-tcp" | "active-backup" | null
    └── interfaces: Interface[]
        ├── name: string
        ├── type: "" | "system" | "internal" | "patch" | "vxlan" | "gre" | "geneve" | "stt" | "lisp"
        ├── mac_in_use: string | null
        ├── mac: string | null
        ├── admin_state: "up" | "down" | null
        ├── link_state: "up" | "down" | null
        └── options: map<string, string> (tunnel: remote_ip, local_ip, key, peer)
```

## Hot-linking

D-Bus watcher → gRPC sync engine → gRPC subscribers:

1. Mirror registers/updates D-Bus objects with `ProjectedObjectV1`
2. `DbusWatcher` catches `PropertiesChanged` signals on `/org/opdbus/v1/*`
3. `SyncEngine.process_dbus_change()` records in EventChain + broadcasts
4. gRPC `Subscribe` streams pick up the `StateChange` messages

The `OvsdbMirror` gRPC service additionally provides direct JSON-RPC
passthrough to ovsdb-server via the D-Bus `OvsdbV1` interface, enabling
remote RFC 7047 operations without D-Bus client libraries.
