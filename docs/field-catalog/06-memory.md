# Slice 6 — Memory, embeddings, and vectors

Source: FldMemory field inventory (read-only, cited). Integrated verbatim; canonical proposals are proposals, not renames applied to code.

# Field Inventory — Memory / Cozo / Qdrant / Embedding (FldMemory slice)

Sealed/live verification is pending (door unreachable); all rows below are source-declared only.

Paths abbreviated: `MP=memory_plugin.rs`, `MM=memory_methods.rs`, `CZ=cozo.rs`, `CZM=cozo_methods.rs`, `QD=qdrant.rs`, `EM=embedding_model.rs`, `EMM=embedding_model_methods.rs` (all under `crates/op-plugins/src/state_plugins/`); `EP=ctl_plane_chatbot_episodes.rs`, `RD=runtime_document_transactions.rs`, `WF=workflow_executions.rs`, `SG=system_graph.rs`, `LIB=lib.rs` (all under `crates/op-cozo-store/src/`).

---

## Part A — Plugin method contracts

### A1. `memory` plugin — `MemoryState` (MP)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|backend|MP:16|String|`engine` (CZ:179)|backend|string enum `cozo`|public|Hardcoded `"cozo"` (MP:35)|
|isolation|MP:17|String|—|isolation|string enum `principal_session`|public|Hardcoded `"principal_session"` (MP:36); scoping semantics, not a tag field|

### A2. `memory` — `NamespaceInfo` / `EntryInfo` (outputs)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|id|MP:106,119|String|`deleted_entry_id` (MP:217)|namespace_id / entry_id|*_id opaque string|private|UUID-like, generated server-side; ambiguous bare `id` in both structs|
|name|MP:107|String|`name` in inputs MP:130,148,160,223|namespace_name|string (≤128, `[A-Za-z0-9-_.:/]`, MM:473-482)|private|User-chosen namespace label|
|max_entries|MP:109,132,235|Option\<u64\>|—|max_entry_count|u64? ( >0 )|public|`None`=unbounded; duplicated in `NamespaceStats` MP:235|
|read_only|MP:110,134,236|bool|—|read_only|bool|public|Also in stats (MP:236)|
|entry_count|MP:111,179|u64|`entries` (MP:234), `total_entries` (MP:242)|entry_count|u64|public|Three names for counts: `entry_count`/`entries`/`total_entries`|
|config|MP:112,136|JsonValue|—|namespace_config|JSON object (unbounded schema)|private|Free-form; credential fields rejected on entry values (MM:500), config validated via `validate_domain_value` (MM:142)|
|created_at|MP:113,123|String|—|created_at|RFC3339 UTC string|private|Set via `Utc::now().to_rfc3339()` (MM:342)|
|updated_at|MP:114,124|String|—|updated_at|RFC3339 UTC string|private|Same format as above|
|namespace|MP:120,171,185,199,211|String|`name` (namespace inputs)|namespace_name|string|private|**Same concept, two names**: `namespace` on entries, `name` on namespaces|
|key|MP:121,172,186,200,212|String|—|entry_key|string (≤512, MM:485-493)|private|User-chosen key within namespace|
|value|MP:122,173,187|JsonValue|`previous_value` (MP:193)|entry_value|JSON (≤1 MiB, MM:495-501)|private|**Private content** — never projected/logged (MM:3-5)|
|previous_value|MP:193|JsonValue|`value`|previous_entry_value|JSON|private|Returned by `update_entry` only — leaks prior content to caller (authorized scope)|

