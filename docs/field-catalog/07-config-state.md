# Slice 7 — Config, datastore, and state core

Source: FldConfigState field inventory (read-only, cited). Integrated verbatim; canonical proposals are proposals, not renames applied to code.

Sealed/live verification is pending (door unreachable); this inventory is source-only. Per Main's rule: raw config values are **private** by default; config key/path, schema hash, revision/generation, and value hash are **hash-only** unless the source contract explicitly marks the datum public (the `datastore` public projection is the explicit-public exception in this slice).

## `op-state-store::PluginSchema` — `crates/op-state-store/src/plugin_schema.rs`

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|name|plugin_schema.rs:266|String|name (PluginMetadata:95, AgentConfig:32), plugin (StateDiff:183), plugin_id (datastore.rs:46)|plugin_id|string (opaque id, 1–128)|hash-only|Plugin identity; routing key|
|category|plugin_schema.rs:269|String (default `uncategorized`)|x-oscal-category schemars ext (config.rs:24)|plugin_category|string enum|public|Free string, not enum — mis-normalized|
|version|plugin_schema.rs:271|String|version (PluginMetadata:96)|schema_version|string (semver)|public|`1.1.0`/`2.0.0` literals (config.rs:127, datastore.rs:204)|
|description|plugin_schema.rs:273|String|description (ValidationError message separate)|description|string|public||
|display_name|plugin_schema.rs:277|Option\<String\>|name|display_name|string|public|Display only, not routing|
|fields|plugin_schema.rs:279|HashMap\<String, FieldSchema\>|fields, object_schemas (PluginMetadata:103)|field_map|map<string, FieldSchema>|hash-only|Field names = config/state paths|
|public_state|plugin_schema.rs:284|PublicStatePolicy|paths (public_state.rs:34)|public_state_policy|PublicStatePolicy|public|Explicit allowlist; empty = withhold all (public_state.rs:280-284)|
|field_writes|plugin_schema.rs:289|HashMap\<String, FieldWriteDecl\>|—|field_write_map|map<string, FieldWriteDecl>|hash-only|Grants no authority (plugin_schema.rs:285-287)|
|dependencies|plugin_schema.rs:292|Vec\<String\>|dependencies (PluginMetadata:100)|dependency_plugin_ids|array<plugin_id>|hash-only|Unbounded strings, should be plugin_ids|
|example|plugin_schema.rs:295|Option\<Value\>|example (FieldSchema:72)|example_state|JSON value|private|May embed config values (config.rs:34-40 example contains a path)|
|immutable_paths|plugin_schema.rs:298|Vec\<String\>|path (StateChange:50, ValidationError:87)|immutable_paths|array<json_pointer>|hash-only|JSON-pointer-style paths|
|tags|plugin_schema.rs:301|Vec\<String\>|tag (event_chain, excluded)|schema_tags|array<tag_id>|hash-only|Tagged-routing input; unbounded strings, should be registered tag ids|
|dialect|plugin_schema.rs:304|String (default draft 2020-12, plugin_schema.rs:19)|DEFAULT_SCHEMA_DIALECT|schema_dialect|string (URI)|public||
|mutation_index|plugin_schema.rs:307|Option\<u64\>|revision (agent_config.rs:78), generation (datastore.rs:22)|mutation_index|u64|hash-only|Synonym of revision/generation — see (a)|
|subids|plugin_schema.rs:312|HashMap\<String, String\>|x-oscal-subid (config.rs:23,29)|subid_map|map<string, oscal_subid>|public|Tagged routing: `mut.…@v1`/`obs.…@v1` taxonomy keys|
|org|plugin_schema.rs:330|Option\<String\>|—|org_id|string `<tenant>.<unit>[@vN]`|hash-only|Tenant scoping, deliberately not subid (plugin_schema.rs:313-328)|
|methods|plugin_schema.rs:333|HashMap\<String, MethodDecl\>|—|method_map|map<string, MethodDecl>|public|Method names are routing surface|
|capabilities|plugin_schema.rs:340|HashMap\<String, CapabilityDecl\>|capability_grants:350|capability_map|map<capability_id, CapabilityDecl>|public||
|capability_grants|plugin_schema.rs:350|HashMap\<String, Vec\<String\>\>|required_capability (MethodDecl:143)|capability_grants|map<footprint, array<capability_id>>|hash-only|Legacy; wildcards discarded on read, never sealed (plugin_schema.rs:341-343)|
|signals|plugin_schema.rs:353|Vec\<SignalDecl\>|—|signals|array<SignalDecl>|public|
|guarantees|plugin_schema.rs:356|PluginCapabilities|capabilities() trait (plugin.rs:177)|capability_guarantees|PluginCapabilities|public|

