# Slice 3 — Provenance stream: event chain, receipts, execution tracker

Source: FldProvenance field inventory (read-only, cited). Integrated verbatim; canonical proposals are proposals, not renames applied to code.

# Provenance / Event-Chain Field Inventory — `FldProvenance`

Sealed/live verification is pending (door unreachable).

Sensitivity is for the **ETH anchor boundary**, per Main's definition: `public` = value itself may be published/anchored; `hash-only` = raw value never leaves OP-DBUS, only its hash / Merkle proof / batched root may be exposed; `private` = neither raw value nor an individually linkable hash may be exposed (low-entropy or identifying values where a lone hash is dictionary-/correlation-linkable).

Canonical rules applied: `snake_case`, singular, `*_hash` = lowercase hex SHA-256 (algorithm-explicit where source differs), `*_id` = opaque id, timestamps = RFC 3339 UTC.

## 1. `ChainEvent` — `crates/op-state-store/src/event_chain.rs:124-200`

|field_as_found|source (file:line)|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|---|
|event_id|event_chain.rs:126|u64|id (ExecutionRecord), change_id|event_id|u64 (monotonic)|public|Anchorable only as batch range member; sequential|
|prev_hash|event_chain.rs:128|String|prev_hash (ExecutionRecord)|prev_event_hash|lowercase hex hash, algorithm-tagged|hash-only|MD5 today (see §8); chain link|
|event_hash|event_chain.rs:130|String|exec_hash, hash()|event_hash|lowercase hex hash, algorithm-tagged|hash-only|H(prev_hash‖canonical_payload) :13, :258-306; MD5 32-hex|
|timestamp|event_chain.rs:132|DateTime\<Utc\>|started_at, arrival_timestamp (i64), timestamp_millis (u64)|occurred_at|RFC3339 UTC|hash-only|Per-event time is correlation metadata; only in root on-chain|
|actor_id|event_chain.rs:134|String|initiated_by, by, actor|actor_id|opaque id|private|Stable actor :135-136; linkable across events|
|session_id|event_chain.rs:138|Option\<String\>|session_id (ActionOrigin)|session_id|opaque id|private|Audit metadata, never authority :135-136|
|session_genesis|event_chain.rs:142|Option\<String\>|genesis_hex, session_genesis|session_genesis_hash|lowercase hex hash|private|Arrival proof, never identity/grant/digest :139-140; raw genesis may be linkable|
|sealed_id_digest|event_chain.rs:148|Option\<String\>|sealed_id_digest (SessionContext)|sealed_id_digest|blake3 lowercase hex (64)|hash-only|Doc: blake3 of SID1 envelope, commitment w/o credential :143-148; blake3 at mutation_engine.rs:460-464. Individually linkable across a session — anchor only via batch root|
|capability_id|event_chain.rs:150|Option\<String\>|capability (DenyReason)|capability_id|opaque id|private|Low-entropy capability names: lone hash is dictionary-linkable|
|plugin_id|event_chain.rs:152|String|plugin_id, plugin|plugin_id|opaque id|public|Routing key|
|schema_version|event_chain.rs:154|String|schema_version, version|schema_version|string (semver-like, unbounded)|public|In canonical hash :270|
|op|event_chain.rs:156|OperationType|operation_type (record projection), op|operation|enum snake_case|hash-only|Enum :35-60 incl. unbounded `Custom(String)` :59; projected as `format!("{:?}")` in mutation_engine.rs:6753|
|target|event_chain.rs:158|String|object_path, target|target_path|string (path/selector)|private|Object path may reveal private state layout|
|tags_touched|event_chain.rs:160|Vec\<String\>|tags_touched (StateChange)|tags_touched|array\<tag_id\>|private|Computed from schema :19,159; tag names may be private/low-entropy. **Method attempt/terminal events hard-code `Vec::new()`** (method_receipt_audit.rs:491,696) — routing signal absent there|
|decision|event_chain.rs:162|Decision (Allow/Deny)|decision, success, outcome, status|decision|enum {allow,deny}|hash-only|Enum :65-68; provenance-critical|
|deny_reason|event_chain.rs:164|Option\<DenyReason\>|code, reason, failure_kind|deny_reason|tagged enum|hash-only|Enum :107-120 carries `tag`, `wrapper_id`, `constraint`, `message`, `capability`, `errors: Vec<String>`, `field`, `reason` — free-text `message/reason/errors` are private content inside a hash-only envelope|
|input_patch_hash|event_chain.rs:167|String|args_digest, sanitized_args_digest, json_args_footprint|input_hash|lowercase hex hash, algorithm-tagged|hash-only|Compatibility digest only; new hashes from payload itself :165-166|
|input_payload|event_chain.rs:171|Option\<Value\>|input, json_args, args, input_patch|input_payload|canonical JSON (bounded/redacted)|hash-only|Bounded/redacted :21, :1030-1098 (64 KiB / 48 KiB preview caps); raw never anchored|
|legacy_args_digest (`json_args_footprint`)|event_chain.rs:174-179|Option\<String\>|json_args_footprint|input_hash (legacy)|lowercase hex hash|hash-only|Read-only legacy compat, never written new :172-173|
|method_name|event_chain.rs:183|Option\<String\>|method (receipt/journal), method_name|method_name|string|hash-only|None for non-method events :180-181|
|result_effective_hash|event_chain.rs:185|Option\<String\>|result_digest, effective_hash|result_hash|lowercase hex hash, algorithm-tagged|hash-only|Post-compile result hash :22. **Overloaded**: terminal audit writes `result_digest` into it (method_receipt_audit.rs:709) yet all callers pass `None` (:624-626,652,662), so it is currently never populated on that path|
|db_delta_hash|event_chain.rs:187|Option\<String\>|—|db_delta_hash|lowercase hex hash|hash-only|Incremental verification :186|
|snapshot_ref|event_chain.rs:189|Option\<String\>|snapshot_id|snapshot_id|opaque id / hash|hash-only|Unbounded string where a hash/id belongs|
|action_origin|event_chain.rs:192|Option\<ActionOrigin\>|generation_origin, facade_origin, routing_admission|action_origin|tagged enum {instructed,autonomous,reactive}|private|Enum :77-102: `by:String`, `session_id`, `prompt_ref` (doc says "hash", type is bare String :84-85 — ambiguous), `reasoning_ref`, `confidence:f32`, `trigger:String`. None = unknown/legacy :190-191. Core autonomy provenance for tagged routing|
|user_id|event_chain.rs:195|Option\<String\>|by, initiated_by|user_id|opaque id|private|Human initiator :193-194|
|conversation_id|event_chain.rs:199|Option\<String\>|session_id, trace_id|conversation_id|opaque id|private|Groups why→what→who :196-199|