### A3. `memory` — method inputs/outputs (grouped)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|name (Create/Delete/Query/ClearNamespaceInput)|MP:130,148,160,223|String|`namespace` (entry inputs)|namespace_name|string|private|See A2 synonym|
|namespace (Insert/Update/Get/DeleteEntryInput)|MP:171,185,199,211|String|`name`|namespace_name|string|private|See A2|
|created (CreateNamespaceOutput)|MP:142|bool|—|created|bool|public|Idempotency signal|
|deleted / deleted_entries|MP:153-154,228|bool / u64|`deleted_entry_id` MP:217|deleted, deleted_entry_count|bool / u64|public|Purge receipts for namespace delete/clear|
|deleted_entry_id|MP:217|Option\<String\>|`id`|deleted_entry_id|*_id?|private|Only place entry id surfaces on delete|
|namespace (QueryNamespaceOutput)|MP:165|Option\<NamespaceInfo\>|—|namespace|NamespaceInfo?|private|—|
|entry (Insert/Get/UpdateEntryOutput)|MP:178,192,205|EntryInfo / Option\<EntryInfo\>|—|entry|EntryInfo?|private|—|
|total_namespaces / total_entries / namespaces (GetStatsOutput)|MP:241-243|u64/u64/Vec\<NamespaceStats\>|`entry_count`|namespace_count, entry_count, namespaces|u64, u64, array|public|Aggregate stats; no content|
|scope (CompactOutput)|MP:248|String|—|compact_scope|string|public|Whole-store Cozo `::compact` (MM:58-63)|
|compacted_at (CompactOutput)|MP:249|String|—|compacted_at|RFC3339 UTC|public|—|
|reclaimed_bytes (CompactOutput)|MP:251|Option\<u64\>|—|reclaimed_byte_count|u64?|public|Always `None` in practice — Cozo reports no delta (MP:250)|

### A4. `memory` — internal (not in schema, cross-ref to Part B)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|principal_id (MemoryAccessContext)|MM:32|String|`principal_id` EP:19|principal_id|string ≤512|private|Verified caller context, **not** a method arg; bridge-supplied|
|session_id (MemoryAccessContext)|MM:33|String|`session_id` EP:20, LIB:101,120|session_id|string ≤512|private|Same|
|owner_document_key (derived)|MM:548-564|String (derived)|—|owner_key_hash|lowercase hex sha256|hash-only|`memory:{sha256("op-dbus.memory.owner.v1\0"‖len‖principal‖len‖session)}` — provenance-grade scoping hash, never exposed|
|MemoryDocument.namespaces|MM:404|BTreeMap\<String,StoredNamespace\>|—|namespaces|map|private|Stored as one RuntimeDocument (→ RD record, B6)|
|StoredNamespace.{id,name,max_entries,read_only,config,entries,created_at,updated_at}|MM:408-417|mixed|`NamespaceInfo` MP:105-115|—|—|private|Storage twin of `NamespaceInfo` + `entries` map; `entry_count` derived (MM:445)|
|StoredEntry.{id,key,value,created_at,updated_at}|MM:420-426|mixed|`EntryInfo`|—|—|private|Storage twin of `EntryInfo` minus `namespace`|