## `FieldSchema` / `FieldType` / `MethodDecl` / `CapabilityDecl` / `SignalDecl` / `PluginCapabilities` / `FieldWriteDecl` / `ReadOnlyCondition` / `Constraint` — `plugin_schema.rs`

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|field_type|plugin_schema.rs:60|FieldType|FieldType::Any (config test:420)|field_type|FieldType enum|public|Variants String/Integer/Float/Boolean/Array/Object/Map/JsonSchema/Enum/OneOf/Any (34-53); `Any` = unbounded — mis-normalized|
|required|plugin_schema.rs:63|bool|—|is_required|bool|public||
|description|plugin_schema.rs:66|String|—|description|string|public||
|default|plugin_schema.rs:69|Option\<Value\>|serde default|default_value|JSON value|private|May be a config value|
|example|plugin_schema.rs:72|Option\<Value\>|—|example_value|JSON value|private||
|constraints|plugin_schema.rs:75|Vec\<Constraint\>|Constraint::{Min,Max,Pattern,OneOf,RequiresField,Custom} (96-109)|constraints|array<Constraint>|public|Pattern.regex:102 unbounded regex string|
|read_only|plugin_schema.rs:78|bool|readOnly (JSON Schema)|is_read_only|bool|public||
|read_only_when|plugin_schema.rs:81|Option\<ReadOnlyCondition\>|property/value (88-90)|read_only_when|ReadOnlyCondition|public|property is a field path — hash-only if config path|
|name (MethodDecl)|plugin_schema.rs:131|String|methods map key|method_name|string|public|Duplicated by map key|
|args|plugin_schema.rs:133|Value (JSON Schema)|arguments (ExecutionJob)|method_args_schema|JSON Schema object|hash-only|Schema itself may reveal config paths|
|returns|plugin_schema.rs:136|Option\<Value\>|output structs|method_returns_schema|JSON Schema object|hash-only||
|side_effect|plugin_schema.rs:138|SideEffect {Read, Mutation}|SideEffect::Read/Mutation inserts|side_effect|enum `read\|mutation`|public|Tagged-routing discriminator|
|idempotent|plugin_schema.rs:140|bool|—|is_idempotent|bool|public|Retry safety|
|required_capability|plugin_schema.rs:143|Option\<String\>|capabilities map key, CapabilityDecl.id|required_capability_id|capability_id|public|Must resolve to declared cap (plugin_schema.rs:162-164)|
|subid (MethodDecl)|plugin_schema.rs:145|String|subids map, x-oscal-subid|method_subid|oscal_subid `mut.…@v1`|public|Primary tagged-routing key|
|name (SignalDecl)|plugin_schema.rs:152|String|—|signal_name|string|public|
|payload|plugin_schema.rs:155|Option\<Value\>|state_snapshot, data|signal_payload_schema|JSON Schema|hash-only||
|subid (SignalDecl)|plugin_schema.rs:157|String|—|signal_subid|oscal_subid `evt.…`|public||
|id (CapabilityDecl)|plugin_schema.rs:171|String|`config.read`, `agent.read`, `datastore.read` (legacy)|capability_id|string `cap.<domain>.<subject>.<op>@vN`|public|Canonical form documented 169-171; all three plugins here use legacy short form — mis-normalized, tolerated per 176-179|
|description (CapabilityDecl)|plugin_schema.rs:173|String|"Grants: …" prose (config.rs:189)|capability_description|string|public|Grant list embedded in prose, not structured|
|supports_rollback / supports_checkpoints / supports_verification / atomic_operations|plugin_schema.rs:241,244,247,250|bool|capabilities() literals (config.rs:398-403)|same (snake_case)|bool|public|Single canonical def; op-state re-exports (plugin.rs:224-227)|
|method / argument (FieldWriteDecl)|plugin_schema.rs:258-259|String|args|write_method / write_argument|method_name / field_name|string|hash-only|argument is a config field name|
|paths (PublicStatePolicy)|public_state.rs:34|Vec\<String\>|immutable_paths|public_state_paths|array<json_pointer>|public|RFC 6901, max 256 paths/depth 32 (public_state.rs:15-16); empty = publish nothing|

