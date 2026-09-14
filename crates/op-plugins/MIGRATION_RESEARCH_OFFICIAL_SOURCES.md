# Official Sources for Structured Data — Plugin Schema Uniformization Mission

**Critical Rule (from architecture.md):**
> For hand-rolled plugins with weak schemas (using `serde_json::Value` for everything, or missing typed fields), workers **MUST** research official sources to get structured data for proper typed Rust structs. This is NOT optional.

All `gb_*.rs` migrations **must** derive concrete `struct`s from official documentation instead of blindly porting loose `Value` / `HashMap` fields.

This document contains researched official sources + key structured types for the current batch of migrators.

## General Instructions for Migrators
- Use `web_search`, `open_page`, and `open_page_with_find` (or read local man pages / specs).
- Prefer RFCs, official man pages, freedesktop.org, project homepages, and `.ovsschema` / introspection XML.
- Translate official fields into:
  - Rust structs + `#[derive(schemars::JsonSchema)]`
  - Proper `x-oscal-subid` annotations
  - Defaults where the official source implies them
- Keep `serde_json::Value` **only** for truly opaque extension points (e.g., `other_config` maps that are intentionally free-form).
- After building the schema with `plugin_schema_from_json`, call `super::common::oscal::ensure_category_metadata_fields(&mut schema)` for any `mut.*` / `src.*` subids.

---

## 1. snowball (snowball_plugin)

**Nature:** Internal `op-snowball` crate (`StreamingSnowball`).

**Official / Authoritative Sources (in-repo):**
- `crates/op-snowball/src/streaming_snowball.rs`
- `crates/op-snowball/src/retention.rs`
- `crates/op-snowball/src/snapshot.rs`

**Key Structured Types (use these, do not invent):**

```rust
pub enum SnapshotInterval { /* PerOperation, EveryMinute, ... Hourly, Daily ... */ }

pub struct RetentionPolicy {
    pub hourly: usize,
    pub daily: usize,
    pub weekly: usize,
    pub quarterly: usize,
}

pub struct BlockEvent { /* timestamp, category, action, data, hash, vector */ }

pub struct SnapshotEntry { name, created, ... }
```

Current weak points in `snowball_plugin.rs`:
- `snapshot_interval: String` (should become the enum)
- `retention: RetentionView` (align exactly with `RetentionPolicy`)
- `current_state: Value` (opaque is acceptable here, but document it)
- `mut.service.snowball.snapshot-interval` is mut.* → **must** expose `actor_id` + `capability_id` via `ensure...`

**Migration Notes:**
- Make `SnowballState` use the real types from `op_snowball`.
- The "official" structure for this plugin comes from the op-snowball crate, not an external standard.

---

## 2. wireguard

**Official Sources:**
- https://www.wireguard.com/quickstart/
- https://man7.org/linux/man-pages/man8/wg-quick.8.html
- https://git.zx2c4.com/wireguard-tools/about/src/man/wg.8
- wg(8) and wg-quick(8) man pages

**Key Structured Fields (from wg-quick + wg config):**

**Interface section:**
- PrivateKey (base64)
- ListenPort (u16)
- Address (list of CIDR strings)
- DNS (list of IPs)
- MTU (u32)
- Table (string or "off" / "auto")
- PreUp / PostUp / PreDown / PostDown (scripts, list or single)
- SaveConfig (bool)

**Peer section (repeated):**
- PublicKey
- PresharedKey (optional)
- AllowedIPs (list of CIDR)
- Endpoint (host:port)
- PersistentKeepalive (u16 seconds)
- (optional) Description / name in some UIs

Current `wireguard.rs` already has decent `WireGuardInterface` + `WireGuardPeer`. Improve by:
- Aligning field names/types exactly with wg config.
- Adding `MTU`, `DNS`, `Table`, `PresharedKey`, script hooks.
- Using `NetworkInterface` common blob where appropriate.
- Subids should be under `network` component (already mostly correct).

---

## 3. ovsdb_bridge

**Official Sources:**
- RFC 7047: https://datatracker.ietf.org/doc/html/rfc7047 (OVSDB protocol + JSON schema)
- https://docs.openvswitch.org/en/stable/ref/ovsdb.7/
- The live schema: `ovsdb-tool list-dbs` + `ovsdb-client get-schema Open_vSwitch`
- man ovs-vswitchd.conf.db (the actual table definitions)