### A5. `cozo` plugin — inputs (CZ) and outputs (CZM)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|query (RunQueryInput)|CZ:33|String|—|datalog_query|string|private|Raw Datalog — arbitrary content access behind `cap.data.cozo.query.run@v1` (CZ:338)|
|params (RunQueryInput)|CZ:35|HashMap\<String,Value\>|—|query_params|map|private|Untyped|
|data (ImportRelationsInput)|CZ:43|BTreeMap\<String,RelationRows\>|`data` (ExportOutput CZM:63)|relation_data|map|private|RelationRows{headers:Vec\<String\> CZM:40, rows:Vec\<Vec\<Value\>\> CZM:41} — fully untyped cells|
|relations (Export/Backup/List)|CZ:49,62; CZM:51,129,139|Vec\<String\>|`relation` singular CZ:135,150|relation_names|array\<string\>|public|Name lists|
|path (Backup/RestoreInput)|CZ:57,70|String|`archive` (receipts CZM:126,137)|archive_label / archive_name|string ≤64|public|**Misnamed**: explicitly *not* a filesystem path (CZ:55-56, CZM:1037); receipt renames it `archive`|
|name (DescribeRelationInput)|CZ:80|String|`relation`|relation_name|string|public|Third variant for relation naming|
|snapshot (ReplaceSystemGraphInput)|CZ:87|GraphSnapshot (→ SG:79)|—|graph_snapshot|GraphSnapshot|public|Cross-ref B7|
|expected_revision|CZ:88,107|u64|`revision` SG:90, CZM outputs|expected_revision|u64|public|CAS token; synonym of store `revision`|
|namespace (SystemGraphStatusInput)|CZ:94|String|`namespace` SG:81,89|graph_namespace|string|public|Graph namespace ≠ memory namespace ≠ collection|
|results (QueryOutput)|CZM:46|Vec\<RelationRows\>|`results` QD:212|query_results|array|private|Untyped rows out|
|rows / committed / triggers_enabled (ImportOutput)|CZM:52-55|u64/bool/bool|—|imported_row_count, committed, triggers_enabled|u64,bool,bool|public|Write receipt|
|encoding (ExportOutput)|CZM:62|String|—|export_encoding|string enum `cozo-json`|public|Tagged codec note (CZM:60-61) relevant to provenance fidelity|
|archive / sha256 / bytes / relations / rows / format / scope (ArchiveReceipt)|CZM:126-133|String/String/u64/…|—|archive_name, archive_sha256, archive_byte_count, …|*_hash lowercase hex sha256|hash-only|**Provenance receipt**: `sha256` is the archive digest; `format`,`scope` are free strings|
|archive / sha256 / relations / rows / committed / atomic / durability (RestoreReceipt)|CZM:137-145|mixed|—|…, durability|string|hash-only (`sha256`)|Receipt; `durability` is a prose string, should be enum|
|RelationSummary{name,arity,access_level,n_keys,n_non_keys,n_put/rm/replace_triggers,description}|CZM:69-78|mixed|`CozoRelation` CZ:149-156|relation_name, …|public|**Duplicate of `CozoRelation`** (CZ:150-155: name,description,keys,values,access_level,row_count) with different field set|
|ColumnDescription{column,is_key,index,type→type_name,has_default}|CZM:83-88|mixed|`CozoColumn` CZ:112-128 (`name`,`type`,`is_key`,`default`,`description`)|column_name, column_type, …|public|Renamed via `#[serde(rename="type")]`; `CozoColumn.default: Option<String>` vs `has_default: bool` — lossy variant|
|IndexDescription{name,type→index_type,relations,config}|CZM:94-98|mixed|`CozoIndex` CZ:133-137 (name,relation,columns)|index_name, index_type|public|Singular `relation` vs plural `relations`|
|TriggerDescription{type→event,idx,trigger}|CZM:104-107|mixed|`CozoTrigger` CZ:141-144 (relation,event,query)|trigger_event, trigger_query|public|`trigger: String` holds the query text — private-ish (query logic)|
|CozoHnswIndex{name,relation,vector_column,dim,dtype,m,ef_construction,ef_search,distance}|CZ:160-170|mixed|`QdrantHnswConfig` QD:104-115; `dimensions` EM:53|vector_dimension_count u32; distance enum|public|`dim` vs `size` (QD:88) vs `dimensions` (EM) — three names for vector length; `ef_construction` vs Qdrant `ef_construct`|
|CozoState{engine,path,version,relations,indices,hnsw_indices,triggers,running_queries}|CZ:176-200|mixed|—|…|public|path: CZ:182 — server-local FS path in state projection, hash-only at most|