## `config` plugin — `crates/op-plugins/src/state_plugins/config.rs`

Methods (config.rs:134-183): `list_keys`→ConfigKeysOutput (Read, `config.read`, `obs.service.config.key.list@v1`:141), `get_value` (Read, `obs.service.config.value.get@v1`:151), `set_value` (Mutation, `config.invoke`, `mut.service.config.value.set@v1`:161), `delete_key` (Mutation, `mut.service.config.key.delete@v1`:171), `export_config` (Read, `obs.service.config.export@v1`:181). Schema name/version `config`/`1.1.0` (122-130); caps `config.read`/`config.invoke` (185-198, legacy ids).

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|configs (ConfigSchemaState)|config.rs:31|JsonValue|configs (HashMap runtime, :47)|config_map|map<config_key, config_value>|private|Schema view is opaque `Any` (test:420); whole-map exposure|
|configs (ConfigStoreState)|config.rs:47|HashMap\<String, Value\>|config (export out)|config_map|map<config_key, config_value>|private|Runtime store at `/etc/op-dbus/config-store.json`:42 — path itself hash-only|
|key (ConfigKeyInput)|config.rs:52|String|key (ConfigValueInput:56), resource (StateAction)|config_key|string (bounded)|hash-only|Unbounded, no length/pattern — mis-normalized vs datastore's 1–128|
|key (ConfigValueInput)|config.rs:56|String|—|config_key|string|hash-only||
|value (ConfigValueInput)|config.rs:57|JsonValue|value (ConfigValueOutput:65)|config_value|JSON value|private|Arbitrary JSON, no schema bound|
|keys (ConfigKeysOutput)|config.rs:61|Vec\<String\>|—|config_keys|array<config_key>|hash-only|Key names leak membership|
|value (ConfigValueOutput)|config.rs:65|Option\<JsonValue\>|—|config_value|JSON value|private||
|found|config.rs:66|bool|deleted:70, success|is_found|bool|hash-only|Existence oracle for a key|
|deleted (ConfigDeleteOutput)|config.rs:70|bool|—|is_deleted|bool|hash-only||
|config (ConfigExportOutput)|config.rs:74|JsonValue|configs|config_map|map<config_key, config_value>|private|Full export under read cap `config.read` — highest-sensitivity row in slice|

## `datastore` plugin — `crates/op-plugins/src/state_plugins/datastore.rs` (explicit public projection, datastore.rs:1-6)

