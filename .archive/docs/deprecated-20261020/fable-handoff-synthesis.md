# Fable 5 Handoff Synthesis

**For:** Jeremy  
**Date context:** Fable 5 promotional window (through July 7, 2026). Sessions ran July 3–4, 2026 across `operation-dbus-proto` and related trees. This doc merges commentary from Fable handoffs, spec review, UI work, and SIGNALS — not raw transcripts.

> **Superseded identity note (2026-08-31):** references below to reading
> Ghostbridge identity headers from the former raw shared-memory record are
> historical. Current callers resolve a per-session `identity_sled` projection.

---

## What these sessions covered

Three parallel threads on the canonical workspace (`operation-dbus-proto` / 3tched):

1. **Blob + schema architecture** — common schema sections, sealing pipeline, OD-32 audit (blobs exist but nothing reads them).
2. **Plugin uniformization** — `gb_*` migration plan and partial pilot work (`gb_adc`, drift-guard fixes).
3. **Mirror spec conformance** — `.kiro/specs` event-dispatcher / session refactor gaps found and partially fixed.
4. **ZeroClaw GUI** — static draft pages, D-Bus-via-gRPC wiring, Gemma gallery (incomplete at session limit).

Most code landed in `operation-dbus-proto`; this 3tched tree holds the audit signals and factory prompts that define what still needs doing.

---

## Themes

### Blob / schema architecture

- **PluginSchema is the single source of truth.** Sealed blobs are deterministic projections of it — same schema ⇒ same bytes ⇒ same hash.
- **Common schema sections consolidated into `op-blob`** (`crates/op-blob/src/sections.rs`): `PluginMetadata`, `NetworkInterface`, `ServiceConfig`, `AIModelConfig`, `SecurityKey`, plus `inject_common_subids` for plugin-namespaced subid injection without global collisions.
- **`op-plugins/common_schema_fields.rs` is now a re-export shim** — nine `gb_*` plugins already using these sections compile unchanged.
- **Acceptance gates were green** in-session: `op-blob` 21 tests, `op-plugins` 140 tests, `op-grpc-bridge` 30 tests, clippy clean.
- **Factory mission** (`FACTORY-PROMPT-op-blob-unification.md`): port per-method typed descriptors from `opdbus-blob`, unify schemars adapter into `op-blob`, retire Struct-typed synthesis, hydrate reflection from BlobStore at startup.
- **Repo is mid-merge (`pr-17`)** — conflict markers in op-projection, op-dbus-mirror, op-web, deploy scripts; workspace may not build until resolved blob-first.

### OD-32 gaps (blobs sealed, unread)

Live audit (July 3) confirmed direction: **projection should read ONLY from blobs**, not `live-schema.json`.

| Gap | Finding |
|-----|---------|
| Zero consumers | Only `opblob` CLI reads SHM blobs; bridge reflection never hydrates at startup; op-projection still reads monolith |
| Two writers | `opblob seal-shm` (boot) AND `grpc_server::register_plugin_methods` → `BlobStore::write()` — pick one authority |
| Legacy monolith wins | `/dev/shm/live-schema.json` still read by schema_router, op-identity (re-hashes, violating manifest rule), op-plugins, dbus-mirror, grpc-bridge, qdrant_shuttle |
| No blob manifest | No catalog_hash/generation on blob dir; `write_to_dir` lacks tmp+rename; stale plugin blobs never swept |
| Schema field loss | Sealing round-trips through op-blob's narrower `PluginSchema`, silently dropping signals, guarantees, category, dependencies, display_name, example, tags, mutation_index |
| Dead code | Stale `op-projection/src/blob.rs`; 12 files with merge-conflict markers |

**Amendment:** delete op-blob's duplicate `PluginSchema`; blobify canonical `op_state_store::PluginSchema` directly so hash covers all fields.

### `gb_*` migration