Related enums in same file: `OperationType` :35-60, `Decision` :65-68, `ActionOrigin` :77-102, `DenyReason` :107-120 (above).

## 2. Merkle / snapshot / proof records — `event_chain.rs`

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|---|
|hash (MerkleNode)|event_chain.rs:390|String|event_hash, root|merkle_node_hash|lowercase hex hash|hash-only|Recursive node :389-393|
|left / right|event_chain.rs:391-392|Option\<Box\<MerkleNode\>\>|—|left_child / right_child|MerkleNode|hash-only|Tree structure only|
|batch_root (EventBatch)|event_chain.rs:399|String|root (MerkleProof)|merkle_root|lowercase hex hash|public|**The ETH-anchorable value**; root of event hashes :398|
|first_event_id / last_event_id|event_chain.rs:401-402|u64|event_id|first_event_id / last_event_id|u64|public|Batch range :400|
|prev_batch_root|event_chain.rs:404|Option\<String\>|prev_hash|prev_merkle_root|lowercase hex hash|public|Batch chaining :403|
|timestamp (EventBatch)|event_chain.rs:406|DateTime\<Utc\>|timestamp|finalized_at|RFC3339 UTC|public|Batch finalization|
|event_count|event_chain.rs:408|usize|—|event_count|u64|public|—|
|event_hash / event_id (MerkleProof)|event_chain.rs:452,454|String / u64|—|event_hash / event_id|hash / u64|hash-only|Proof subject :450-459|
|siblings|event_chain.rs:456|Vec\<(String,bool)\>|—|merkle_sibling_hashes|array of (hash, is_right)|hash-only|Proof path; verify :463-475|
|root (MerkleProof)|event_chain.rs:458|String|batch_root|merkle_root|lowercase hex hash|public|Expected root :457|
|snapshot_id (StateSnapshot)|event_chain.rs:482|String|snapshot_ref|snapshot_id|lowercase hex hash|hash-only|Hash of content :481; built :528-537|
|at_event_id|event_chain.rs:484|u64|event_id|at_event_id|u64|hash-only|—|
|plugin_id / schema_version (Snapshot)|event_chain.rs:486,488|String|—|plugin_id / schema_version|string|hash-only|In-snapshot copies|
|stub_hash|event_chain.rs:490|String|plugin_core_hash|stub_hash|lowercase hex hash|hash-only|Currently aliased to effective_hash :511|
|immutable_wrappers_hash|event_chain.rs:492|String|—|immutable_wrappers_hash|lowercase hex hash|hash-only|Hash of `[]` placeholder :512|
|tunable_patch_hash|event_chain.rs:494|String|tunable_hash|tunable_patch_hash|lowercase hex hash|hash-only|Hash of `{}` placeholder :513|
|effective_hash|event_chain.rs:496|String|result_effective_hash|effective_state_hash|lowercase hex hash|hash-only|:495|
|timestamp (Snapshot)|event_chain.rs:498|DateTime\<Utc\>|—|created_at|RFC3339 UTC|hash-only|—|
|state (Snapshot)|event_chain.rs:500|Value|new_value, result|state_payload|canonical JSON|private|Actual state data :499 — never anchored, not even individually hashed on-chain if low-entropy|
|valid / events_verified / batches_verified / errors (ChainVerificationResult)|event_chain.rs:1011-1014|bool/usize/usize/Vec\<String\>|—|same|bool/u64/u64/array\<string\>|public|Verification summary; `errors` text should stay local (hash-only if exported)|
|tag / is_immutable / violations / total_events_checked (TagImmutabilityProof)|event_chain.rs:1020-1023|String/bool/Vec\<u64\>/usize|tag (DenyReason)|tag_id / is_immutable / violating_event_ids / events_checked_count|…|private (tag, violations); hash-only (counts)|Direct tagged-routing provenance artifact :1017-1024|