### A6. `qdrant` plugin

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|name (Create/DeleteCollectionInput)|QD:149,155|String|`collection_name` (all other inputs)|collection_name|string|public|**Split naming**: create/delete use `name`, everything else `collection_name` (QD:163,169,176,182,188,193,198)|
|collection_name|QD:163-198|String|`name`|collection_name|string|public|See above|
|vectors_config (CreateCollectionInput)|QD:150|Value (untyped)|`vectors` QD:70, `vector` QD:170,180|vector_config|typed VectorParams|public|Untyped at creation; typed on read-back (`QdrantVectorParams` QD:87-100)|
|points (Upsert/Delete/Scroll)|QD:164,177,217|Vec\<Value\>|—|points|array of {id,vector,payload}|private|**Untyped points**: `id` (int in tests QD tests:132-135), `vector`, `payload` all invisible to schema; payload = private content|
|vector (SearchPointsInput)|QD:170|Vec\<f32\>|`vectors` EmbedOutput EMM:180|query_vector|array\<f32\>|private|Query embedding — private|
|limit (Search/Scroll)|QD:171,183|usize|—|result_limit|u32|public|—|
|results (SearchPointsOutput)|QD:212|Vec\<Value\>|`results` CZM:46|search_results|array of {id,score,payload}|private|**Score is untyped**: `score: f64` exists only inside the opaque Value (observed in native test `qdrant_native_tests.rs:241`, `result[0]["score"]`); no declared score field anywhere in the contract|
|next_page_offset (ScrollPointsOutput)|QD:218|Option\<Value\>|`next_episode_id` EP:75, `next_after_*` SG|next_page_cursor|opaque cursor?|public|Cursor naming diverges per crate|
|field_name / field_schema (CreatePayloadIndexInput)|QD:199-200|String / Value|—|payload_field_name, payload_field_schema|string, JSON|public|Payload index = tag-routing-relevant (payload keys are the de-facto tags)|
|QdrantCollectionInfo{name,status,optimizer_status,points_count,indexed_vectors_count,segments_count,config,payload_schema}|QD:38-49|mixed|—|collection_name, collection_status, …, point_count, indexed_vector_count, segment_count|u64 counts|public|`points_count` vs memory `entry_count` vs episode `episode_count` — count suffix inconsistent (`points_count`/`episode_count` vs bare `entries`)|
|QdrantVectorParams{size,distance,hnsw_config,quantization_config,on_disk,datatype,multivector_config}|QD:87-100|mixed|`dim` CZ:164, `dimensions` EM:53|vector_dimension_count u32; distance enum (`Cosine` etc. as string)|public|`size` is the third name for dimensionality|
|QdrantHnswConfig{m,ef_construct,full_scan_threshold,max_indexing_threads,on_disk,payload_m,m0}|QD:104-115|mixed|`ef_construction` CZ:167|hnsw_ef_construct u32|public|Param name drift vs Cozo HNSW|
|QdrantState{version,title,commit,http_endpoint,grpc_endpoint,collections,cluster_status,telemetry}|QD:132-143|mixed|`endpoint` EM:48|service_version, source_commit, …|public|`commit` here = build commit; cf. GraphProvenance.source_commit SG:53|
|OperationMutationOutput{status,operation_id,operation_status}|QD:233-237|String/u64/String|`operation_id` elsewhere = string|operation_id|u64 (Qdrant-native)|public|Type clash: Qdrant `operation_id: u64` vs provenance `operation_id: String` — must not be conflated in receipts|
|SnapshotDescription{name,size,creation_time,checksum}|QD:240-248|String/u64/Option\<String\>/Option\<String\>|`sha256` CZM:127|snapshot_name, snapshot_byte_count, created_at RFC3339?, snapshot_checksum|hash-only (`checksum`)|`checksum` algorithm unspecified — vs explicit `sha256` in Cozo receipts|
|CreateSnapshotOutput{status,snapshot}|QD:251-254|—|—|—|—|public|—|
|CollectionMutationOutput{status,result}|QD:227-230|String/bool|—|mutation_status, mutation_result|public|—|

### A7. `embedding_model` plugin

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|EmbeddingModelState.config_revision|EM:34|u64|`revision` (ConfigOutput EMM:205), `config_revision` (Embed/ListModels EMM:186,199; EpisodeEmbedding EP:31)|embedding_config_revision|u64|public|CAS/version of embedding config; carried into every episode embedding (EP:31) — key provenance link|
|provider (State)|EM:39|String|`EmbeddingProvider` enum EMM:28-33|embedding_provider|string enum `voyage\|ollama\|openai_compatible`|public|**Type drift**: String in state/episode records, enum in config/outputs; enum serializes `voyage/ollama/openai_compatible` (alias `openai`, EMM:31)|
|model_id (State/Config)|EM:44; EMM:56,211|String|`model` (EmbedOutput EMM:183; EpisodeEmbedding EP:28)|embedding_model_id|string ≤512|public|Canonical model name|
|model (EmbedOutput)|EMM:183|String|`model_id`|embedding_model_id|string|public|Synonym of `model_id` within same plugin|
|provider_model|EMM:184; EP:29|Option\<String\>|—|provider_reported_model_id|string?|public|Provider-echoed model; distinct from requested `model_id` — provenance-relevant, keep separate|
|endpoint|EM:48; EMM:59|String|`http_endpoint`/`grpc_endpoint` QD:136-137|embedding_endpoint|URL string|public|Credentials forbidden in endpoint (EMM:57-58); api_key never a field — env-only (`VoyageEmbedParams.api_key` EM:134, deliberately non-serializable EM:129-135)|
|dimensions|EM:53; EMM:62,181; EP:30|u32|`size` QD:88, `dim` CZ:164|embedding_dimensions|u32 (1..=16384, EMM:121-124)|public|Voyage trio fixed 1024 (EM:104)|
|status (State)|EM:58|String|—|embedding_status|string enum (`unprobed`, …)|public|Free string, should be enum; "unprobed" default (EMM:140)|
|available_models|EM:63|Vec\<String\>|`models` EMM:191|available_model_ids|array\<string\>|public|—|
|model_digest|EM:69|String|—|embedding_model_digest|string (provider-reported)|hash-only|Empty when provider exposes none (EM:64-69); algorithm unspecified — mis-normalized vs `*_sha256` convention|
|EmbeddingConfig{provider,model_id,endpoint,dimensions,request_dimensions,timeout_ms}|EMM:54-68|enum+…|`EmbeddingModelState`|embedding_provider, …, timeout_ms u64 (100..=120000)|public|Config input twin of State minus status/digest/revision|
|EmbedInput.texts|EMM:168|Vec\<String\>|`episode`/`value`/`push` (content fields)|input_texts|array\<string\>|private|**Private content** in transit|
|EmbedInput.input_type|EMM:173|Option\<enum Query\|Document\>|—|embedding_input_type|enum|public|Retrieval-role hint|
|EmbedInput.truncate|EMM:175|bool (default false)|—|truncate_input|bool|public|—|
|EmbedOutput.vectors|EMM:180|Vec\<Vec\<f32\>\>|`vector` EP:26, QD:170|embedding_vectors|array\<array\<f32\>\>|private|**Private derived content**; cardinality/dim/finiteness validated (EMM:1182-1193)|
|ListModelsOutput{models,provider,source,enumerated,embedding_support_verified,config_revision}|EMM:190-199|mixed|—|model_ids, …, model_source enum `provider\|configured`|public|`source` is provenance of the list itself|
|SetModelInput.model_id / UpdateConfigInput.config|EMM:210-217|String / EmbeddingConfig|—|embedding_model_id / embedding_config|public|—|
|VoyageModel enum variants|EM:85-92|`voyage-4-large\|voyage-4\|voyage-4-lite`|`model_id`|embedding_model_id|enum|public|Legacy shared-space catalog; shared collection, no re-vectorization on switch (EM:72-82)|

