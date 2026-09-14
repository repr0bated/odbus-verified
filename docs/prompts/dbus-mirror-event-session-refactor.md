# Prompt: Refactor op-dbus-mirror to Event/Session-Driven Architecture

You are a Senior Rust Systems Architect. Refactor `crates/op-dbus-mirror` from its
current poll-and-snapshot model to a fully event-driven, session-scoped architecture.

## Data sources (authoritative — reflect what the code actually uses)

- **rovs suite** (`rovs_ovsdb`, `rovs_types`, `rovs_transport`) — the live OVSDB
  IDL replica. `OvsdbClient` wraps this and already exposes:
  - `idl()` — synchronous read of the in-memory IDL replica (no RPC)
  - `monitor_db(db: &str) -> Result<mpsc::Receiver<serde_json::Value>>` — fires on
    every IDL update with reconnect/backoff. This is the event feed to wire in.
  - `commit(tx: Transaction)` — write path
  There is NO separate OvsdbClient to replace — `OvsdbClient` IS the rovs wrapper.
  Do not introduce a new OVSDB abstraction.

- **NonNetDb** (`op_jsonrpc::nonnet::NonNetDb`) — internal key-value store.
  Currently polled; needs a `watch()` change feed added.

- **procfs** (`/proc/meminfo`, `/proc/cpuinfo`, `/proc/loadavg`, and the stub
  sections: stat, vmstat, diskstats, mounts, version, uptime`) — currently read
  on every 30-second tick via blocking `tokio::fs::read_to_string`. These need
  inotify-based or timed event sources:
  - Use `inotify` (via the `inotify` crate) on `/proc/meminfo` and `/proc/stat`
    — Linux notifies on reads of these pseudo-files when content changes.
  - For `/proc/loadavg` use a 5-second `tokio::time::interval` (it changes
    frequently but has no inotify support).
  - For static/slow files (`cpuinfo`, `version`, `mounts`) read once at startup
    and re-read only on `SIGHUP` or an explicit `Refresh` D-Bus call.
  - Expose procfs data via the `procfs` crate (add as dependency) rather than
    hand-parsing `/proc` text files. Replace `gather_meminfo`, `gather_cpuinfo`,
    `gather_loadavg` with typed reads from `procfs::Meminfo`, `procfs::CpuInfo`,
    `procfs::LoadAverage`.

- **ComponentRegistry** (via `grpc_server.registry_watch()`) — already event-driven
  via `apply_registry_event`. Keep this path unchanged.

- **StateManager / plugin snapshot** — currently polled every 30 seconds.
  Add a `watch() -> broadcast::Receiver<PluginEvent>` to `StateManager` and wire it.

## Current state (patterns to eliminate)

- `tokio::time::interval(Duration::from_secs(30))` background loop calling
  `refresh_full_tree()` unconditionally — DELETE this entirely.
- `publish_host_snapshot`, `publish_ovsdb_snapshot`, `publish_nonnet_snapshot`,
  `publish_plugin_snapshot` — full-table scan functions called every tick — DELETE.
- Hand-parsed `/proc` text files in `gather_meminfo`, `gather_cpuinfo`,
  `gather_loadavg` — REPLACE with `procfs` crate.
- `simd_json::json!` and `simd_json::OwnedValue` throughout — REPLACE with
  `serde_json::json!` and `serde_json::Value`.

## Target architecture

### 1. Session layer

```rust
pub struct MirrorSession {
    pub peer: zbus::names::UniqueName<'static>,
    pub subscribed_paths: HashSet<String>,
    pub last_seq: HashMap<String, u64>,   // path → last acknowledged sequence
}
```

- Created when a peer calls any method or subscribes to a signal.
- Destroyed when `org.freedesktop.DBus.NameOwnerChanged` signals the peer is gone.
- Stored in `Arc<DashMap<String, MirrorSession>>` keyed by peer name string.
- If a session's pending event queue exceeds 500 events, emit `InterfacesRemoved`
  for all its subscribed paths and drop the session so it knows to reconnect.

### 2. Unified event enum

```rust
#[derive(Debug, Clone)]
pub enum MirrorEvent {
    OvsdbRow    { table: String, uuid: String,     delta: serde_json::Value },
    NonNet      { key: String,                     delta: serde_json::Value },
    Plugin      { plugin_id: String,               delta: serde_json::Value },
    Registry    { event: op_grpc_bridge::proto::registry::RegistryEvent },
    ProcMem     { delta: serde_json::Value },
    ProcLoad    { delta: serde_json::Value },
    ProcStatic  { section: String, data: serde_json::Value },
}
```

Single `broadcast::Sender<MirrorEvent>` in `DbusMirror`; each source task
publishes to it; a single dispatch loop applies deltas.

### 3. OVSDB event feed

Wire `OvsdbClient::monitor_db("Open_vSwitch")` into the event bus:

```rust
let mut rx = self.ovsdb.monitor_db("Open_vSwitch").await?;
tokio::spawn(async move {
    while let Some(update) = rx.recv().await {
        // parse rovs IDL update → MirrorEvent::OvsdbRow{table, uuid, delta}
        let _ = tx.send(MirrorEvent::OvsdbRow { ... });
    }
});
```

Use `OvsdbClient::idl()` for the startup snapshot only. Do not call
`dump_db()` on the polling path.

### 4. NonNetDb change feed

Add to `NonNetDb`:

```rust
pub fn watch(&self) -> broadcast::Receiver<NonNetChanged>
```

Fire from every write path (insert/update/delete). Internally hold a
`broadcast::Sender<NonNetChanged>` in the struct.

### 5. procfs event feeds

Add dependencies to `crates/op-dbus-mirror/Cargo.toml`:
- `procfs = "0.17"`
- `inotify = "0.10"`

```rust
// Meminfo — inotify on /proc/meminfo
let mut inotify = Inotify::init()?;
inotify.watches().add("/proc/meminfo", WatchMask::ACCESS)?;
// read procfs::Meminfo on each notification, emit MirrorEvent::ProcMem

// Loadavg — 5-second interval
let mut interval = tokio::time::interval(Duration::from_secs(5));
// read procfs::LoadAverage on each tick, emit MirrorEvent::ProcLoad

// Static: cpuinfo, version, mounts — read once at startup
// re-read on SIGHUP (tokio::signal::unix::signal(SignalKind::hangup()))
```

Replace hand-parsed gather functions with typed procfs reads:

```rust
fn meminfo_to_json() -> serde_json::Value {
    let m = procfs::Meminfo::new().unwrap_or_default();
    serde_json::json!({
        "MemTotal": m.mem_total,
        "MemFree":  m.mem_free,
        "MemAvailable": m.mem_available,
        "SwapTotal": m.swap_total,
        "SwapFree":  m.swap_free,
    })
}
```

### 6. StateManager plugin feed

Add to `StateManager`:

```rust
pub fn watch(&self) -> broadcast::Receiver<PluginEvent>
```

Fire from every register/deregister path.

### 7. Delta-only publication

`publish_object` already compares old vs new value. Extend it to:
- Track changed fields only and emit `PropertiesChanged` only for those fields.
- Increment a `sequence: u64` counter stored per object path in a
  `DashMap<String, (serde_json::Value, u64)>`.

### 8. Revised `start()` — event loop replaces poll loop

```rust
pub async fn start(self: Arc<Self>) -> Result<()> {
    self.publish_startup_snapshot().await?;   // one-shot, replaces refresh_full_tree
    self.register_dbus_objects().await?;       // same as today
    self.spawn_event_sources().await?;         // wires all feeds → broadcast tx
    self.run_event_loop().await                // dispatch loop, never returns
}
```

`run_event_loop` receives from `broadcast::Receiver<MirrorEvent>` and calls
`publish_object` for each delta. No `interval` tick inside this loop.

Heartbeat safety net — a separate task fires every 5 minutes and re-syncs only
objects whose sequence number has not advanced in that window:

```rust
let mut heartbeat = tokio::time::interval(Duration::from_secs(300));
loop {
    heartbeat.tick().await;
    self.resync_stale_objects(Duration::from_secs(300)).await;
}
```

## Constraints

- All existing D-Bus object paths (`/org/opdbus/v1/...`) and interface names unchanged.
- `GetManagedObjects`, `InterfacesAdded`, `InterfacesRemoved` must work correctly.
- `DbusMirrorInterface::Refresh` triggers resync of a single named path only.
- **zbus 5.12** — upgrade `crates/op-dbus-mirror/Cargo.toml` from `4.0` to `5.12`
  (matching `crates/op-identity/Cargo.toml` which is already on 5.12). In zbus 5.x
  signals are emitted via interface proxy methods rather than `SignalContext`; update
  all `InterfacesAdded`, `InterfacesRemoved`, `PropertiesChanged` emission sites
  accordingly. Reference `crates/op-identity/src/lib.rs` for the established 5.x
  usage pattern in this workspace. Also upgrade the workspace `Cargo.toml` pin from
  `4.0` to `5.12` so all crates stay consistent.
- No `unsafe` blocks. No `simd_json` — use `serde_json` throughout.
- All new public types `derive(Debug)`.
- Do not add features beyond what is listed above.

## Deliverables

1. Revised `crates/op-dbus-mirror/src/lib.rs` — `MirrorSession`, `MirrorEvent`,
   `start()`, `spawn_event_sources()`, `run_event_loop()`, `publish_startup_snapshot()`
2. Revised procfs helpers replacing `gather_meminfo` / `gather_cpuinfo` / `gather_loadavg`
3. `NonNetDb::watch()` stub
4. `StateManager::watch()` stub
5. Updated `crates/op-dbus-mirror/Cargo.toml` adding `procfs = "0.17"`,
   `inotify = "0.10"`, and `zbus = { version = "5.12", features = ["tokio"] }`
6. Architecture note ≤ 20 lines explaining session lifecycle and event source map

Cite every changed line as file:line. Do not add features beyond what is listed.