Hash primitives (all in `event_chain.rs`): `compute_hash` = **MD5** hex :1101-1104, `compute_hash_str` = MD5 :1107-1109, `compute_hash_pair`/Merkle :1112-1141. Every `*_hash` in §1–2 is therefore 32-hex MD5, not SHA-256.

## 3. Method receipts — `crates/op-state-store/src/method_receipt.rs`

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|---|
|event_id (MethodEventProof)|method_receipt.rs:50|u64|event_id|event_id|u64 ≥1|hash-only|Schemars range(min=1) :49|
|event_hash (MethodEventProof)|method_receipt.rs:52|String|event_hash|event_hash|lowercase hex, 32 (MD5)|hash-only|Length/regex pinned `^[0-9a-f]{32}$` :51; `valid_event_hash` :106-114 explicitly documents MD5, "not a plugin's BLAKE3 hash"|
|operation_id (GetMethodReceiptInput)|method_receipt.rs:71|String|change_id, operation_id|operation_id|UUID (36, regex-pinned :67-70)|private|Caller-scoped lookup key; linkable|
|operation_id (MethodReceiptRecovery)|method_receipt.rs:82|String|—|operation_id|UUID pinned :78-81|private|Caller-private recovery :74|
|status (Recovery)|method_receipt.rs:83|MethodRecoveryStatus|status, outcome, success|recovery_status|enum {completed,failed,unknown,not_found} :57-62|hash-only|—|
|plugin_id (Recovery)|method_receipt.rs:84|Option\<String\>|plugin_id|plugin_id|opaque id|hash-only|—|
|method (Recovery)|method_receipt.rs:85|Option\<String\>|method_name, method|method_name|string|hash-only|Naming split vs ChainEvent.method_name|
|attempt / terminal (Recovery)|method_receipt.rs:86-87|Option\<MethodEventProof\>|attempt_event_id/_hash (terminal payload)|attempt_event / terminal_event|MethodEventProof|hash-only|Attempt→terminal provenance pair|
|result_digest (Recovery)|method_receipt.rs:88|Option\<String\>|result_effective_hash|result_hash|lowercase hex hash (blake3 where produced)|hash-only|Enforced `None` in journal decode (method_receipt_audit.rs:292) and terminal callers pass `None` — declared but currently always absent; doc :621-623 (method_receipt_audit.rs) bars digests of secret-bearing results from shared audit|
|success (MethodExecutionReceipt)|method_receipt.rs:94|bool|success, decision|success|bool|hash-only|Must be true + outcome=completed :121-124|
|operation_id (Receipt)|method_receipt.rs:95|String|change_id|operation_id|UUID canonical (validated :129-133)|private|—|
|outcome|method_receipt.rs:96|String|status, decision|outcome|enum {completed,…} (currently bare String)|hash-only|Unbounded string where enum belongs; compared to literal `"completed"` :122|
|event_id / event_hash|method_receipt.rs:97-98|u64 / String|—|attempt_event_id / attempt_event_hash|u64 / MD5 hex|hash-only|Renamed in terminal payload (method_receipt_audit.rs:682-683)|
|terminal_event_id / terminal_event_hash|method_receipt.rs:99-100|u64 / String|terminal (proof)|terminal_event_id / terminal_event_hash|u64 / MD5 hex|hash-only|Ordering `terminal > event_id` enforced :134-137|
|plugin_id / method|method_receipt.rs:101-102|String|plugin_id / method_name|plugin_id / method_name|string|hash-only|Attribution check :125-128|
|result|method_receipt.rs:103|Value|result, output, new_value|result_payload|schema-validated JSON ≤16 MiB :7,167-177|private|Domain result owned by plugin schema :2; raw never anchored. Only digest (currently suppressed) could be hash-only|
|code / message (UnsupportedBeforeEffect)|method_receipt.rs:16-17|&'static str|code, deny_reason|refusal_code / refusal_message|closed static strings|public|Static diagnostics exclude private args/backend text :9-12|
|operation_id / refusal (AuditedMethodRefusal)|method_receipt.rs:36-37|Uuid / UnsupportedBeforeEffect|—|operation_id / refusal|UUID / struct|private / public|Public refusal only after durable record :30-32|