---

## Part B — `op-cozo-store` record structs

### B1. `EpisodeScope` (EP:18-22) — the memory/episode identity key

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|principal_id|EP:19|String|`principal_id` MM:32|principal_id|string ≤512B, no controls (EP:108)|private|Scope key pt.1; duplicated inside episode JSON and checked equal (EP:142)|
|session_id|EP:20|String|`session_id` LIB:101,120|session_id|string ≤256B|private|Scope key pt.2|
|session_genesis|EP:21|String|`session_genesis` LIB:125; `genesis` (Cozo param EP:255)|session_genesis|string ≤512B|private|Scope key pt.3; binds episodes to identity genesis — provenance anchor|

### B2. `ScopedEpisodeRecord` (EP:35-41) + Cozo relation (EP:447-453)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|scope|EP:36|EpisodeScope|flattened columns `principal_id,session_id,session_genesis` EP:448|episode_scope|EpisodeScope|private|Struct nests, relation flattens|
|episode_id|EP:37|String|—|episode_id|string ≤128B (EP:15)|private|Key pt.4; also duplicated inside `episode` JSON (EP:146)|
|content_hash|EP:38|String|`content_hash` (knowledge EP:49)|episode_content_hash|lowercase hex sha256 (64 chars, validated EP:127-131 — hex check only, case not enforced)|hash-only|**Hash-only provenance field**; duplicated inside episode JSON (EP:147)|
|episode|EP:39|Value (JSON, ≤16 MiB EP:8)|`episode_json: String` EP:451|episode_document|JSON object|private|**Private content**; must contain principal_id/session_id/session_genesis/on_behalf_of/episode_id/content_hash matching the key (EP:140-152) — `on_behalf_of` exists only inside the JSON, no struct field|
|embedding|EP:40|Option\<EpisodeEmbeddingRecord\>|`embedding_json: String default ""` EP:452|episode_embedding|EpisodeEmbeddingRecord?|private|Vectorization state is **implicit**: `None`/`""` = unvectorized, `Some` = vectorized. No status enum, no timestamp, no failure state|
|revision / episode_count (scope relation)|EP:454-456 (relation); read EP:308-329|Int / Int|`revision` everywhere|scope_revision u64, episode_count u64|public|Stored in `ctl_plane_chatbot_episode_scopes`; surfaces via EpisodeSnapshot/Page|