**Core Tables (typed structs recommended):**
- Open_vSwitch (root)
- Bridge (name, ports, datapath_type, fail_mode, other_config map, external_ids)
- Port (name, interfaces, tag, trunks, vlan_mode, other_config)
- Interface (name, type, options map, ofport, mac_in_use, error, other_config)
- Flow_Table, QoS, Queue, etc. (as needed)

Current file already uses `OvsdbDbusClient` and has transact/monitor methods that take loose JSON (acceptable for the generic OVSDB protocol surface).

For the **state** projection:
- Mirror the key tables as typed structs (use `HashMap<String, Value>` only for `other_config` / `external_ids` / `options`).
- Methods can keep generic `Vec<serde_json::Value>` for operations because OVSDB transact is intentionally a JSON-RPC array of ops.

**Research Action:** Run `ovsdb-client get-schema Open_vSwitch` in the environment for the authoritative live schema and base structs on that.

---

## 4. openflow

**Official Source:**
- OpenFlow Switch Specification v1.5.1 (PDF): https://opennetworking.org/wp-content/uploads/2014/10/openflow-switch-v1.5.1.pdf

**Key Structured Concepts:**
- Flow tables (multiple)
- Match fields (many standard + experimenter)
- Instructions / Actions (set_field, push/pop, output, group, meter, etc.)
- Group table (types: all, select, indirect, fast_failover)
- Meter table
- Ports (physical, logical, reserved)

For the plugin (from current code):
- It exposes generic `TransactInput` etc. over OVSDB/OpenFlow southbound.
- For state, prefer concrete representations of bridges/ports/flows when possible.
- Many "operations" will legitimately stay as `Vec<Value>` because they are protocol messages.

Use the spec to name typed action / match structs where the plugin surfaces higher-level convenience methods.

---

## 5. login1

**Official Source:**
- https://www.freedesktop.org/software/systemd/man/org.freedesktop.login1.html
- man org.freedesktop.login1(5)

**Key Objects & Structures:**
- Manager: ListSessions, ListUsers, ListSeats, GetSession, etc.
- Session: properties like Id, User, Seat, State (active/online/closing), Leader, Remote, etc.
- User: UID, Name, Sessions list, IdleHint, etc.
- Seat: Id, ActiveSession, Sessions list, CanMultiSession.

Define:
```rust
pub struct LoginSession { id, user, seat, state, leader, remote, ... }
pub struct LoginUser { uid, name, sessions, idle_hint, ... }
pub struct LoginSeat { id, active_session, sessions, ... }
```

This plugin should expose typed views + methods like `activate_session`, `terminate_session`, etc.

---

## 6. cozo (cozo)

**Official Source:**
- https://docs.cozodb.org/
- https://github.com/cozodb/cozo
- Datalog tutorial + manual

**Data Model (structured):**
- Relations (named tables) with typed columns (Int, Float, Bool, String, List, Json, etc.)
- Rules (Datalog)
- Queries return relations (rows of values)
- Time travel (historical versions)
- Vector search integrated
- Storage backends (mem, sqlite, rocksdb)

For the plugin state, typical useful structured view:
- List of named relations + their arity / column schemas
- Recent queries / rules
- Current storage backend info

Methods will likely take Datalog strings (keep as String + perhaps parsed when possible), but results should be returned as typed rows where feasible.

---

## 7. dnsresolver

**Official Source:**
- https://www.freedesktop.org/software/systemd/man/org.freedesktop.resolve1.html
- man org.freedesktop.resolve1(5)

**Key Structures:**
- Manager interface
- ResolveHostname, ResolveAddress, ResolveRecord, etc. returning typed arrays of:
  - (ifindex, family, address) or (name, family, address, ...)

---

## Next Batch (Milestone 2/3 continuation)

### 8. users

**Official Source:**
- AccountsService / org.freedesktop.Accounts : https://www.freedesktop.org/wiki/Software/AccountsService/
- D-Bus interface XML and man pages for org.freedesktop.Accounts.User

