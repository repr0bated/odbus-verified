# op-plugins — Design: Plugin Inventory & Schema Quality Survey

> Historical survey: the schema counts and compatibility constructors below are
> obsolete. September 13 consolidation removed the duplicate built-in definitions
> and `schemas/` loaders. Current contracts belong to their plugins; runtime
> presence comes from the sealed blob catalog. See
> [current architecture](../../docs/src/architecture/overview.md) and
> [tched_router contract ownership](../../docs/src/architecture/tched-router-baseline.md).

**Crate**: `op-plugins`  
**Scope**: Complete inventory of all state plugins, schema coverage audit, quality evaluation,
and identification of missing plugins.

**Core rule**: If a plugin does not have a validated schema returned from
`StatePlugin::schema()`, it is not catalog-recognized and for all intent and purpose does not
exist on the system.

---

## Critical Finding: The Schema Gap

**Zero out of 35 plugins** currently override `StatePlugin::schema()`. The default
implementation in `op-state/src/plugin.rs` returns `None`:

```rust
fn schema(&self) -> Option<PluginSchema> {
    None   // ← all 35 plugins inherit this default
}
```

Compatibility schemas exist in `op-state-store/src/plugin_schema.rs`
(`builtin_plugin_schema_from_canonical_name`) for 37 plugin names, but these are **not**
returned by `StatePlugin::schema()`. They are a separate compatibility layer. Per the catalog
contract, a plugin must return `Some(PluginSchema)` from its trait method to be recognized.

**The only plugin that has JSON schema definitions on its struct methods is `web_ui.rs`**
(`WebUiIdentity::schema()`, `WebUiTunables::schema()`, `WebUiCapabilities::schema()`), but
even `WebUiPlugin` does not override `StatePlugin::schema()`.

---

## Plugin Inventory

### Registered with `DefaultPluginRegistry`

35 plugins have `impl StatePlugin`. The registry's `load_plugin()` recognizes these names:

| Plugin Name | Source File | Auto-Loaded | Schema Override |
|---|---|---|---|
| `adc` | `adc.rs` | ❌ | ❌ |
| `agent_config` | `agent_config.rs` | ❌ | ❌ |
| `config` | `config.rs` | ✅ | ❌ |
| `dinit` | `dinit.rs` | ✅ | ❌ |
| `dnsresolver` | `dnsresolver.rs` | ❌ | ❌ |
| `endpoint` | `endpoint.rs` | ❌ | ❌ |
| `full_system` | `full_system.rs` | ❌ | ❌ |
| `gcloud_adc` | `gcloud_adc.rs` | ❌ | ❌ |
| `hardware` | `hardware.rs` | ❌ | ❌ |
| `incus` | `incus.rs` | ✅ | ❌ |
| `keypair` | `keypair.rs` | ❌ | ❌ |
| `keyring` | `keyring.rs` | ❌ | ❌ |
| `login1` | `login1.rs` | ❌ | ❌ |
| `lxc` | `lxc.rs` | ❌ | ❌ |
| `mcp` | `mcp.rs` | ✅ | ❌ |
| `net` | `net.rs` | ✅ | ❌ |
| `netmaker` | `netmaker.rs` | ❌ | ❌ |
| `openflow` | `openflow.rs` | ✅ | ❌ |
| `openflow_obfuscation` | `openflow_obfuscation.rs` | ❌ | ❌ |
| `ovsdb_bridge` | `ovsdb_bridge.rs` | ✅ | ❌ |
| `packagekit` | `packagekit.rs` | ❌ | ❌ |
| `pcidecl` | `pcidecl.rs` | ❌ | ❌ |
| `privacy` | `privacy.rs` | ❌ | ❌ |
| `privacy_router` | `privacy_router.rs` | ✅ | ❌ |
| `privacy_routes` | `privacy_routes.rs` | ✅ | ❌ |
| `proxmox` | `proxmox.rs` | ❌ | ❌ |
| `proxy_server` | `proxy_server.rs` | ❌ | ❌ |
| `rtnetlink` | `rtnetlink.rs` | ✅ | ❌ |
| `service` | `service.rs` | ❌ | ❌ |
| `sess_decl` | `sessdecl.rs` | ❌ | ❌ |
| `software` | `software.rs` | ❌ | ❌ |
| `systemd` | `systemd.rs` | ❌ (compat alias for `dinit`) | ❌ |
| `users` | `users.rs` | ❌ | ❌ |
| `web_ui` | `web_ui.rs` | ❌ | ❌ (has struct schemas, not trait override) |
| `wireguard` | `wireguard.rs` | ❌ | ❌ |

