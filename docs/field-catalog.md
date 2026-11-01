# Field catalog — normalized fields across plugin contracts (draft)

Status: **draft.** All 10 domain-slice inventories are integrated
below (one linked file per slice, ~350 KB of cited field tables).
Every row cites its source; sealed/live verification is pending
(live door unreachable from the authoring session), so rows are
declarations as written, not yet live-confirmed.

Purpose: one canonical field vocabulary for tagged schema routing
(`docs/tagged-schema-routing-provenance.md`) and the private IP
provenance hash stream. A field that appears in two plugins with two
names or two types is a routing bug waiting to happen; this catalog is
where it gets one name, one type, and one sensitivity class.

## Normalization rules

1. **Names:** `snake_case`, singular, no abbreviations that collide
   (`id` always qualified: `plugin_id`, `session_id`, `operation_id`).
2. **Hashes:** `*_hash` = lowercase hex SHA-256 of canonical JSON unless
   the field says otherwise; Ethereum-facing digests are the same bytes
   as `bytes32`.
3. **Identifiers:** `*_id` = opaque, never parsed; UUIDs say so
   (`*_uuid` or format note). Human names are `*_name`, never `*_id`.
4. **Timestamps:** RFC 3339 UTC, `*_at`. Durations carry units
   (`*_seconds`, `*_micros`).
5. **Enums:** closed, `snake_case` values; open taxonomies (tags,
   controls) are versioned references, e.g. `tag` + `taxonomy_version`.
6. **Sensitivity:** every field is one of —
   `public` (may be published/anchored as-is),
   `hash-only` (publish only its hash or a Merkle commitment),
   `private` (never leaves OP-DBUS; appears in the private hash stream
   only as part of a redacted canonical payload).
7. **Receipts:** `operation_id` + `event_hash` is the accountability
   pair; any record claiming an effect must carry both or say why not.

## Slices

<!-- Integrated from the 10 slice inventories. One table per
     plugin/record: field_as_found | source | declared_type |
     naming_variants | canonical_name | canonical_type | sensitivity |
     notes. Synonym and mis-normalization lists follow each slice. -->

### 1. Identity, session, principals, keys
Full inventory: [`field-catalog/01-identity.md`](field-catalog/01-identity.md)

Headline findings: `wireguard_pubkey`/`human_pubkey`/`pubkey`/`public_key`
are one field under four names; `principal_kind` means two incompatible
things (actor class vs identity namespace — rename to `actor_class`);
timestamps mix i64/u64/DateTime/RFC3339; **no secp256k1/Ethereum wallet
field exists yet** — it is a new field, not a rename. Secrets
(WG private key, BIP39 seed, OAuth/decoy keys, persona credentials)
must never enter tags, index entries, or chain data.

### 2. Schema tags and routing decisions
Full inventory: [`field-catalog/02-tag-routing.md`](field-catalog/02-tag-routing.md)

Headline findings: registry/decision state is private by default
(`x-opdbus-private`, empty public state); hashes are hash-only when
disclosed. `schema_hash` is a plain bounded string, **not hash-format
validated**; `revision` means at least four different counters
(binding/tag/marker/policy) and must stay qualified.

### 3. Provenance stream: event chain, receipts, execution tracker
Full inventory: [`field-catalog/03-provenance-stream.md`](field-catalog/03-provenance-stream.md)

Headline findings: the private hash stream's `event_hash`/`prev_hash`
are **MD5 today, not SHA-256** — the anchor layer must re-hash
canonically before anything is published; `sealed_id_digest` is
blake3. `tags_touched` is hard-coded empty on method attempt/terminal
events, so that routing signal is currently absent there.

### 4. Chat, router, and model routing
Full inventory: [`field-catalog/04-chat-routing.md`](field-catalog/04-chat-routing.md)

Headline findings: `effective_capabilities` is the computed grant
intersection (`capability_ceiling ∩ caller grants`) — tags never
grant, in code as well as in the taxonomy. `session_id` means the
principal's account-lifetime identity here, conflicting with per-login
session meanings elsewhere; `memory_namespace` uses an empty-string
sentinel for `ui_render` mode.

### 5. JSON render, canvas, and catalog
Full inventory: [`field-catalog/05-json-render.md`](field-catalog/05-json-render.md)

Headline findings: **three incompatible admitted spec shapes** —
`children` is required in the runtime schema but optional in the Rust
canvas shapes; `visible`/`repeat`/`watch` exist in only one shape;
`type` vs `component_type` naming already conflicts across repos.

### 6. Memory, embeddings, and vectors
Full inventory: [`field-catalog/06-memory.md`](field-catalog/06-memory.md)