### B3. `EpisodeEmbeddingRecord` (EP:25-32) / `EpisodeEmbeddingWire` (EP:402-409)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|vector|EP:26 / 403|Vec\<f32\>|`vectors` EMM:180, `vector` QD:170|embedding_vector|array\<f32\>, len=dimensions, all finite (EP:82-85)|private|Derived private content; wire struct is field-identical duplicate (EP:402-409) — pure serialization twin|
|provider|EP:27 / 404|String|`EmbeddingProvider` enum|embedding_provider|string ≤128B (EP:87)|public|String here vs enum in plugin — see A7|
|model|EP:28 / 405|String|`model_id`|embedding_model_id|string ≤512B|public|Synonym drift (model vs model_id) persists into storage|
|provider_model|EP:29 / 406|Option\<String\>|EP:184(same)|provider_reported_model_id|string? ≤512B|public|—|
|dimensions|EP:30 / 407|u32|`size`,`dim`|embedding_dimensions|u32 >0|public|Redundant with `vector.len()` — validated consistent (EP:84)|
|config_revision|EP:31 / 408|u64|`config_revision` EM:34|embedding_config_revision|u64|public|**The provenance link** episode→embedding-config; CAS via `compare_exchange_episode_embedding` (EP:804-848) keyed on content_hash+expected embedding|

### B4. `ScopedKnowledgeRecord` (EP:46-51), `EpisodeSnapshot` (EP:54-63), `EpisodePage` (EP:71-76)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|push_id|EP:48|String|—|knowledge_push_id|zero-padded 20-digit decimal string (EP:161-162)|private|Id *is* the scope revision that committed it (EP:43-44) — ordering/provenance dual-use|
|content_hash (knowledge)|EP:49|String|`content_hash` EP:38|knowledge_content_hash|hex sha256 64|hash-only|Same validation as B2|
|push|EP:50|Value (≤64 KiB EP:67)|`push_json` EP:462|knowledge_push_document|JSON object|private|**Private content**; inner fields checked vs key (EP:178-183)|
|episodes / episode_count / revision / truncated (Snapshot)|EP:55-58|Vec\<ScopedEpisodeRecord\>/u64/u64/bool|—|episodes, episode_count, scope_revision, snapshot_truncated|public (except episodes)|Snapshot bounded 16 MiB (EP:9)|
|knowledge / knowledge_count / knowledge_truncated (Snapshot)|EP:60-62|Vec\<ScopedKnowledgeRecord\>/u64/bool|—|knowledge_pushes, knowledge_push_count, knowledge_truncated|private (pushes)|Newest ≤32 pushes (EP:66)|
|next_episode_id (Page)|EP:75|Option\<String\>|`next_page_offset` QD:218, `next_after_id` SG:156|next_episode_id|episode_id?|private|Cursor = last episode_id; three cursor namings across slice|

### B5. `WorkflowExecutionRecord` (WF:7-15) — reuses EpisodeScope

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|scope|WF:8|EpisodeScope|—|episode_scope|EpisodeScope|private|Same scope triple as B1 — cross-domain reuse|
|execution_id|WF:9|String|—|workflow_execution_id|string ≤128|private|—|
|workflow_id|WF:10|String|—|workflow_id|string ≤256|private|—|
|owner_epoch|WF:11|String|—|owner_epoch|string ≤128|private|Ownership fencing token; "ownership never reconstructed after restart" (WF:1-2)|
|status|WF:12|String|—|workflow_execution_status|string ≤64, should be enum|private|Free string|
|revision|WF:13|u64|—|execution_revision|u64 ≤ i64::MAX|public|—|
|document|WF:14|Value (object, ≤256 KiB)|`episode`/`push`/`document` RD:44|workflow_document|JSON object|private|Private content; inner `accepted_at` must parse RFC3339 (WF:50-57) — timestamp lives untyped inside JSON|

### B6. `RuntimeDocumentRecord` (RD:42-45) — substrate of the whole memory plugin

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|revision|RD:43|u64|—|document_revision|u64|public|Memory document (A4) version; CAS substrate|
|document|RD:44|Value|`MemoryDocument` MM:403|document|JSON|private|Debug impl redacts as `"<private>"` (RD:51) — explicit private marking in code|