Auto-loaded in normal mode: `mcp`, `config`, `dinit`, `incus`, `net`, `openflow`,
`ovsdb_bridge`, `privacy_router`, `privacy_routes`, `rtnetlink`  
Auto-loaded in WG-only mode: `config`, `service`, `dinit`, `net`, `rtnetlink`, `wireguard`

### Compat Schemas Without StatePlugin Implementations

These have entries in `builtin_plugin_schema_from_canonical_name` but no `impl StatePlugin`:

| Name | Notes |
|---|---|
| `incus-wireguard-ingress` | Schema only — Incus container WireGuard ingress profile |
| `incus-xray-reality-client` | Schema only — Incus Xray Reality client profile |
| `incus-xray-reality-server` | Schema only — Incus Xray Reality server profile |

### Source File Without Registry Entry

| File | Plugin Name | Issue |
|---|---|---|
| `systemd_networkd.rs` | (no `impl StatePlugin`) | Helper module only — `SystemdNetworkdManager` for network plugin |

---

## Schema Quality Analysis

Schemas were evaluated against three axes: **field coverage** (how many fields, how specific),
**type quality** (typed vs. `FieldType::Any`), and **constraints** (validation rules).

### Tier A — Excellent (typed, constrained, examples, `readOnly` used)

These are the gold standard and should be used as templates:

| Plugin | Fields | Any% | Constraints | `readOnly` Used | Notes |
|---|---|---|---|---|---|
| `incus-wireguard-ingress` | 26+ | ~8% | 7 | ✅ | Best schema in codebase |
| `incus-xray-reality-client` | 35+ | ~6% | 7 | ✅ | |
| `incus-xray-reality-server` | 36+ | ~6% | 5 | ✅ | |
| `openflow` | 16+ | 0% | 4 | ❌ | Deep typed fields, good constraints |
| `privacy_router` | 15+ | 0% | 8 | ❌ | Most constrained active plugin |
| `lxc` | 5 (object) | 20% | 1 | ✅ | Uses `readOnly` and `readOnly_when` |
| `incus` | 7+ | 0% | 0 | ✅ | Enum types, good structure |

### Tier B — Good (mostly typed, some constraints)

| Plugin | Fields | Any% | Constraints | Issue |
|---|---|---|---|---|
| `net` | 5+ | 0% | 0 | No constraints on typed fields |
| `rtnetlink` | 6+ | 0% | 0 | No constraints |
| `privacy_routes` | 13+ | 0% | 0 | No constraints |
| `dinit` | 3 | 0% | 0 | Thin but correct types |
| `dnsresolver` | 2 | 0% | 1 | Reasonable for size |
| `proxy_server` | 2 | 0% | 2 | Good constraints for typed fields |
| `packagekit` | 2 | 50% | 1 | One field still `Any` |
| `pcidecl` | 2 | 50% | 1 | One field still `Any` |
| `web_ui` (compat) | 6 | 50% | 1 | Has full struct-level schemas not wired to trait |

### Tier C — Minimal (1 field, mostly `Any`, no constraints)

These plugins are effectively invisible to the catalog because all their data is untyped:

| Plugin | Fields | Type | Issue |
|---|---|---|---|
| `adc` | 1 | Boolean | Only field is a boolean flag — no identity |
| `agent_config` | 1 | Array(Any) | `agents` array is entirely untyped |
| `config` | 1 | Any | Entire config store is `Any` |
| `endpoint` | 1 | Array(String) | Endpoint list only |
| `gcloud_adc` | 3 | 2 Any | `account`, `project_id` are untyped |
| `hardware` | 3 | All Any | CPU/memory/disk are untyped objects |
| `keypair` | 1 | Any | Entire keypair list untyped — **security risk** |
| `keyring` | 2 | All Any | Secret collections untyped — **security risk** |
| `login1` | 1 | Any | Sessions untyped |
| `mcp` | 3 | All Any | MCP servers/tools entirely untyped |
| `netmaker` | 0 | — | **Empty schema** — no fields defined |
| `openflow_obfuscation` | 1 | Any | Config untyped |
| `ovsdb_bridge` | 1 | Any | Bridge declarations untyped |
| `privacy` | 1 | Any | Privacy orchestration config untyped |
| `proxmox` | 1 | Any | Container declarations untyped |
| `service` | 1 | Any | Service map untyped |
| `sess_decl` | 1 | Any | Session declarations untyped |
| `software` | 1 | Any | Package list untyped |
| `users` | 1 | Any | User list untyped — **security risk** |
| `wireguard` | 1 | Any | Interface/peer list untyped — **security risk** |