- **~52 `gb_*` rewrites exist on disk; only ~8 wired and trusted** (7 pre-existing + `gb_adc` pilot).
- **Factory prompt** defines 7-agent pipeline per plugin: schema location → drift guard → OSCAL subids → schemars adapter → SideEffect → x-oscal-subid → registration/retirement.
- **Phase 1 priority:** fail2ban, persona, snowball, json_render, openflow_obfuscation, software (CI blockers).
- **Constraints in effect:** `gb_*` files treated as untrusted until individually verified; skip deprecated privacy_router/privacy_routes/sessdecl; use `MIGRATION_RESEARCH_OFFICIAL_SOURCES.md` for official structured data; only references &lt;1 month old.
- **Session progress:** fixed `gb_antigravity` (12 missing LLM-projection subids) and `gb_persona` (untyped personas golden → typed array&lt;object&gt;); pilot `gb_adc` swap verified against old plugin + Google ADC docs.
- **Wave 1 parallel agents hit session limits** before wiring cozo, dnsresolver, btrfs, agent_config, login1, wgcf, endpoint, keyring, users, s6_systemctl.

### Mirror spec fixes

High-effort `.kiro/specs/op-dbus-mirror-event-session-refactor` review found behavioral gaps (cargo check passed; spec did not):

- **Event dispatcher:** `publish_delta` pre-wrote `current_data` so change detection never fired; sessions never created; events ignored `subscribed_paths`; queue overflow skipped `InterfacesRemoved`.
- **Heartbeat:** logging stub gated on nonexistent sessions — rewritten to resync objects whose sequence numbers haven't advanced.
- **StateManager watcher:** sleep-forever placeholder — wired to real `StateManager::watch()`; added `deregister_plugin` + broadcast.
- **OVSDB client:** pointed at nonexistent D-Bus destination — rewired to `org.opdbus.rovs.jsonrpc` via op-openvswitch-daemon; added `ListDbs`/`GetSchema` on daemon; capabilities probe back on D-Bus (no raw socket bypass).
- **Dead plugin cleanup:** removed lxc, privacy_router, privacy_routes, proxmox from tool discovery and builtin schemas.
- **Still open:** simd-json removal (tasks 1.3/13), session creation (task 2.3), OVSDB monitor_db stub, duplicate op-jsonrpc vs op-network OVSDB clients.

### ZeroClaw GUI

- **Root cause of "changes don't stick":** stale `~/.local/bin/zeroclaw-gui` (June 21) vs fresh builds in `target/`; self-referencing submodule `operation-dashboard-ui-07/operation-dashboard-ui-07/`.
- **Static draft pages implemented:** `pages/<route>.json` hot-reloads (~1s); same json-render DSL as catalog interpreter; `embed-pages` feature for finalization.
- **Live data via D-Bus objects:** `source` block → `PluginService/CallMethod` over existing gRPC bridge (zcall path); Ghostbridge identity sled headers from `/dev/shm/plugin_schema.dat`.
- **Starter pages:** overview (gemma_brain), grpc (json_render), privacynetwork (wireguard — xray plugin had no methods in blob).
- **Gemma gallery view started** (prompt + promote-to-catalog); session ended before json-render.dev docs handoff to Gemma and blob-dir parsing instructions.

---

## Notable quotes

> "blob architecture is the priority for this task" — Jeremy, opening the plugin uniformization session.

> "just because plugin has gb, does not mean it is right, had multiple agents fail" — Jeremy, after drift-guard fixes; `gb_*` treated as untrusted until verified.

> "blobs are sealed but NOTHING reads them; direction confirmed: projection should read ONLY from blobs" — Fable 5 OD-32 audit, SIGNALS.md.

> "sealing DROPS schema fields; blobs are not yet the whole plugin" — OD-32 amendment on op-blob's narrower PluginSchema round-trip.

> "i still would like it static pages until finalized" — Jeremy, ZeroClaw GUI thread.

> "no ip use th dbus obj" — Jeremy, on gRPC wiring (D-Bus plugin objects via bridge, not per-service IPs).