## 4. MutationEngine envelopes — `crates/op-grpc-bridge/src/mutation_engine.rs` (record/envelope structs only)

### `StateChange` :88-102
|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|---|
|change_id|mutation_engine.rs:89|String|operation_id, id|operation_id|UUID|private|This is the `operation_id` source: journal uses `call.change.change_id` (method_receipt_audit.rs:333,475)|
|event_id|mutation_engine.rs:90|u64|event_id|event_id|u64|hash-only|Back-filled from attempt event (method_receipt_audit.rs:511)|
|plugin_id|mutation_engine.rs:91|String|—|plugin_id|opaque id|public|—|
|object_path|mutation_engine.rs:92|String|target|target_path|string|private|Becomes ChainEvent.target (method_receipt_audit.rs:490)|
|change_type|mutation_engine.rs:93|ChangeType :105-113|op, operation_type|operation|enum|hash-only|Second operation taxonomy, overlaps OperationType|
|member_name|mutation_engine.rs:94|Option\<String\>|method_name|member_name|string|private|Signal/property member|
|old_value / new_value|mutation_engine.rs:95-96|Option\<OwnedValue\> / OwnedValue|input_payload, args, result|old_value / new_value|canonical JSON|private|`new_value` is the audited args source (method_receipt_audit.rs:463,478); raw never anchored|
|tags_touched|mutation_engine.rs:97|Vec\<String\>|tags_touched|tags_touched|array\<tag_id\>|private|Present here, dropped (empty) when building method ChainEvents|
|event_hash|mutation_engine.rs:98|String|event_hash|event_hash|MD5 hex|hash-only|—|
|timestamp|mutation_engine.rs:99|DateTime\<Utc\>|timestamp|occurred_at|RFC3339 UTC|hash-only|Footprint converts to `timestamp_millis: u64` :6735|
|actor_id|mutation_engine.rs:100|String|actor_id|actor_id|opaque id|private|—|
|source|mutation_engine.rs:101|ChangeSource {DBus,Grpc,Internal} :116-120|source, origin|change_source|enum|hash-only|Transport provenance; distinct from ActionOrigin|