### Tier D — Broken / Incomplete

| Plugin | Issue |
|---|---|
| `netmaker` | 0 fields in compat schema — completely empty |
| `full_system` | 11 fields but 9 are `Any`; mostly an aggregate dump with no structure |
| `systemd` | Compat alias for `dinit`; no independent schema; no `StatePlugin::schema()` |

---

## Security/Privacy Risks in Current Schemas

Several plugins handle sensitive data but have no secret or PII path declarations:

| Plugin | Sensitive Data | Risk |
|---|---|---|
| `keypair` | Private keys | `keypairs` field is `Any` — no secret paths declared |
| `keyring` | Secret service collections | `collections` is `Any` — no secret paths |
| `wireguard` | Private keys, PSKs | `interfaces` is `Any` — no secret paths |
| `gcloud_adc` | Cloud credentials | `account` is `Any`, auto-PII on `account` name may not match |
| `users` | User/group data | `users` is `Any` — PII not declared |
| `config` | May contain secrets | `configs` is `Any` — no secret paths |

Auto-detection (`is_secret_field_name`, `is_pii_field_name`) only fires on field names
containing `secret`, `private`, `token`, `password`, `credential`, `license`, `api_key`, `key`,
`email`, `account`, `google_id`, `google_email`, `user_id`. Fields like `collections`,
`interfaces`, `keypairs`, `users` are **not** caught automatically.

---

## Missing Plugins

The following capability areas have no dedicated plugin, meaning the catalog has no schema
authority and those mutations are either untracked or buried in other plugins' `Any` fields.

### High Priority (system integrity gaps)

| Missing Plugin | Domain | Why Needed |
|---|---|---|
| `mutation_footprint` | Audit trail | **The snowball plugin being designed** — no mutations tracked |
| `firewall` / `nftables` | Firewall rules | No schema for firewall policy changes |
| `certificate` / `pki` | TLS certificates | Cert lifecycle untracked; `keypair` is insufficient |
| `vault` / `secrets_backend` | Secret management | No authoritative schema for secret storage backends |

### Medium Priority (operational gaps)

| Missing Plugin | Domain | Why Needed |
|---|---|---|
| `dns_zone` | Authoritative DNS | DNS zone records not managed declaratively |
| `ntp` / `chrony` | Time sync | Time sync config untracked |
| `btrfs` | Storage subvolumes | `op-snowball`'s subvolumes have no plugin schema |
| `journal` / `logging` | Log management | Log retention/forwarding unschematized |
| `ssh_authorized_keys` | SSH access | SSH keys not declaratively managed |
| `vlan` | VLAN management | VLAN config buried in `net` as `Any` |

### Lower Priority (AI/platform specific)

| Missing Plugin | Domain | Why Needed |
|---|---|---|
| `vector_store` | Embedding backend | Vector DB config not schematized |
| `model_config` | LLM configuration | Model selection/parameters not tracked |
| `skill_registry` | Agent skills | Skills not in plugin catalog |
| `metrics` | Observability | Metrics config not declaratively managed |
| `alerts` | Alerting | Alerting rules not schematized |

### Schema-Only Plugins Needing StatePlugin Impls

| Plugin | Status | Gap |
|---|---|---|
| `incus-wireguard-ingress` | Schema ✅, StatePlugin ❌ | Can't apply/verify/rollback |
| `incus-xray-reality-client` | Schema ✅, StatePlugin ❌ | Can't apply/verify/rollback |
| `incus-xray-reality-server` | Schema ✅, StatePlugin ❌ | Can't apply/verify/rollback |

---

## Schema Consistency Issues

### Naming Inconsistencies