Methods (225-274): `list_objects`, `get_object`, `query`, `get_stats`, `export` — all Read, cap `datastore.read` (legacy), subids `obs.service.datastore.*@v2`. Version `2.0.0` (204).

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|generation (DataStoreState)|datastore.rs:22|u64|mutation_index, revision|snapshot_generation|u64|public|Provenance binding for projection|
|catalog_hash|datastore.rs:23|String|catalog_hash (SnapshotBinding:67)|catalog_hash|lowercase hex sha256|public|Example is 64-hex (210) but no length/regex constraint declared — mis-normalized|
|object_count / present_count / withheld_count / unobserved_count|datastore.rs:24-27|usize|same in GetStatsOutput:123-126|same|u64 count|public||
|state_bytes|datastore.rs:28|u64|state_bytes (DataStoreObjectRef:50)|state_size_bytes|u64 (bytes)|public|Already unit-explicit|
|objects|datastore.rs:29|Vec\<DataStoreObjectRef\>|results:117|object_refs|array<DataStoreObjectRef>|public|
|plugin_id (DataStoreObjectRef)|datastore.rs:46|String|name/plugin (PluginSchema)|plugin_id|string 1–128 (input bound :82)|public|Canonical form already|
|object_path|datastore.rs:47|String|path (StateChange)|object_path|string (D-Bus path)|public|Example `/org/opdbus/v1/plugins/service`:218|
|schema_hash|datastore.rs:48|String|schema_hash binds sealed bytes (:58)|schema_hash|lowercase hex sha256|public|Comment says it binds to sealed bytes; no format constraint on the field|
|exposure|datastore.rs:49|PublicObjectExposure {present, withheld, unobserved}|exposure filter (QueryInput:95)|exposure|enum|public|Policy outcome, provenance-relevant|
|state_bytes (ref)|datastore.rs:50|u64|—|state_size_bytes|u64|public|
|reference (DataStoreObject)|datastore.rs:56|DataStoreObjectRef|binding|object_ref|DataStoreObjectRef|public|
|schema (DataStoreObject)|datastore.rs:59|serde_json::Value|fields|plugin_schema_document|JSON Schema|public|Manifest-pinned sealed schema|
|state (DataStoreObject)|datastore.rs:61|Option\<serde_json::Value\>|state (DesiredState:14)|projected_state|JSON value|public|Only policy-filtered public state; `None` explained by exposure|
|generation / catalog_hash (SnapshotBinding)|datastore.rs:66-67|u64 / String|DataStoreState:22-23|snapshot_generation / catalog_hash|u64 / sha256 hex|public|Repeated in every output (103-134) — good provenance binding pattern|
|plugin_ids (PluginSelection/QueryInput)|datastore.rs:76,92|Vec\<String\> (max 256, inner 1–128)|plugin_id singular:83|plugin_ids|array<plugin_id>|hash-only|Selection reveals catalog interest|
|plugin_id (GetObjectInput)|datastore.rs:83|String (1–128)|plugin_ids|plugin_id|string|hash-only||
|exposure (QueryInput)|datastore.rs:95|Option\<PublicObjectExposure\>|—|exposure_filter|enum|public|
|binding (all outputs)|datastore.rs:104,110,116,122,132|SnapshotBinding|—|snapshot_binding|SnapshotBinding|public|
|results (QueryOutput)|datastore.rs:117|Vec\<DataStoreObject\>|objects:105,133|results|array<DataStoreObject>|public|

## `agent_config` plugin — `crates/op-plugins/src/state_plugins/agent_config.rs`

Methods (173-222): `get_config` (Read, `agent.read`, `obs.service.agent.config.get@v1`), `update_config` (Mutation, `agent.invoke`, `mut.service.agent.config.update@v1`), `list_tools` (Read, `obs.service.agent.tool.list@v1`), `register_tool` (Mutation, `mut.service.agent.tool.register@v1`), `reset_config` (Mutation, `mut.service.agent.config.reset@v1`). Version `2.0.0` (102,164); caps legacy `agent.read`/`agent.invoke` (224-237).

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|agents (AgentConfigState)|agent_config.rs:21|Vec\<AgentConfig\>|agents list in document (`{"agents":[…]}` :251)|agent_configs|array<AgentConfig>|private|Config values private per Main rule|
|name (AgentConfig)|agent_config.rs:32|String|name (AgentNameInput:56, RegisterToolInput:64)|agent_name|string|hash-only|Runtime-validated (trim/non-empty/no control, 242-248) but schema declares no bounds|
|enabled|agent_config.rs:38|bool|—|is_enabled|bool|private||
|model|agent_config.rs:44|Option\<String\>|—|model_id|string (opaque model id)|private|Unbounded free string where a model id belongs — mis-normalized|
|tools (AgentConfig)|agent_config.rs:51|Vec\<String\>|tool singular:65, tools out:73|tool_names|array<tool_name>|private|Sorted/deduped at write (309-310)|
|name (AgentNameInput)|agent_config.rs:56|String|—|agent_name|string|hash-only||
|config (UpdateConfigInput)|agent_config.rs:60|AgentConfig|config out:69,77|agent_config|AgentConfig|private|
|name / tool (RegisterToolInput)|agent_config.rs:64-65|String|tools|agent_name / tool_name|string|hash-only|tool is a name, not `tool_id`|
|config (AgentConfigOutput)|agent_config.rs:69|Option\<AgentConfig\>|—|agent_config|AgentConfig|private|
|tools (AgentToolsOutput)|agent_config.rs:73|Vec\<String\>|—|tool_names|array<tool_name>|private|
|config (AgentMutationOutput)|agent_config.rs:77|AgentConfig|—|agent_config|AgentConfig|private|
|revision (AgentMutationOutput)|agent_config.rs:78|u64|generation, mutation_index|document_revision|u64|hash-only|From `RuntimeDocument.revision` (runtime_documents.rs:16) via `row.revision` (agent_config.rs:328,356,382)|