### `AdmittedMethodCall` :126-141 (internal, post-admission)
|field_as_found|source|declared_type|canonical_name_proposal|sensitivity|notes|
|---|---|---|---|---|---|
|plugin_id / method / capability_id / actor_id / session_id / session_genesis|mutation_engine.rs:127,128,130-133|String/String/Option\<String\>×4|plugin_id / method_name / capability_id / actor_id / session_id / session_genesis_hash|private|Verified identity metadata, does not mint identity/grant :122-125|
|json_args|mutation_engine.rs:129|String|input_payload (canonical JSON string)|private|Opaque args; Debug deliberately not derived :124-125|
|parsed_value|mutation_engine.rs:134|OwnedValue|parsed_args|private|Parsed form of json_args — duplicate representation|
|admitted_schema|mutation_engine.rs:135|PluginSchema|admitted_schema|hash-only|Supplies schema_version + method `subid` to journal (method_receipt_audit.rs:340-347)|
|generation_origin / facade_origin / routing_admission / bound_oracle|mutation_engine.rs:136,137,140,138|Option\<…\>|provenance objects|private|Tagged-routing/admission provenance; bound_oracle contributes `_verified_transport` to attempt payload (method_receipt_audit.rs:480-482)|
|change|mutation_engine.rs:139|StateChange|state_change|private|Envelope above|

### `GenesisStamp` :433-445 / `SessionContext` :449-456
|field_as_found|source|declared_type|canonical_name_proposal|sensitivity|notes|
|---|---|---|---|---|---|
|session_id|mutation_engine.rs:434,451|String|session_id|private|—|
|wireguard_pubkey|mutation_engine.rs:435,452|String|wireguard_public_key|private|Public key material — identifying, do not anchor individually|
|genesis_hex|mutation_engine.rs:436,450|String|session_genesis_hash|private|Hex string, format unpinned|
|trace_id|mutation_engine.rs:437|String|trace_id|private|Correlation id (cf. ExecutionRecord.trace_id)|
|sealed_id|mutation_engine.rs:440|String|sealed_id (raw envelope)|private|Full SID1 credential envelope :438-440 — **never** leaves; only digest below may|
|sealed_id_digest|mutation_engine.rs:455|Option\<String\>|sealed_id_digest (blake3 hex)|hash-only|Stamped on every session mutation, "never the credential itself" :453-455|
|arrival_timestamp / head_timestamp_at_arrival|mutation_engine.rs:441,444|i64|arrived_at|private|Unix seconds as bare i64 — format/unit split vs DateTime\<Utc\>|
|chain_head_at_arrival / catalog_hash_at_arrival|mutation_engine.rs:442-443|String|chain_head_hash / catalog_hash|hash-only|Arrival-time chain/catalog provenance; algorithm unpinned in name/type|