### B7. `GraphSnapshot` / `GraphProvenance` / `GraphSnapshotReceipt` (SG) — cross-referenced by cozo plugin (CZ:87,94-100)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|GraphSnapshot{format_version,namespace,provenance,nodes,edges}|SG:79-85|u32/String/…|`snapshot` CZ:87|… graph_namespace …|public|Input to `replace_system_graph`|
|GraphProvenance{kind,source_commit,release_id,generated_at,inputs,coverage,limitations}|SG:51-59|enum/String/…|`commit` QD:135|provenance_kind enum, source_commit, release_id?, generated_at RFC3339, input_digests map, …|public|**Explicit provenance record** — the model for tagged-routing provenance; `inputs: BTreeMap<String,String>` values are untyped (digests? paths?)|
|GraphNode{id,kind,label,attributes}|SG:62-67|String/…/map|`id` everywhere|node_id, node_kind, node_label, node_attributes|public|`kind` vs `type` (CZM renamed fields) drift|
|GraphEdge{src,relation,dst,evidence,attributes}|SG:70-76|…|`relation` CZ:135|edge_source_id, edge_relation, edge_destination_id|public|`src`/`dst` abbreviations vs full names elsewhere|
|GraphEvidence{source,locator,assertion}|SG:44-48|String/String/enum|—|evidence_source, evidence_locator, assertion_kind|public|Assertion ∈ Declared/Observed/Inferred (SG:37-40) — trust grading for routing|
|GraphSnapshotReceipt{namespace,revision,digest,node_count,edge_count,provenance}|SG:88-95|…|`sha256` CZM:127, `checksum` QD:247|graph_namespace, graph_revision, snapshot_digest, node_count, edge_count|hash-only (`digest`)|Receipt returned by cozo system-graph methods; `digest` algorithm unspecified in field name |

*(Other op-cozo-store structs — identity sled/genesis, chat conversations, UI gallery, model registry — are outside this slice's focus; the memory-relevant ones are B1–B7 above. `IdentitySledRecord.vector_id` LIB:128 and `GenesisInputsRecord.{catalog_hash_at_arrival,schema_content_hash}` LIB:149,152 are noted only as hash/id neighbours of B1's `session_genesis`.)*

---

## (a) Duplicate / synonym fields across the slice

1. **Model name**: `model_id` (EM:44, EMM:56) vs `model` (EMM:183, EP:28) vs `provider_model` (EMM:184, EP:29 — semantically distinct, keep split).
2. **Provider**: `provider: String` (EM:39, EP:27) vs `provider: EmbeddingProvider` enum (EMM:55,185).
3. **Vector dimensionality**: `dimensions` (EM:53/EMM:62/EP:30) vs `size` (QD:88) vs `dim` (CZ:164).
4. **Namespace identifier**: `name` (MP namespace inputs) vs `namespace` (MP entry inputs, CZ:94, SG:81).
5. **Collection identifier**: `name` (QD:149,155) vs `collection_name` (QD:163+).
6. **Relation metadata structs duplicated**: `CozoRelation` (CZ:149) ≈ `RelationSummary` (CZM:68); `CozoColumn` (CZ:112) ≈ `ColumnDescription` (CZM:82); `CozoIndex` (CZ:133) ≈ `IndexDescription` (CZM:93); `CozoTrigger` (CZ:141) ≈ `TriggerDescription` (CZM:103) — each pair with renamed/lossy fields (`type`→`type_name`, `default: Option<String>`→`has_default: bool`, `query`→`trigger`).
7. **Counts**: `entry_count` / `entries` / `total_entries` (MP), `points_count`/`indexed_vectors_count` (QD), `episode_count`/`knowledge_count` (EP), `node_count`/`edge_count` (SG), `rows` (CZM).
8. **Revision**: `revision` (RD:43, SG:90, WF:13, EP scopes) vs `expected_revision` (CZ:88) vs `config_revision` (EM/EMM/EP embedding).
9. **Hashes**: `content_hash` (EP:38,49), `sha256` (CZM:127,138), `digest` (SG:91), `model_digest` (EM:69), `checksum` (QD:247), `catalog_hash_at_arrival`/`schema_content_hash` (LIB:149,152) — six names, only `sha256` self-describing.
10. **Cursors**: `next_episode_id` (EP:75), `next_page_offset` (QD:218), `next_after_id`/`next_after_namespace` (SG:105,156).
11. **Identity triple** `principal_id`+`session_id`(+`session_genesis`): MM:32-33 (2 of 3), EP:19-21, LIB:120+125 — memory context lacks `session_genesis`, episodes require it.
12. **Content blobs**: `value` (MP:122), `episode` (EP:39), `push` (EP:50), `document` (RD:44, WF:14), `texts` (EMM:168), `points`/`payload` (QD) — all private JSON, all differently named.
13. `on_behalf_of` (EP:145, JSON-only) duplicates `principal_id` by construction — checked equal, never a struct field.