**Key Structured Properties (org.freedesktop.Accounts.User):**
- UserName (s)
- RealName (s)
- HomeDirectory (s)
- Shell (s)
- Uid (u)
- Gid (u)
- Groups (as)
- Locked (b)
- PasswordHint (s)
- etc.

From current code, the weak schema uses UserConfig with username, uid, gid, groups, shell, present.

**Recommendation for gb_users.rs:**
Use concrete fields from AccountsService. Add methods like create_user, delete_user using the structured inputs.

### 9. s6_systemctl / s6

**Official Sources:**
- https://skarnet.org/software/s6/
- s6-svstat: https://skarnet.org/software/s6/s6-svstat.html
- s6-supervise, s6-rc documentation

**Key Structured State (from s6-svstat):**
- up (bool)
- wantedup (bool)
- normallyup (bool)
- ready (bool)
- paused (bool)
- pid (i64 or none)
- pgid
- exitcode / signal / signum
- updownsince, readysince, updownfor, readyfor (timestamps / durations)

Service directory layout: 
- run, down, supervise/ (pid, stat, etc.)

s6-rc adds compiled database for dependencies.

### 10. keyring

**Official Source:**
- Secret Service API: https://specifications.freedesktop.org/secret-service/latest/
- https://specifications.freedesktop.org/secret-service/latest/ch03.html (Items and Collections)

**Key Structured Objects:**
- Collection: Label (s), Locked (b), Created (t), Modified (t), Items (ao)
- Item: Label (s), Attributes (a{ss}), Secret (struct(ay, ay, s, ay)), Locked (b), Created (t), Modified (t)
- Service: Collections (ao), Aliases, etc.
- Prompt objects for unlock.

Use typed structs for Item and Collection. Attributes are map<string,string>.

### 11. packagekit

**Official Source:**
- https://www.freedesktop.org/software/PackageKit/gtk-doc/api-reference.html
- Transaction interface, Package structure

**Key Structured Data:**
- Package ID format: "name;version;arch;repository"
- Package details: summary, description, license, size, etc.
- Transaction status: percentage, status enum (downloading, installing...), remaining time
- Categories, Groups (enum-like)

Methods return arrays of structured packages.

### 12. pcidecl

**Official Sources:**
- PCI Local Bus Specification (pcisig.com)
- Linux: /sys/bus/pci/devices/ structure, lspci output
- PCI ID database: https://pci-ids.ucw.cz/

**Key Structured Fields for PCI devices:**
- Domain:Bus:Slot.Function (string or tuple)
- Vendor ID (u16), Device ID (u16)
- Class code, Subclass
- Subsystem Vendor/Device
- Revision
- Resources (BARs, memory, I/O)
- Driver bound (from sysfs)

Current plugins often dump lspci-like text or sysfs raw. Define typed PciDevice struct.

### 13. service / s6

**See s6 section above.** Overlaps with s6_systemctl.

**Structured Service State:**
- Name
- State (up/down/starting/stopping)
- PID
- Uptime
- Log location
- Dependencies (from s6-rc if used)

Prefer s6 specific state over generic.

### 14. systemd / systemd_networkd (bonus)

**Official:**
- org.freedesktop.systemd1 D-Bus (freedesktop.org)
- systemd.unit(5), properties like ActiveState, SubState, LoadState, etc.

**Key for systemd units:**
- Id, Description
- LoadState, ActiveState, SubState
- MainPID, ExecMainStartTimestamp
- etc.

Use for service-like plugins.

---

**Action for next batch of 7 migrators:**
Assign the above (users, s6_systemctl, keyring, packagekit, pcidecl, service, s6).

Instruct agents to read this document + run targeted `web_search` / `open_page` for the exact plugin + call `ovsdb-client` or `busctl introspect` where applicable for live schema.

Always prefer official spec fields over current hand-rolled Value blobs.- Link-specific: SetLinkDNS, SetLinkDomains, SetLinkDNSOverTLS, etc.
  - DNS servers as list of (address, port?, family?)
  - Domains / routing domains

Define clean structs:
```rust
pub struct ResolvedAddress { family: i32, address: Vec<u8>, ... }
pub struct LinkDNS { servers: Vec<DnsServer>, domains: Vec<String>, ... }
```

---