## `op-state` core records — `crates/op-state/src/plugin.rs`

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|state (DesiredState)|plugin.rs:14|Value|state (datastore), document|desired_state|JSON value|private||
|timestamp (DesiredState)|plugin.rs:15|DateTime\<Utc\>|timestamp i64 (DiffMetadata:191, Checkpoint:219)|desired_at|RFC3339 UTC timestamp|hash-only|Type clash with i64 epoch elsewhere — mis-normalized|
|hash (DesiredState)|plugin.rs:16|String|current_hash/desired_hash:192-193|state_hash|lowercase hex sha256|hash-only|Computed as **md5** hex (plugin.rs:23-26) — algorithm mis-normalized|
|description (DesiredState)|plugin.rs:17|Option\<String\>|description everywhere|description|string|private|Free text may embed config|
|source (DesiredState)|plugin.rs:18|StateSource {User, AutoDiscovered, Import(String), Plugin(String), Default}|origin (event_chain, excluded)|state_source|enum|hash-only|Embedded String payloads unbounded; provenance-relevant|
|operation (StateChange)|plugin.rs:49|ChangeOperation {Create, Update, Delete, NoOp}|StateAction variants:199-202|change_operation|enum|hash-only|Duplicates StateAction vocabulary — synonym|
|path (StateChange)|plugin.rs:50|String|immutable_paths, object_path|field_path|string (JSON pointer)|hash-only|Config path → hash-only per Main|
|old_value / new_value|plugin.rs:51-52|Option\<Value\>|value, changes:200|old_value / new_value|JSON value|private|Raw config values|
|description (StateChange)|plugin.rs:53|String|—|description|string|private|
|hash (StateChange)|plugin.rs:54|String|—|change_hash|lowercase hex sha256|hash-only|No algorithm specified at field|
|timestamp (StateChange)|plugin.rs:55|DateTime\<Utc\>|i64 timestamps|changed_at|RFC3339 UTC|hash-only|
|valid / errors / warnings / suggestions (ValidationResult)|plugin.rs:68-71|bool / Vec\<ValidationError\> / Vec\<String\> / Vec\<String\>|Schema ValidationResult (plugin_schema.rs:941-945)|is_valid / errors / warnings / suggestions|bool / array|hash-only|Two ValidationResult types in workspace — synonym collision|
|path / message / code (ValidationError)|plugin.rs:87-89|String|—|field_path / message / error_code|string|hash-only|path is config path|
|name / version / description / author / license / dependencies / dbus_services / feature_schemas / object_schemas (PluginMetadata)|plugin.rs:95-103|String/String/String/Option/Option/Vec\<String\>/Vec\<String\>/Vec\<Value\>/HashMap\<String,Value\>|PluginSchema name/version/description/dependencies/fields|plugin_id / schema_version / description / author / license / dependency_plugin_ids / dbus_services / feature_schemas / object_schemas|mixed|hash-only (schemas private)|Duplicates PluginSchema identity fields — synonym; superseded by PluginSchema|
|plugin (StateDiff)|plugin.rs:183|String|plugin_id, name|plugin_id|string|hash-only|
|actions (StateDiff)|plugin.rs:184|Vec\<StateAction\>|StateChange list|actions|array<StateAction>|private|Actions carry raw config values|
|metadata (StateDiff)|plugin.rs:185|DiffMetadata|—|diff_metadata|DiffMetadata|hash-only|
|timestamp (DiffMetadata)|plugin.rs:191|i64 (epoch s)|DateTime<Utc> elsewhere|computed_at|RFC3339 UTC (from epoch s)|hash-only|config.rs:334 and agent_config.rs:114 both use `.timestamp()`|
|current_hash / desired_hash|plugin.rs:192-193|String|hash (DesiredState:16)|current_state_hash / desired_state_hash|lowercase hex sha256|hash-only|config.rs:335-336 computes **md5**; agent_config.rs:115-116 hardcodes `"unknown"` — provenance hole|
|Create{resource, config} / Modify{resource, changes} / Delete{resource} / NoOp{resource} (StateAction)|plugin.rs:199-202|String + Value|key (config), name (agent)|resource_id + config_value / changes|resource_id string; values private|private (values) / hash-only (resource)|`resource` = config key in config.rs:311-326; `config` vs `changes` field-name split by variant — mis-normalized|
|success / changes_applied / errors / checkpoint (ApplyResult)|plugin.rs:208-211|bool / Vec\<String\> / Vec\<String\> / Option\<Checkpoint\>|deleted, found|is_success / applied_change_descriptions / errors / checkpoint|bool / array<string> / array<string> / Checkpoint|private (descriptions may name keys → hash-only for key part)|config.rs:350-358 embeds key names in prose strings|
|id (Checkpoint)|plugin.rs:217|String|`config-{timestamp}` (config.rs:383), UUID (agent_config.rs:136)|checkpoint_id|string (UUID)|hash-only|Two incompatible id formats — mis-normalized|
|plugin (Checkpoint)|plugin.rs:218|String|plugin_id|plugin_id|string|hash-only|
|timestamp (Checkpoint)|plugin.rs:219|i64|DateTime<Utc>|created_at|RFC3339 UTC|hash-only|
|state_snapshot (Checkpoint)|plugin.rs:220|Value|state, data, document|state_snapshot|JSON value|private|Full config snapshot; both plugins pass `null` (config.rs:381, agent_config.rs:139) yet claim checkpoint support|
|backend_checkpoint (Checkpoint)|plugin.rs:221|Option\<Value\>|—|backend_checkpoint|JSON value|private|Opaque, unbounded|