---

## Merged here vs still in operation-dbus-proto handoffs only

### Merged into this doc

- Blob/schema foundation work and OD-32 audit findings
- `gb_*` migration status, constraints, and pilot results
- Mirror spec review summary and fixes applied
- ZeroClaw static pages + D-Bus-via-gRPC wiring
- Factory prompt priorities (plugin uniformization, op-blob unification)
- SIGNALS OD-32 entries (gaps 1–5 + schema-field amendment)

### Still only in operation-dbus-proto / session transcripts

- Line-by-line diffs for mirror fixes (event_dispatcher.rs, heartbeat.rs, ovsdb.rs, etc.)
- Full 8-angle code-review candidate list (~27 items before dedup)
- Per-agent wave-1 verification reports (agents terminated at session limit)
- Nested submodule reconciliation (Antigravity split-layout chat in inner clone)
- Uncommitted working-tree state across touched crates
- Gemma gallery completion + json-render.dev documentation bundle
- Parallel `gb_*` wiring batch (10 plugins queued, not executed)

---

## Recommended next actions

Prioritized for a Gemma tonight skim:

1. **OD-32 step 1 — resolve pr-17 merge conflicts blob-first.** op-projection, op-dbus-mirror, op-web, deploy/s6/opdbus/run block the workspace; pick blob catalog as winner over manifest-writer vs identity-blob-write sides.

2. **OD-32 step 2 — one PluginSchema, one writer.** Delete op-blob's narrow `PluginSchema`; seal `op_state_store::PluginSchema` directly. Pick single blob-dir authority (`opblob seal-shm` *or* runtime grpc write, not both). Add atomic manifest (blake3 leaf-fold + generation), tmp+rename writes, stale-blob sweep.

3. **OD-32 step 3 — wire consumers to blobs.** op-projection reads via `BlobRef::plugin_schema()`; hydrate `dynamic_reflection::ActiveReflectionCatalog` from BlobStore at bridge startup; migrate 7 `live-schema.json` consumer sites; delete monolith writers.

4. **Resume `gb_*` migration serially** — one compile + full test at end (not per-wave). Phase 1 CI blockers first; verify each file against old plugin + official sources before wiring in mod.rs.

5. **ZeroClaw GUI** — restart after reinstall; reconcile self-submodule; finish Gemma gallery + give Gemma blob dir + json-render.dev docs only (per your "no other instructions" constraint).

6. **Deploy mirror fixes** when workspace builds — event dispatcher, heartbeat, StateManager watcher, OVSDB client rewiring are in working tree but not deployed (`s6-svc -r` not run in that session).

---

## Source files

| Path | Contents |
|------|----------|
| `/home/jeremy/3tched/fable-handoff.txt` | Fable 5 plugin/blob session transcript (gb_*, sections.rs, pilot status) |
| `/home/jeremy/3tched/fable-spec-check.md` | Mirror spec review + fixes transcript |
| `/home/jeremy/3tched/ui-handoff.md` | ZeroClaw GUI static pages + gRPC/D-Bus wiring transcript |
| `/home/jeremy/3tched/SIGNALS.md` | OD-32 audit entries (lines 66–67) |
| `/home/jeremy/3tched/FACTORY-PROMPT-plugin-schema-uniformization.md` | 7-agent gb_* pipeline, phases, common blobs |
| `/home/jeremy/3tched/FACTORY-PROMPT-op-blob-unification.md` | Per-method typed descriptors, adapter unification, reflection |
| `/home/jeremy/3tched/WISHLIST.md` | Task board — OD-32 row added |
| `~/git/operation-dbus-proto/` | Canonical workspace where most Fable edits landed |
| `~/git/operation-dbus-proto/operation-dashboard-ui-07/` | ZeroClaw GUI source |
| `~/git/operation-dbus-proto/.kiro/specs/op-dbus-mirror-event-session-refactor/` | Mirror refactor spec + tasks.md |