Headline findings: **no tag field exists in this slice** — Qdrant
payload keys are the de-facto routeable tags. Episode
`content_hash` + embedding `{provider, model_id, dimensions,
config_revision}` is a complete vectorization provenance record, but
episodes have **no purge/delete function** in op-cozo-store today.
Qdrant `operation_id` is a u64 and must not be conflated with string
provenance operation ids.

### 7. Config, datastore, and state core
Full inventory: [`field-catalog/07-config-state.md`](field-catalog/07-config-state.md)

Headline findings: state hashes are md5 in places and the literal
"unknown" in `agent_config` diff hashes (a provenance hole); legacy
capability ids (`config.read`) coexist with canonical `cap.*@v1`;
`export_config` returns the full private config map under a read cap.
Datastore's `SnapshotBinding{generation, catalog_hash}` on every output
is the provenance pattern to generalize.

### 8. Network
Full inventory: [`field-catalog/08-network.md`](field-catalog/08-network.md)

Headline findings: **four unrelated "tag" namespaces** collide — VLAN
`tag`, Xray `inbound_tag`, OpenFlow reserved match keys
(`identity_tag`/`service_tag`/`oscal_subid`, reserved but inert), and
registry `network_tag_id`. OpenFlow binds to the registry only via
`OpenFlowBindingRef.network_tag_id`; no network receipt carries
`operation_id`/`event_hash` yet. All key material is private.

### 9. System, services, and host
Full inventory: [`field-catalog/09-system.md`](field-catalog/09-system.md)

Headline findings: service state is an untyped JSON blob; pid/uid
widths differ across plugins (u32/i32/u64); timestamps are unix
counters with unspecified epochs in places. Host-identifying fields
(uids, paths, PIDs) must stay private/hash-only.

### 10. Workflows, cron, cognitive, MCP, model registry, OSCAL
Full inventory: [`field-catalog/10-workflows-oscal.md`](field-catalog/10-workflows-oscal.md)

Headline findings: `workflow_id` vs `id` for the same concept;
triggers and statuses are free strings beside properly enumerated
ones; the etched OSCAL taxonomy's operation/capability names match
the sealed backend names where checked.
## Cross-slice conflicts (integrated)

1. **Hash algorithms are not normalized.** The provenance chain
   (`prev_hash`/`event_hash`), state diff hashes, and OpenFlow
   obfuscation hashes are MD5 in places, blake3 in others
   (`sealed_id_digest`, genesis), SHA-256 by example only elsewhere,
   and the literal `"unknown"` in `agent_config` diff hashes.
   Resolution: canonical `*_hash` = lowercase hex SHA-256; the anchor
   layer re-hashes the private stream head canonically before any
   publication. Nothing already published may be silently redefined.
2. **`revision` is at least seven counters** (binding, tag, marker,
   policy, document, snapshot generation, workflow execution, config).
   Resolution: never bare `revision` across a boundary; always
   qualified (`document_revision`, `snapshot_generation`, …), and
   document vs snapshot counters must not be conflated.
3. **`operation_id` has two types:** string in provenance receipts,
   u64 in Qdrant. Resolution: `operation_id` stays the string
   provenance id; Qdrant's becomes `qdrant_operation_id`.
4. **Capability ids are mid-migration:** canonical `cap.*@v1` beside
   legacy dotted (`config.read`, `persona.execute`) and unversioned
   forms. Resolution: `cap.<domain>.<subject>.<op>@vN` everywhere at
   the routing boundary; legacy ids remain source aliases only.
5. **Timestamps:** i64/u64 unix seconds, RFC3339 strings, and
   `DateTime<Utc>` coexist, plus `0 = lifelong` vs `Option` expiry.
   Resolution: RFC3339 UTC at every boundary; convert at the source.
6. **"Tag" is four namespaces** (schema tags, VLAN tags, Xray inbound
   tags, OpenFlow match keys) plus de-facto tags (Qdrant payload keys,
   memory namespaces) with no tag field at all. Resolution: routing
   tags are always qualified (`schema_tag_id`, `network_tag_id`,
   `vlan_tag`, `inbound_tag`); payload/namespace routing must name
   its key explicitly.
7. **Identity fields collide:** `principal_kind` (two meanings),
   `session_id` (account-lifetime vs per-login), pubkey under four
   names, `user`/`id` renames in `sess_decl`. Resolutions are recorded
   per-slice in slice 1; the metadata projection is the existing
   public/hash-only subset to build on.
8. **No Ethereum wallet field exists.** The bound secp256k1 wallet in
   the design spec is new schema (`wallet_address`,
   `secp256k1_pubkey`), not a normalization of anything present.