### `MutationResult` :6645-6651 / `MutationError` :6654-6657
|field_as_found|source|declared_type|canonical_name_proposal|sensitivity|notes|
|---|---|---|---|---|---|
|success / event_id / event_hash|mutation_engine.rs:6646-6648|bool/u64/String|same / event_id / event_hash|hash-only|Mirror of receipt fields — duplicate envelope|
|result|mutation_engine.rs:6649|Option\<OwnedValue\>|result_payload|private|Raw domain result|
|error {code: ErrorCode :6660-6666, message: String}|mutation_engine.rs:6650,6655-6656|struct|error_code / error_message|hash-only / private|`message` is free text — private content|

### `AuditEventRecord` projection (constructed :6743-6760; type imported :43-44 from snowball_plugin)
Fields as built: `event_id` :6745, `event_hash` :6746, `prev_hash` :6747, `timestamp` **String (RFC3339)** :6748, `actor_id` :6749, `capability_id` (defaulted `""`) :6750, `plugin_id` :6751, `method_name` (defaulted `""`) :6752, `operation_type` (**`format!("{:?}")` Debug string**) :6753, `target` :6754, `tags_touched` :6755, `decision` (**Debug string**) :6756, `input_patch_hash` :6757, `result_effective_hash` (defaulted `""`) :6758. Sensitivity follows §1; note lossy `""`-for-None and Debug-string enums. The footprint path (`event_to_footprint` :6678-6740) instead embeds the **full serialized ChainEvent** as payload :6724 — private fields leave in payload form on that surface, metadata duplicates actor/session/capability/method/event ids :6680-6721.

## 5. `OperationJournal` — `crates/op-grpc-bridge/src/method_receipt_audit.rs:241-249`

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|---|
|format|method_receipt_audit.rs:242|String|format|format|const `"__method_operation_v1"` :21|hash-only|Namespace tag; journal key domain-separated by it :267-271|
|actor_id|method_receipt_audit.rs:243|String|actor_id|actor_id|opaque id|private|Scope-checked on decode :284; also hashed into journal key via blake3 :267-271|
|session_id|method_receipt_audit.rs:244|Option\<String\>|session_id|session_id|opaque id|private|Absent-vs-empty distinguished in key scope :265-266|
|schema_version|method_receipt_audit.rs:245|String|schema_version|schema_version|string|hash-only|From admitted schema :340|
|subid|method_receipt_audit.rs:246|String|subid|method_subid|string (e.g. `obs.fixture.read@v1` :781)|public|OSCAL-style routed method identifier — primary tagged-routing key in the journal|
|sanitized_args_digest|method_receipt_audit.rs:247|String|args_digest, input_patch_hash|sanitized_args_hash|blake3 lowercase hex (64)|hash-only|blake3 of canonicalized `new_value` :462-466; test fixture is 64×`a` :782. Comment :460-461: hash only after redaction, never of unredacted request|
|receipt|method_receipt_audit.rs:248|MethodReceiptRecovery|receipt|receipt|struct (§3)|private|Embedded recovery envelope; `result_digest` forced None :292|
|(derived) journal key|method_receipt_audit.rs:258-272|String|`__method_operation_v1:{blake3(scope)}:{operation_id}`|journal_key|private|Blake3 over (namespace, actor_id, session_id) — correctly keeps actor/session out of the raw key while operation_id stays plaintext in-key|

Terminal/attempt payload fields built inline: `operation_id`, `args_digest`, `method`, `args` (attempt :474-479, plus `_verified_transport` :481) and `operation_id`, `attempt_event_id`, `attempt_event_hash`, `status`, `result_digest`, `code` (terminal :680-687) — same canonicalization as §3; `status`/`code` are bare `&str` literals ("completed"/"failed"/"unknown" :673,718-722) where enums belong.