| Plugin Name | File Name | Issue |
|---|---|---|
| `sess_decl` (registry) | `sessdecl.rs` | File name drops underscore |
| `systemd` (alias) | `systemd.rs` | Plugin file exists but is a `dinit` alias in registry |
| `wireguard` (registry) | `wireguard.rs` | WireGuard plugin not in normal-mode auto-load |

### Version Inconsistency

Most compat schemas use `version: "1.0.0"` via `simple_schema()`. The `lxc` schema uses
`version: "2.0.0"`. No plugin has a version bump policy or migration strategy documented.

### Missing `category` Tags

The `simple_schema()` helper does not set `category`. All simple schemas default to
`"uncategorized"`. Only schemas built with explicit `.category(…)` calls (e.g., `lxc`, `incus`,
`privacy_router`) have meaningful categories. This breaks any UI or compliance query that
groups plugins by category.

Expected categories by domain:

| Category | Plugins |
|---|---|
| `network` | `net`, `rtnetlink`, `dnsresolver`, `endpoint`, `netmaker`, `wireguard`, `openflow`, `openflow_obfuscation`, `ovsdb_bridge`, `privacy_router`, `privacy_routes` |
| `compute` | `incus`, `lxc`, `proxmox`, `hardware` |
| `identity` | `users`, `keypair`, `keyring`, `adc`, `gcloud_adc`, `wireguard` |
| `services` | `dinit`, `service`, `mcp`, `agent_config` |
| `configuration` | `config`, `software`, `packagekit` |
| `audit` | `mutation_footprint` |
| `security` | `privacy`, `privacy_router`, `sess_decl` |
| `ui` | `web_ui` |
| `platform` | `pcidecl`, `login1`, `full_system` |

### Missing `example` Values

Only `lxc`, `incus`, and the incus-* variants include `example` values in their `FieldSchema`
entries. All Tier C schemas have zero examples, making them opaque to documentation generators
and LLM tools that use the schema for context.

---

## Recommended Remediation Order

### Immediate (catalog recognition)

1. Wire existing compat schemas to `StatePlugin::schema()` for the 10 auto-loaded plugins
   (`mcp`, `config`, `dinit`, `incus`, `net`, `openflow`, `ovsdb_bridge`, `privacy_router`,
   `privacy_routes`, `rtnetlink`). Each plugin file should add:
   ```rust
   fn schema(&self) -> Option<PluginSchema> {
       Some(create_<name>_schema())
   }
   ```
   This unblocks catalog recognition with zero field changes.

2. Add `mutation_footprint` plugin — this is the audit system that tracks all other mutations.
   See `crates/op-snowball/REQUIREMENTS.md` and `crates/op-snowball/DESIGN.md`.

### Short Term (schema quality)

3. Add `category` to all schemas using the domain table above.
4. Replace `Any` fields in security-sensitive plugins (`keypair`, `keyring`, `wireguard`,
   `users`) with typed `Object` schemas and declare `secret_paths`/`pii_paths`.
5. Add `example` values to all Tier C schemas.

### Medium Term (missing plugins)

6. Implement `StatePlugin` for `incus-wireguard-ingress`, `incus-xray-reality-client`,
   `incus-xray-reality-server` — schemas exist, implementations missing.
7. Add `firewall`, `certificate`, `dns_zone`, `ntp`, `btrfs` plugins.
8. Split `full_system` aggregate fields into typed sub-objects.

---

## Pattern: Wiring `StatePlugin::schema()` to Compat Schema

For any plugin that already has a compat schema in `plugin_schema.rs`, the minimum viable
schema override is:

```rust
// In crates/op-state-store/src/plugin_schema.rs — expose helper:
pub fn schema_for_net() -> PluginSchema { create_net_schema() }

// In crates/op-plugins/src/state_plugins/net.rs:
use op_state_store::plugin_schema::schema_for_net;

impl StatePlugin for NetStatePlugin {
    fn schema(&self) -> Option<op_state_store::PluginSchema> {
        Some(schema_for_net())
    }
    // … rest of trait
}
```

The 3-section pattern (`Identity` / `Tunables` / `Capabilities` structs with their own
`schema() -> Value` methods) as demonstrated by `web_ui.rs` is the preferred full-quality
approach for new or heavily refactored plugins.