## (b) Mis-normalized fields

- **Unbounded/untyped strings where enum/id belongs**: `MemoryState.backend/isolation`, `EmbeddingModelState.provider/status`, `Qdrant* .status/optimizer_status/distance/datatype`, `WorkflowExecutionRecord.status`, `RestoreReceipt.durability`, `ListModelsOutput.source`, `ExportOutput.encoding`, `Cozo*.access_level/event/distance/dtype` — all free `String`.
- **`path` that is not a path** (CZ:57 BackupInput) renamed `archive` in receipts — rename to `archive_label`/`archive_name`.
- **Hash fields without algorithm in the name**: `content_hash`, `digest`, `model_digest`, `checksum` (algorithm entirely unspecified), vs well-formed `sha256`. Canonical: `*_sha256` = lowercase hex; note EP validation (EP:127-131) accepts uppercase hex too.
- **Timestamps as bare `String`**: `created_at`/`updated_at` (MP), `creation_time` (QD:245), `generated_at` (SG:55), `compacted_at` (MP:249) — all RFC3339 by convention only; `accepted_at` (WF) is at least parse-validated. `LIB` records use `i64` unix seconds instead (LIB:104-105,135-140) — two timestamp regimes in one crate.
- **Untyped `Value` where schema exists**: Qdrant `points`/`results`/`vectors_config`/`payload_schema` (score, point id, payload invisible to contract); Cozo `RelationRows.rows`; memory `config`; GraphProvenance `inputs` values.
- **`operation_id: u64`** (QD:235) vs string operation ids in provenance receipts — collision risk.
- **`size` for bytes and dimensions both**: `SnapshotDescription.size` (bytes, QD:242) vs `QdrantVectorParams.size` (dimensions, QD:88).
- **Redundant derived fields stored**: `EpisodeEmbeddingRecord.dimensions` vs `vector.len()`; `push_id` encoding the revision; identity fields duplicated inside episode/push JSON documents.
- **Abbreviations**: `src`/`dst` (SG:71,73), `idx` (CZM:106), `m`/`m0`/`ef_*` (QD/CZ HNSW).

## (c) Tagged-routing / provenance relevance

- **No tag field exists in this slice.** The nearest tag-like surfaces: Qdrant point `payload` (untyped) + `CreatePayloadIndexInput.field_name` (QD:198-200) — payload keys are the de-facto routeable tags; memory namespace `name`+`config` (MP:107,112); Cozo relation `name`. Tagged routing must key off payload/namespace names today, or a tag field must be added.
- **Hash-only provenance anchors already present**: episode/knowledge `content_hash` (B2/B4), archive/restore `sha256` receipts (CZM:125-146), graph `digest` receipt (SG:88), memory owner key hash (MM:548, never exposed), `model_digest` (EM:69).
- **Provenance chain for vectorization**: episode `content_hash` + embedding `{provider, model, provider_model, dimensions, config_revision}` (B3) + CAS `compare_exchange_episode_embedding` (EP:804) is a complete "who vectorized what, with which model revision" record — but has **no timestamp, no actor, no status/failure state**, and insert rejects pre-vectorized episodes (EP:502-505: embeddings are always derived, never primary).
- **Purge gaps**: memory has full purge (delete_namespace/clear_namespace/delete_entry/compact, MP:276-363); Qdrant has delete_points/delete_collection; **episodes have no delete/purge function at all** in op-cozo-store (no matches) — episode purge would currently require relation-level Cozo access. Cozo `compact` (MM:60) is physical only, `reclaimed_bytes` always `None`.
- **Receipt-shaped outputs** usable by tagged routing outcomes: `ArchiveReceipt`/`RestoreReceipt` (CZM), `GraphSnapshotReceipt` (SG), memory `deleted_entries` counts, Qdrant `OperationMutationOutput` (beware u64 `operation_id`).
- **Sensitivity boundary is enforced in code, not just convention**: memory contents never projected (MM:3-5), `RuntimeDocumentRecord` Debug-redacted (RD:47-53), `VoyageEmbedParams`/api_key non-serializable (EM:129-135), episode relations only on the dedicated owner handle (EP:438-439). Private = `value/episode/push/document/texts/vector(s)/points.payload`; hash-only = `content_hash/sha256/digest/owner key`; everything else above is public metadata.