## 6. `ExecutionRecord` + `ExecutionTiming` — `crates/op-execution-tracker/src/record.rs:100-135, 46-59`

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|---|
|id|record.rs:102|String|event_id, change_id, operation_id|execution_id|UUID v4 string (:140)|private|Also exposed via `execution_id()` alias :216-218|
|trace_id|record.rs:104|String|trace_id (GenesisStamp)|trace_id|opaque id (defaults to id :143)|private|Correlation across systems :103|
|tool_name|record.rs:106|String|plugin_id+method, tool()|tool_name|string|hash-only|`tool()` alias :221-223|
|input|record.rs:108|Value|input_payload, args|input_payload|JSON|private|Raw input; hashed into exec_hash :364-371|
|output|record.rs:110|Value|result, new_value|output_payload|JSON|private|Raw output|
|status|record.rs:112|ExecutionStatus :24-38|status, outcome, decision|execution_status|enum {pending,running,completed,failed,cancelled,timeout}|hash-only|Third status taxonomy|
|timing.started_at / ended_at|record.rs:48,50|DateTime\<Utc\> / Option|timestamp|started_at / ended_at|RFC3339 UTC|hash-only|ExecutionTiming :46-59|
|timing.monotonic_ns|record.rs:52|u128|—|monotonic_ns|u128 ns|hash-only|**Mis-set**: filled from wall-clock epoch nanos, not monotonic :65-68|
|timing.duration_ms / duration_ns / wallclock_ns|record.rs:54,56,58|u64/u128/u128|—|duration_ms (drop aliases)|u64 ms|hash-only|`duration_ns`/`wallclock_ns` self-described "compatibility aliases" :55-57; three fields for one duration|
|policy_id|record.rs:116|String|capability_id|policy_id|opaque id|private|Governing policy :115|
|plugin_core_hash|record.rs:118|String|stub_hash|plugin_core_hash|lowercase hex SHA-256 (where computed via hash_execution family)|hash-only|Determinism fingerprint :117; empty-string default :150|
|tunable_hash|record.rs:120|String|tunable_patch_hash|tunable_hash|lowercase hex hash|hash-only|:119|
|prev_hash|record.rs:122|String|prev_hash (ChainEvent)|prev_execution_hash|lowercase hex SHA-256|hash-only|Execution chain link :121|
|exec_hash|record.rs:124|String|event_hash, hash()|execution_hash|lowercase hex SHA-256 (64)|hash-only|SHA-256 over tool‖input‖output‖prev_hash **with no delimiters/domain separation** :364-371; `hash()` alias :226-228|
|output_summary|record.rs:126|Option\<String\>|result|output_summary|string ≤1000 chars (:180)|private|Truncated output text — content leak if exported|
|error|record.rs:128|Option\<String\>|message, deny_reason|error_message|string|private|Free-text errors :127, set from literals :190,200,210|
|success|record.rs:130|bool|success, decision|success|bool|hash-only|Redundant with status :129|
|initiated_by|record.rs:132|Option\<String\>|actor_id, user_id, by|actor_id|opaque id|private|User/session initiator :131|
|metadata|record.rs:134|HashMap\<String,String\>|metadata (footprint)|metadata|map\<string,string\>|private|Unbounded, untyped escape hatch|

## (a) Duplicate / synonym fields across this slice

- **Event identity:** `event_id` (ChainEvent/StateChange/receipts) vs `id`+`execution_id()` (ExecutionRecord) vs `change_id` (= operation_id in journal path).
- **Operation identity:** `operation_id` (receipts/journal/terminal payload) = `StateChange.change_id` (method_receipt_audit.rs:333) — same UUID, two names.
- **Chain hash:** `event_hash` (MD5) vs `exec_hash`/`hash()` (SHA-256) vs `snapshot_id` vs `batch_root`/`root`.
- **Input digest:** `input_patch_hash` (MD5) vs `args_digest`/`sanitized_args_digest` (blake3) vs legacy `json_args_footprint` — three algorithms, one concept.
- **Result digest:** `result_effective_hash` vs `result_digest` vs `effective_hash`; `result_digest` is written into `result_effective_hash` (method_receipt_audit.rs:709).
- **Actor:** `actor_id` vs `user_id` vs `initiated_by` vs `ActionOrigin::Instructed.by` vs `actor`.
- **Method:** `method_name` (ChainEvent/AuditEventRecord) vs `method` (receipts/journal/AdmittedMethodCall) vs `tool_name`.
- **Operation:** `op: OperationType` vs `operation_type` (Debug string) vs `change_type: ChangeType` vs `operation` (footprint).
- **Tunable/stub:** `tunable_patch_hash` vs `tunable_hash`; `stub_hash` vs `plugin_core_hash`.
- **Session provenance:** `session_genesis` vs `genesis_hex` vs `sealed_id`/`sealed_id_digest` (raw vs digest correctly split only in SessionContext).