## Supporting store records — `op-state-store/src/lib.rs`, `runtime_documents.rs` (in-scope: revision source; event_chain excluded per assignment)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|id / object_type / namespace / data (StoredObject)|lib.rs:57-60|String/String/String/OwnedValue|plugin_id, object_path|object_id / object_type / namespace_id / object_data|string / string / string / JSON|private (data) / hash-only (id)|Generic store envelope|
|objects / executions / snowball (CanonicalDbExport)|lib.rs:66-68|Vec\<StoredObject\>/Vec\<OwnedValue\>/Vec\<OwnedValue\>|—|objects / executions / snowball_events|arrays|private|Export envelope|
|revision (RuntimeDocument)|runtime_documents.rs:16|u64|generation, mutation_index|document_revision|u64|hash-only|CAS-guarded (runtime_documents.rs:194-208); source of agent_config `revision` outputs|
|document (RuntimeDocument)|runtime_documents.rs:17|Value|state, configs|document|JSON value|private|Holds `{"configs":{…}}` / `{"agents":[…]}` documents|

## (a) Duplicate / synonym fields across this slice

- **Plugin identity:** `PluginSchema.name` (plugin_schema.rs:266) = `plugin_id` (datastore.rs:46,83) = `StateDiff.plugin`/`Checkpoint.plugin` (plugin.rs:183,218) = `PluginMetadata.name` (plugin.rs:95) = trait `name()`. Canonical: `plugin_id`.
- **Version:** `PluginSchema.version` = `PluginMetadata.version` = trait `version()` (config `1.1.0`, datastore/agent_config `2.0.0`). Canonical: `schema_version`.
- **Revision counter:** `AgentMutationOutput.revision` (agent_config.rs:78) = `RuntimeDocument.revision` (runtime_documents.rs:16) ≈ `PluginSchema.mutation_index` (plugin_schema.rs:307) ≈ `DataStoreState.generation`/`SnapshotBinding.generation` (datastore.rs:22,66 — snapshot generation, not document revision; conflating them would corrupt provenance). Canonical: `document_revision` vs `snapshot_generation`, kept distinct.
- **Hashes:** `hash` (plugin.rs:16,54), `current_hash`/`desired_hash` (plugin.rs:192-193), `catalog_hash` (datastore.rs:23), `schema_hash` (datastore.rs:48) — one concept, four names, three algorithms in play (md5 at plugin.rs:25/config.rs:335, blake3 digests in schema_projection.rs:208, implied sha256 by 64-hex examples). Canonical: `*_hash` = lowercase hex sha256.
- **Timestamps:** `DateTime<Utc>` (plugin.rs:15,55) vs `i64` epoch seconds (plugin.rs:191,219). Canonical: RFC3339 UTC.
- **Change vocabulary:** `StateChange`+`ChangeOperation` (plugin.rs:48-64) duplicates `StateAction` (plugin.rs:198-203); `Create{config}` vs `Modify{changes}` split the value field name.
- **Config key:** `key` (config.rs:52) = `resource` (StateAction) = map key of `configs`. Canonical: `config_key`.
- **ValidationResult** defined twice (plugin.rs:67, plugin_schema.rs:941).
- **Capability id:** map key = `CapabilityDecl.id` = `MethodDecl.required_capability` (triple-stated).