## How to Use This Document
1. Each migrator agent: read this file **before** designing the state structs.
2. For any field that was `serde_json::Value` in the old plugin, find the corresponding official definition here or in the linked spec and make a concrete type.
3. Add a comment in the gb_ file citing the source, e.g.:
   ```rust
   /// From wg-quick(8) man page + https://www.wireguard.com/quickstart/
   pub struct WireGuardInterface { ... }
   ```

Last updated: 2026-07-03 (during execution of the 7 parallel migrators)

**Reminder to agents:** If the official source uses maps for extensibility (e.g. `other_config`, `options`), keep a `HashMap<String, String>` or `Value` for those, but give first-class fields to the well-known structured ones.

---

## 31. btrfs_plugin (gb_btrfs_plugin)

**Nature:** Btrfs storage management plugin for subvolumes, snapshots, scrub, balance, devices, df/usage (infrastructure/filesystem layer; uses D-Bus only, no direct CLI).

**Official / Authoritative Sources (researched 2026-07-03):**
- btrfs.readthedocs.io: https://btrfs.readthedocs.io/en/latest/btrfs-subvolume.html , btrfs-scrub.html, btrfs-balance.html, btrfs-filesystem.html, btrfs-device.html, Subvolumes.html
- btrfs-ioctl(2): https://btrfs.readthedocs.io/en/latest/btrfs-ioctl.html + kernel uapi/linux/btrfs.h (btrfs_ioctl_vol_args_v2, btrfs_ioctl_scrub_args + btrfs_scrub_progress, btrfs_ioctl_balance_args + btrfs_balance_progress, btrfs_ioctl_dev_info_args, btrfs_ioctl_get_subvol_info_args)
- man pages: btrfs-subvolume(8), btrfs-scrub(8), btrfs(8), btrfs-filesystem(8)
- Cross-ref: op-snowball/src/streaming_snowball.rs RetentionPolicy (for snapshot retention in config)
- XAI model assist (via https://api.x.ai/v1 with XAI_API_KEY): summarized exact output formats + ioctl fields for structs.

**Key Structured Output Formats (use these for typed state; no invention):**

- `btrfs subvolume list` (default): ID <id> gen <gen> [cgen <cgen>] parent <pid> top level <tlid> [uuid <u>] [parent_uuid <pu>] [received_uuid <ru>] path <path>
- `btrfs subvolume show`: Name, UUID, Parent UUID, Received UUID, Creation time, Subvolume ID, Generation, Gen at creation, Parent ID, Top level ID, Flags (ro), Snapshot(s)
- `btrfs scrub status`: UUID, Scrub started, Status: running/finished, Duration, Time left, ETA, Total to scrub, Bytes scrubbed, Rate, Error summary: csum=N Corrected=N Uncorrectable=N ...
- `btrfs filesystem df`: Data,<profile>: total=.. used=.. ; Metadata,<p>: ... ; System,<p>: ... ; GlobalReserve,single: ...
- `btrfs filesystem usage`: Device size/allocated/unallocated/used/free, per-type Size/Used, Unallocated per dev.
- `btrfs balance status`: Balance ... is running/finished , Started, Status: ... chunks balanced (pct%)
- Retention in config aligns with op-snowball: hourly/daily/weekly/quarterly counts (for snapshot_schedule/retention).

**Key ioctl structs (for modeling input effects in state):**
- subvol/snap: btrfs_ioctl_vol_args_v2 { fd, transid, flags (BTRFS_SUBVOL_RDONLY etc), name or subvolid }
- scrub progress: data_extents_scrubbed, bytes_scrubbed, read_errors, csum_errors, corrected_errors, uncorrectable_errors, ...
- balance: flags, state (RUNNING etc), stat {expected, considered, completed}, per-type btrfs_balance_args
- dev: devid, uuid, bytes_used, total_bytes, path

**Typed structs to use in gb_btrfs_plugin (with full x-oscal-subid):**

```rust
pub struct BtrfsSubvolume { /* id, name/path, uuid, parent_id, readonly, gen, cgen, ... */ }
pub struct BtrfsSnapshot { /* similar + source, created, send_status, readonly */ }
pub struct BtrfsScrubStatus { uuid, status, started, duration, bytes_scrubbed, total, rate, errors: BtrfsScrubErrors {csum, corrected, uncorrectable,...} }
pub struct BtrfsBalanceStatus { running: bool, started, completed_chunks, total_chunks, ... }
pub struct BtrfsSpaceInfo { profile: String, total, used } // for df
pub struct BtrfsDevice { devid, path, size, used, ... }
```

**Migration Notes for gb_btrfs:**
- State: use PluginMetadata (flatten from common_schema_fields) + typed vecs for subvolumes/snapshots (leave send/dr/config as structured objects; avoid loose top-level Value except for opaque).
- Component-type: use "software" or "this-system" for btrfs plugin subids (per 7 allowed; old used invalid "storage" as component-type -> fix to "software.plugin.btrfs" or "this-system.storage.btrfs" but stick to listed: software/this-system).
- Retention: embed or reference RetentionPolicy shape from op-snowball where config.retention present.
- Methods: keep the 12, but typed inputs/outputs, use common AckOutput etc.
- MUST call plugin_schema_from_json + apply_state_defaults + ensure_category_metadata_fields (for mut.* actor_id/capability_id).
- Use inventory submit with PLUGIN_NAME.
- Drift + all_subids_are_valid tests.
- Subids e.g. obs.software.plugin.btrfs.subvolumes@v1 , mut.software.plugin.btrfs.subvolume.create@v1 (fix from old "mut.storage..." and "obs.software.plugin.btrfs.status@v1" etc. to consistent).
- Cite sources in code comments + this doc extension.

**Use of common_schema_fields:** Embed `#[serde(flatten)] pub metadata: PluginMetadata` in BtrfsState for uniform running/healthy/version etc. Subids injected via ensure + inject if present.

XAI_API_KEY used for model call to obtain precise field mappings from man/ioctl before writing structs.

---


---

## 15. agent_config

**Nature:** Agent enablement, model overrides, and per-agent tool allow-list configuration. Single source for which agents from the op-agents catalog are active in a given context (UI forms, MCP exposure, cognitive sessions). Used as dependency by mcp and cognitive_mcp.

**Official / Authoritative Sources (researched via code + project specs 2026-07-03):**
- crates/op-agents/src/agent_catalog.rs + lib.rs: `AgentDescriptor { agent_type: String, name: String, description: String, operations: Vec<String> }` and `builtin_agent_descriptors()`. This is the authoritative list of known agents (agent_type acts as "name").
- crates/op-agents/src/unified/agent_trait.rs: `UnifiedAgent` trait (id, name, description, category: AgentCategory, capabilities: HashSet<AgentCapability>), `AgentRequest`, `AgentResponse`, `AgentCapability` enum. Categories: Execution/Persona/Orchestration.
- crates/op-gemma/src/ui_gallery.rs `gen_agent_config`: renders AgentConfigForm example with `agentName`, `provider` ("anthropic"|"openai"), `model` (e.g. "claude-sonnet-4-20250514"|"gpt-4o-mini"), `status` ("idle"|"running"|"error"|"stopped"), `tools: Vec<String>`.
- crates/op-plugins/src/state_plugins/agent_config.rs (legacy): `AgentConfigState { agents: Vec<AgentConfig> }`; `AgentConfig { name, enabled, model: Option<String>, tools: Vec<String> }`. Methods: get_config/update_config/list_tools/register_tool/reset_config.
- mcp.rs (dependency): mcp lists "agent_config" as dep; cognitive_mcp uses for agent tool registration surface.
- Project specs: DESIGN.md (agents array untyped weakness), docs/plugins/plugin-catalog.md ("agent tool/model configuration"), PLUGIN-METHOD-SPEC.md, FACTORY-PROMPT-plugin-schema-uniformization.md, schema-from-structs.md.
- No external RFC/man for this (internal control plane); typed from op-agents catalog + UI form contracts.

**Key Structured Types (use for typed Rust structs — replace Any/Value where possible):**

```rust
// From op-agents catalog + UI form + legacy (augmented for provider/status from gemma ui)
pub struct AgentConfig {
    pub name: String,           // matches agent_type / agentName
    pub enabled: bool,
    pub provider: Option<String>, // researched: anthropic/openai/ollama etc.
    pub model: Option<String>,    // override e.g. "gemma:2b", "gpt-4o-mini"
    pub tools: Vec<String>,       // enabled tool names (subset of operations + global)
    // optional future: status, capabilities
}

pub struct AgentConfigState {
    // flatten PluginMetadata + AIModelConfig patterns for uniform
    pub agents: Vec<AgentConfig>,
}
```

**Migration Notes (for gb_agent_config.rs):**
- State uses `#[serde(flatten)] PluginMetadata` + specific agents vec (typed AgentConfig).
- All fields + I/O structs + methods get `#[schemars(extend("x-oscal-subid" = "..."))]` .
- Subids: use subject "agent-config", component "software" or "service" (align gb_* : "software.plugin.agent-config" for state, "service.plugin..." allowed for method surfaces).
- Methods preserve names for D-Bus stability: get_config (Read), update_config (Mutation), list_tools (Read), register_tool (Mutation), reset_config (Mutation, idempotent).
- After plugin_schema_from_json + apply_state_defaults: call `ensure_category_metadata_fields` (for mut.*) + `inject_common_subids(&mut schema, "agent-config", &["metadata"])` .
- Drift guard golden + `all_subids_are_valid` using collect + validate_subid.
- No direct FS/DB reads (D-Bus first spirit); current impl is schema projection.
- Research used local `grep`/`read_file` on official crates + specs (XAI key exported, web_search fallback not needed as authoritative is in-tree op-agents + ui contracts).
- Use common_schema_fields via flatten; SideEffect::Read/Mutation + idempotency flags.

**Example subids (strict AGENTS.md §4a):**
- sch.software.plugin.agent-config.schema@v1
- obs.software.plugin.agent-config.agents@v1
- mut.software.plugin.agent-config.agent.enabled@v1
- obs.service.plugin.agent-config.config.get@v1  (for method)
- mut.service.plugin.agent-config.config.update@v1
- All mut.* must get actor_id/capability_id injected.

This completes the official structured data for agent_config uniformization.

---

## systemd

**Official Sources:**
- https://www.freedesktop.org/wiki/Software/systemd/dbus/ (The D-Bus API of systemd)
- https://www.freedesktop.org/software/systemd/man/org.freedesktop.systemd1.html
- man org.freedesktop.systemd1(5)
- Manager interface on /org/freedesktop/systemd1

**Key Structured Types (Manager + Unit):**
- Unit: Id (s), Description, LoadState, ActiveState, SubState, FollowUnit, etc.
- Methods: StartUnit(name, mode), StopUnit, RestartUnit, ReloadUnit, KillUnit, SetUnitProperties(name, runtime, properties a(sv))
- Properties via Get on unit objects: MainPID (for services), etc.
- ListUnits, ListUnitFiles return arrays of structs.

Use typed:
```rust
pub struct SystemdUnit {
    pub id: String,
    pub description: String,
    pub load_state: String,
    pub active_state: String,
    pub sub_state: String,
    // ...
}
```

mut.* (e.g. mut.service.systemd.unit.start) require actor_id + capability_id.

---

## packagekit

**Official Sources:**
- https://www.freedesktop.org/software/PackageKit/gtk-doc/PackageKit.html
- Transaction interface: https://www.freedesktop.org/software/PackageKit/gtk-doc/Transaction.html
- org.freedesktop.PackageKit.xml in upstream

**Key Structured:**
- CreateTransaction -> object path
- Transaction props: Percentage (u), Status, Sender, etc.
- Methods: InstallPackages, UpdatePackages, RemovePackages, Search, Resolve, GetPackages, etc. (with filters, transaction flags).

For state:
- Current transactions list with progress.
- Package info structs: id, summary, version, etc.

Prefer typed Package, TransactionProgress over raw Value.

---

## incus / incus_device (from Incus REST + API)

**Official:**
- https://linuxcontainers.org/incus/docs/main/api/
- Instance struct, Device map (disk, nic, unix-block, gpu, usb, etc with config).

Key: Instance { name, status, type, devices: HashMap<String, DeviceConfig>, ... }

DeviceConfig { type: "disk" | "nic" | ..., config: map for path/source/etc }

Use in gb_incus and gb_incus_device.

## Additional for hardware, rtnetlink, net etc.

See Linux sysfs (/sys/class/net, /sys/bus/pci), rtnetlink(7), ip-link(8) for interface/address structs.

Prioritize struct over Value for listed fields.