## (b) Mis-normalized fields

1. **Hash algorithm chaos under one suffix:** `*_hash` = MD5 32-hex (all event_chain.rs hashes, :1101-1114, pinned by `valid_event_hash` method_receipt.rs:106-114), SHA-256 64-hex (record.rs:364-371), blake3 64-hex (sanitized_args_digest, sealed_id_digest). Canonical `*_hash` = lowercase hex SHA-256 is violated by the entire chain layer; no field carries an algorithm tag. MD5 for a tamper-evident chain is the headline issue.
2. **Enums degraded to strings:** AuditEventRecord `operation_type`/`decision` via `format!("{:?}")` (mutation_engine.rs:6753,6756); receipt `outcome` and journal `status`/`code` as bare strings/literals; `OperationType::Custom(String)` (:59) and `ChangeType` duplicate the taxonomy.
3. **Unbounded strings where id/hash/enum belongs:** `actor_id`, `capability_id`, `target`/`object_path`, `snapshot_ref`, `policy_id`, `plugin_core_hash`, `tunable_hash`, `schema_version`, `method` — contrast the pinned UUID/MD5 regexes in method_receipt.rs:51,67-81 (the good pattern).
4. **Ambiguous digest-typed-as-string:** `ActionOrigin.prompt_ref` documented as a hash (:84-85) but bare `String`; `chain_head_at_arrival`/`catalog_hash_at_arrival` unpinned hex.
5. **Lossy Option erasure:** `capability_id`/`method_name`/`result_effective_hash` become `""` instead of null in AuditEventRecord (:6750,6752,6758).
6. **Duplicate/alias fields:** ExecutionTiming `duration_ms`+`duration_ns`+`wallclock_ns` (+ mislabeled `monotonic_ns`, record.rs:65-68); ExecutionRecord `status`+`success`, `id`+`trace_id` defaulting equal (:143); AdmittedMethodCall `json_args`+`parsed_value`.
7. **Timestamp format split:** `DateTime<Utc>` vs RFC3339 String (:6748) vs `timestamp_millis: u64` (:6735) vs unix `i64` (:441,444) vs `u128` epoch-ns (record.rs:58).
8. **Dead/never-populated fields:** `result_digest` (enforced None, method_receipt_audit.rs:292; all terminal calls pass None) yet persisted in receipt/journal schemas; `tags_touched` on method events always empty (:491,696).

## (c) Tagged-routing / provenance relevance

- `tags_touched` is the routing tag set (ChainEvent :160, StateChange :97) and `TagImmutabilityProof` (:1019-1024) is the tag-scoped compliance artifact, but method-call events — the ones carrying `operation_id` — record **no tags**, so tag routing cannot be reconstructed for admitted methods from the chain alone; the journal's `subid` (method_receipt_audit.rs:246,341-347) is currently the only durable method-routing key.
- Provenance spine: `prev_hash`→`event_hash`→`batch_root`/`prev_batch_root`→MerkleProof (§2) is the only ETH-anchorable chain; `sealed_id_digest` (blake3 SID1 commitment) + `session_genesis` + `action_origin` + `capability_id` form the private-IP provenance tuple and must stay off-chain except inside the batched root.
- Attempt/terminal `MethodEventProof` pairs (method_receipt.rs:48-53) linked by `operation_id` give per-operation provenance; `args_digest` is correctly computed only over redacted/canonical args (method_receipt_audit.rs:460-466), and result digests are deliberately withheld from shared audit (:621-623) — any tagged-schema routing design should preserve both rules.