## (b) Mis-normalized fields

1. **Hash algorithm:** `DesiredState.hash` and config `current_hash`/`desired_hash` are md5 hex (plugin.rs:23-26, config.rs:335-336), not sha256; agent_config diff hashes are the literal `"unknown"` (agent_config.rs:115-116). No `*_hash` field anywhere declares length/regex — including `catalog_hash`/`schema_hash` whose examples are 64-hex (datastore.rs:210,219).
2. **Legacy capability ids:** `config.read`, `agent.read`, `agent.invoke`, `datastore.read` vs canonical `cap.<domain>.<subject>.<op>@vN` (plugin_schema.rs:169-185). Grants also embedded as prose in `CapabilityDecl.description` (config.rs:189).
3. **Unbounded strings where bounded/id types belong:** `key` (config.rs:52, no limits — contrast datastore's 1–128/plugin_id caps), `AgentConfig.name`/`model` (agent_config.rs:32,44; `model` should be `model_id`), `tags` (plugin_schema.rs:301), `dependencies` (plugin_schema.rs:292), `category` free string (plugin_schema.rs:269).
4. **Opaque values:** `configs: JsonValue`/`FieldType::Any` (config.rs:31, plugin_schema.rs:53), `value: JsonValue` (config.rs:57), `state_snapshot`/`backend_checkpoint: Value` (plugin.rs:220-221) — no per-key schema, so no field-level sensitivity tagging is possible inside config values today.
5. **Checkpoint id formats:** `config-{epoch}` (config.rs:383) vs UUID (agent_config.rs:136); both plugins snapshot `null` while advertising `supports_checkpoints: true` (config.rs:380-388, agent_config.rs:134-142).
6. **Case/format:** all Rust fields are already snake_case; anomalies are semantic, not casing — `state_bytes` (datastore) is the good pattern; `timestamp` (i64 vs DateTime) and `PluginMetadata.feature_schemas/object_schemas` (plugin.rs:102-103) duplicate `PluginSchema.fields`.

## (c) Tagged-routing / provenance relevance

- **Routing keys:** `MethodDecl.subid` (`mut.…@v1`/`obs.…@v1`, plugin_schema.rs:145) + `PluginSchema.subids` map (:312) + `schema.tags` (:301) are the tag surface; `side_effect` (:138) and `required_capability` (:143) gate execution. All three plugins' method subids are enumerated above.
- **Provenance binding (strong pattern):** datastore stamps every output with `SnapshotBinding{generation, catalog_hash}` and binds each object via `schema_hash` to sealed bytes (datastore.rs:56-58) — the model to generalize.
- **Provenance gaps:** config diffs hash with md5 and agent_config diffs with `"unknown"` (above); `ApplyResult` has no operation/receipt id in this slice, and `Checkpoint.state_snapshot` is `null` in both stateful plugins, so neither diffs nor checkpoints can presently anchor a receipt.
- **Sensitivity boundary:** `public_state.paths` empty-by-default (public_state.rs:34, plugin_schema.rs:280-284) is the only explicit-public mechanism; `export_config` returning the full private map under a read cap (config.rs:174-182) is the sharpest conflict with the private-by-default rule.