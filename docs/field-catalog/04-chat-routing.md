# Slice 4 — Chat, router, and model routing

Source: FldChatRouting field inventory (read-only, cited). Integrated verbatim; canonical proposals are proposals, not renames applied to code.

Sealed/live verification is pending (door unreachable) — all rows below are from source declarations only.

Canonical rules used throughout: unified `snake_case`; `*_id` = opaque identifier, `*_hash` = lowercase hex SHA-256 (64 chars) unless explicitly noted otherwise, timestamps = RFC3339 UTC string, sets sorted/deduped. No source is prioritised; conflicts are flagged, not resolved in favour of one plugin.

## 1. `project` — `ChatMode`, context selection, resolved context, project record
*(core record structs consumed by `chat_router`; included because `GetOwnAgentRouteOutput.mode: ChatMode` and `memory_namespace`/`read_only`/`effective_capabilities`/`allowed_agents` live here)*

|field_as_found|source (file:line)|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|Personal/Project/ControlPlane/Accountability/NotebookLm/UiRender (`ChatMode`)|project.rs:17-27|enum (`snake_case` serde)|`mode`; UI `MODES` list in json_render_gallery.rs:32-38|chat_mode|enum: `personal\|project\|control_plane\|accountability\|notebook_lm\|ui_render`|public|Default `Personal`. `UiRender` = no memory (project.rs:31-33)|
|mode|project.rs:314,328,338,351; chat_router.rs:408|`ChatMode`|—|chat_mode|`ChatMode`|public|Same field in `ResolveContextInput`, `ChatContextSelection`, `SelectContextInput`, `ResolvedChatContext`, `GetOwnAgentRouteOutput`|
|project_id|project.rs:118,223,239,248,255,262,287,297,316,330,340,353|`String` / `Option<String>`|`id` (Provider), `container_id`|project_id|opaque id (UUID in incus.rs usage)|public|Required in `Project`, optional in context inputs — optionality is mode-dependent (`Project` mode requires it, project.rs:66-69)|
|personal_namespace|project.rs:319,332,342|`Option<String>`|`memory_namespace`, `namespace_template`|personal_namespace|bounded string `[a-z0-9_-]{1,64}` (project.rs:37-47)|private|User-defined sub-namespace under principal|
|session_id|project.rs:103,352; 353 (`session_id: String` in `ResolvedChatContext`)|`String`|`session` (GenerationOwner, chat_router_generation.rs:162)|session_id|opaque id|private|Doc (project.rs:49-51): "session id is the principal's account-lifetime identity" — conflicts with per-login session meaning elsewhere, see §7(a)|
|memory_namespace|project.rs:354|`String`|`memory_namespaces` (Vec, chat_router), `namespace_template`|memory_namespace|path string `session/{session}/…` or `project/{project_id}`, **empty string** for `UiRender` (project.rs:76)|private|Derived by `memory_namespace_for()` (project.rs:52-78); empty-string sentinel, see §7(b)|
|read_only|project.rs:355|`bool`|schema `readOnly=true` extensions (many records); `read_only` bool in unrelated plugins|read_only|`bool`|public|In `ResolvedChatContext` only; derived from `ProjectRole::Reader` path, not a stored flag|
|effective_capabilities|project.rs:356|`BTreeSet<String>`|`capability_ceiling`, `capability_id`, grants|effective_capability_set|sorted set of exact capability strings `cap.*@v1`|public (capability ids); grants themselves hash-only|Computed: `capability_ceiling ∩ caller grants`, Reader filtered to `project.read`/`chat.context`/`memory.read` (project.rs:154-173). **Tags never grant** — this is the grant intersection|
|capability_ceiling|project.rs:128,211|`BTreeSet<String>`|`effective_capabilities`|capability_ceiling|sorted set of capability strings|public|Ceiling only; "effective authority also intersects the caller's exact grants" (project.rs:127)|
|allowed_agents|project.rs:129,213|`BTreeSet<String>`|`agent_alias`, `agent_id`, `selected_agent`|allowed_agent_id_set|sorted set of agent ids|public|Membership tested against `known_agent_ids()` = `agent_type` strings (project.rs:363-368)|
|selected_agent|project.rs:130,358|`Option<String>`|`agent_alias`, `agent_id` (SelectAgentInput, project.rs:263)|selected_agent_id|opaque agent id \| null|public|Singular selection; must be member of `allowed_agents`|
|agent_id|project.rs:263 (SelectAgentInput)|`String`|`agent_alias`, `selected_agent`|agent_id|opaque id|public|Input-only spelling of the same concept|
|owner_session_id|project.rs:120|`String`|`invited_by_session_id`, `invited_session_id`, `session_id`|owner_session_id|opaque id|private||
|container_id|project.rs:121|`String`|`project_id`|container_id|opaque id|public|Incus container, distinct from `project_id`|
|status (`ProjectStatus`)|project.rs:95-98,122|enum `Provisioning\|Ready`|`status` (String elsewhere)|project_status|enum|public|Typed here, free `String` in LLM records — inconsistency|
|role (`ProjectRole`)|project.rs:87-91,104|enum `Reader\|Contributor\|Admin`|—|project_role|enum|public|Drives `read_only`/`effective_capabilities` derivation|
|members / invitations|project.rs:125-126|`Vec<ProjectMember>` / `Vec<ProjectInvitation>`|—|project_member_set / project_invitation_set|array of records|private (contains session ids)||
|invitation_id / invited_session_id / invited_by_session_id|project.rs:109-111|`String`|—|invitation_id / invited_session_id / invited_by_session_id|opaque ids|private||
|expires_at|project.rs:113|`i64`|`started_at`/`ended_at` (String)|expires_at|RFC3339 UTC string|private|Epoch integer, unit unstated — mis-normalised, §7(b)|
|created_at|project.rs:131|`i64`|`started_at` (String)|created_at|RFC3339 UTC string|public|Epoch integer vs RFC3339 strings everywhere else|
|title|project.rs:119,209|`String`|—|title|string (bounded)|private||

## 2. `chat_router` — state, route resolution, chat I/O, generation

### ChatRouterState / ObservedChatRouterState (chat_router.rs:324-392, 508-514)

|field_as_found|source (file:line)|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|status|chat_router.rs:340,509|`String`|enum `ProjectStatus`, `GenerationStatus`, `RunStatus`|router_status|enum (values undeclared)|public|Unbounded string where an enum belongs|
|selected_provider|chat_router.rs:344,510,797,819|`String`|`provider`, `provider_id`, `id`, `route`|selected_provider_id|opaque provider id|public|State/output spelling|
|selected_model|chat_router.rs:348,511,805,820|`String`|`model`, `model_id`, `chat_model`|selected_model_id|opaque model id|public||
|catalog_model_binding|chat_router.rs:328|`Option<ResolveCatalogModelBindingOutput>`|—|catalog_model_binding|record \| null|public (credential-free)|Read-only, excluded from public projection (chat_router.rs:325-328, 1838)|
|native_config|chat_router.rs:354|`Option<JsonValue>`|—|native_config|JSON object \| null|private|Masked native config; "never part of shared public-state policy" (chat_router.rs:349-351)|
|model_assignments|chat_router.rs:358|`ModelAssignments`|—|model_assignment_set|record|public|Per-capability model ids, see below|
|memory_namespaces|chat_router.rs:313,378|`Vec<MemoryNamespaceOption>`|`memory_namespace` (singular, resolved)|memory_namespace_template_set|array of `MemoryNamespaceOption`|private|Plural = declared templates; singular elsewhere = resolved namespace — same stem, different thing|
|ovs_routing / obfuscation / vectorization / qdrant_retrieval / cozo_retrieval|chat_router.rs:77-89|`String`|`model_id`|ovs_routing_model_id / obfuscation_model_id / vectorization_model_id / qdrant_retrieval_model_id / cozo_retrieval_model_id|opaque model id|public|Field names name a *capability*, values are model ids (`SetOvsRoutingModelInput.model_id`, chat_router.rs:825)|
|namespace_template / keys / purpose / identity_link_key (`MemoryNamespaceOption`)|chat_router.rs:235-247|`String`/`Vec<String>`|`memory_namespace`|namespace_template / namespace_key_set / purpose / identity_link_key|string / string set|private|`identity_link_key` binds namespace to container/user identity|
|privacy_policy.sensitive_fields, email_storage_rule (`PrivacyOptions`)|chat_router.rs:287-291|`Vec<String>`/`String`|—|sensitive_field_set / email_storage_rule|string set / enum|private|Declares which fields "must not be rendered casually"|
|transport.* (`LlmTransport`: dbus_object, grpc_target, incus_container, browser_surface, rest_aliases, policy_source)|chat_router.rs:44-68|`String`/`Vec<String>`|`uri`, `endpoint`, `listener`|dbus_object_path / grpc_target / container_target / browser_surface / rest_alias_set / policy_source|string|public|`incus_container` retained for compat, defaults to `"host"` sentinel (chat_router.rs:49-56)|

### Route / selection method contracts (chat_router.rs:402-498, 759-831)

|field_as_found|source (file:line)|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|mode (`GetOwnAgentRouteOutput`)|chat_router.rs:408|`super::project::ChatMode`|—|chat_mode|`ChatMode` enum|public|Caller's *persisted* mode; input is empty — no agent/identity/path may be supplied (chat_router.rs:398-402)|
|reference|chat_router.rs:409,429|`String`|—|route_reference|string (format undeclared)|public|Ambiguous: neither id, hash, nor URI by name; appears in both route output and catalog binding|
|model|chat_router.rs:410,430,471,777|`String`|`model_id`, `selected_model`, `chat_model`, `reported_model`|model_id|opaque model id|public|In `ChatInput` it is "model id **or route hint**" (chat_router.rs:469-471) — overloaded|
|uri|chat_router.rs:412,431|`String`|`endpoint`, `grpc_target`, `listener`|inference_endpoint_uri|HTTPS URI, no credentials/query/fragment|public|Explicitly credential-free (chat_router.rs:411)|
|provider_id / model_id (`ResolveCatalogModelBindingInput/Output`, `SetProviderInput`, `SetModelInput`, `SetSelectionInput`)|chat_router.rs:419-420,427-428,490,497,811-812|`String`|`provider`, `id`, `model`|provider_id / model_id|opaque ids|public|The `*_id` spelling exists only on these inputs/outputs|
|oauth|chat_router.rs:435|`bool`|—|oauth|bool|public|Verified subscription profile flag; not inferred from empty URI|
|catalog_revision|chat_router.rs:436|`u64`|`revision`, `document_revision`, `episode_revision`|catalog_revision|u64 monotonic|public||
|hint (`ResolveRouteInput`)|chat_router.rs:444|`String`|`route`, `model` (as hint)|route_hint|string (enum-like: code/fast/local/reasoning/vision…)|public|Free string for an enumerated concept|
|provider (`ChatInput`, `ListModelsInput`)|chat_router.rs:468,483|`String`|`provider_id`, `id`, `route`, `aliases`|provider_id|opaque id (empty = use `selected_provider`)|public|"Provider id, route, **or alias**. Empty uses selected" (chat_router.rs:466) — three namespaces in one field|
|conversation_id (`ChatInput`)|chat_router.rs:475|`Option<String>`|`conversation_id` (bridge-minted UUIDv7 in chat_response)|conversation_correlation_label|string \| null|private|Caller correlation label, "never identity" (chat_router.rs:472-475) — **not** the chat_response conversation id despite identical name|
|message / messages[].role / messages[].content (`ChatInput`/`ChatMessage`)|chat_router.rs:453-465|`String`/`Vec<ChatMessage>`|`message` (chat_response)|message_text / message_set; role → message_role enum|private|OpenAI-compatible role string (`system\|user\|assistant`) untyped|
|content / provider / model / finish_reason / usage (`ChatOutput`)|chat_router.rs:772-781|`String`/`JsonValue`|`message`, `response`|response_text / provider_id / model_id / finish_reason / token_usage|usage = provider-specific JSON object|content private; provider/model/finish_reason public|Usage is untyped `JsonValue` — no canonical token fields (contrast `tokens_in/out` in ctl `RunRecord`)|

### Generation (chat_router_generation.rs:41-157)

|field_as_found|source (file:line)|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|agent_alias|chat_router_generation.rs:42,112|`String`|`agent_id`, `selected_agent`, `allowed_agents`|agent_id|opaque id (validated id charset, gen.rs:48)|public|Third spelling for "agent identifier"|
|job_id|chat_router_generation.rs:60,65,110,155|`String`|—|generation_job_id|opaque id ("never a fabricated native task ID", gen.rs:109-110)|public||
|prompt|chat_router_generation.rs:43|`String`|—|prompt_text|bounded string|private|Validated non-empty/bounded (gen.rs:49-52); test asserts prompt must not persist (generation_tests.rs:10)|
|prompt_digest|chat_router_generation.rs:120|`String`|—|prompt_hash|lowercase hex hash (algorithm/encoding unstated in field doc)|hash-only|Provenance commitment to private prompt; must specify `*_hash` algorithm|
|source_schema_hash|chat_router_generation.rs:121|`String`|`canvas_schema_hash`, `schema_hash`|source_schema_hash|lowercase hex SHA-256|hash-only|Provenance: schema the generation was produced under|
|reported_provider / reported_model|chat_router_generation.rs:117-119|`Option<String>`|`provider`, `provider_id`, `model`, `provider_model`|reported_provider_id / reported_model_id|opaque id \| null|public|Self-reported by native backend — weaker provenance than bridge-verified ids|
|status (`GenerationStatus`)|chat_router_generation.rs:74-82,113|enum snake_case|—|generation_status|enum `queued\|running\|cancel_requested\|completed\|cancelled\|failed\|interrupted`|public|Properly typed, unlike router `status: String`|
|failure (`GenerationFailure`)|chat_router_generation.rs:94-103,125|enum snake_case|—|generation_failure|enum \| null|public||
|output / committed_chunks|chat_router_generation.rs:114-115|`String`/`u32`|—|output_text / committed_chunk_count|string / u32|private (output text)||
|created_at / updated_at|chat_router_generation.rs:122-123|`i64`|RFC3339 strings elsewhere|created_at / updated_at|RFC3339 UTC string|public|Epoch int, unit unstated|
|revision / document_revision / generation_revision / deleted_revision / expected_revision|chat_router_generation.rs:65,111,131,137,143,154-156|`u64`|`catalog_revision`, `episode_revision`|revision (per-record, qualified by record name)|u64 monotonic|public|Four names for the monotonic-revision concept across this slice|

## 3. `op-llm` / `llm_projection` (common/llm_projection.rs) — shared by `chat_router` + `antigravity`

### Provider (llm_projection.rs:15-56)

|field_as_found|source (file:line)|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|id|llm_projection.rs:19|`String`|`provider_id`, `provider`, `selected_provider`|provider_id|opaque id|public|Bare `id` vs `*_id` elsewhere — primary synonym conflict|
|route|llm_projection.rs:23|`String`|—|provider_route_tag|string|public|"Routing tag used by the classifier" — a **tag**, not a grant (tagged-routing input)|
|kind|llm_projection.rs:27|`String`|`kind` (ModelRoute)|provider_kind|enum `local\|provider\|orchestrator\|policy\|virtual\|cli\|router` (doc-only)|public|Enum values only in doc comment|
|aliases|llm_projection.rs:31|`Vec<String>`|`rest_aliases`|provider_alias_set|string set|public|Why `ChatInput.provider` accepts "id, route, or alias"|
|endpoint|llm_projection.rs:35|`String`|`uri`, `grpc_target`, `listener`|provider_endpoint_uri|URI string|public; private if credentials embedded|No credential-free guarantee stated (contrast `uri` in chat_router)|
|auth|llm_projection.rs:39|`String`|—|provider_auth_method|enum (undeclared)|private (method name only; never the secret)||
|sdk|llm_projection.rs:42|`String`|`sdk_package`, `schema_crate`|provider_sdk|string|public|Three names for "owning package" across this file|
|oscal_source / source|llm_projection.rs:47,51|`String`|—|oscal_source|registry source string|public|Both carry near-identical doc comments (llm_projection.rs:44-51) — duplicate fields|
|description|llm_projection.rs:55|`String`|—|description|string|public||

### ModelRoute (llm_projection.rs:61-106)

|field_as_found|source (file:line)|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|hint|llm_projection.rs:65|`String`|`hint` (ResolveRouteInput)|route_hint|enum-like string|public|Tagged-routing qualifier|
|provider|llm_projection.rs:69|`String`|`provider_id`, `id`, `selected_provider`|provider_id|opaque id|public||
|upstream_provider|llm_projection.rs:73|`String`|`provider`|upstream_provider_id|opaque id|public|Proxied-through orchestrator|
|transport|llm_projection.rs:77|`String`|—|transport|enum `direct\|loopback\|auto\|dbus` (doc-only)|public||
|model|llm_projection.rs:81|`String`|`model_id`, `selected_model`|model_id|opaque id|public||
|kind|llm_projection.rs:85|`String`|`kind` (Provider)|route_kind|enum `chat\|router\|multimodal\|embedding\|policy\|orchestrator` (doc-only)|public||
|status / status_reason / available|llm_projection.rs:89-97|`String`/`String`/`bool`|`status` (String), typed status enums elsewhere|route_status (enum) / status_reason / available|enum / string / bool|public|`available` duplicates what `status` should encode|
|api_key|llm_projection.rs:101|`Option<JsonValue>`|—|api_key|secret string \| null — **should not be in a projected record at all**|private (secret)|"API key placeholder, null when not configured" — secret-adjacent field in a D-Bus projection; worst sensitivity offender in this slice|
|source|llm_projection.rs:105|`String`|`oscal_source`|oscal_source|string|public||

### Router / SelectorPolicy / StructuredOutput / LlmProjection (llm_projection.rs:111-367)

|field_as_found|source (file:line)|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|provider / model (`Router`)|llm_projection.rs:115,119|`String`|`provider_id`, `model_id`|router_provider_id / router_model_id|opaque ids|public|Classifier hosting|
|endpoint (`Router`)|llm_projection.rs:123|`String`|`uri`|router_endpoint_uri|URI|public||
|scope / role (`Router`)|llm_projection.rs:127,131|`String`|—|router_scope / router_role|enum (doc: `classifier\|context_aware_allocator`)|public||
|emits|llm_projection.rs:135|`Vec<String>`|—|emitted_tag_set|string set|public|**Tagged routing**: tags the classifier emits; inputs to tag→control resolution, never grants|
|classification_rules|llm_projection.rs:143|`JsonValue`|`parameters`, `env_vars` (JsonValue)|classification_rule_set|typed JSON (currently untyped)|public|"Mapping task kinds to routes" as raw JSON — should be a typed record for tagged routing|
|default_effort / effort_weight / cost_weight / latency_weight / health_weight (`SelectorPolicy`)|llm_projection.rs:212-224|`String`/`f64`×4|—|default_effort_class / effort_weight / cost_weight / latency_weight / health_weight|enum-like string / f64 ≥ 0|public|Pure selector weights; `default_effort="medium"` (llm_projection.rs:227-229)|
|sdk / config_object / extractor / response_schema / pydantic_source / ui_renderer / required / response_mime_type / schema_validation / refusals_as_none (`StructuredOutput`)|llm_projection.rs:296-332|mixed|`response_format`, `response_mode`, `message_type`|structured_output_* (per-field prefix)|typed record|public|**Response-format** authority for LLM side; `response_schema: JsonValue`, `schema_validation: String`, `response_mime_type: String` all untyped where enums/schema belong|
|providers / model_routes / router / tools / config_schema / ui_surfaces / structured_output (`LlmProjection`)|llm_projection.rs:342-366|`Vec<…>`/records|flattened into `ChatRouterState.catalog` (chat_router.rs:385-387)|same, snake_case plural|arrays/records|public (except nested `api_key`)|Flattened, so these appear as top-level chat_router state fields|
|parameters / response_schema (`LlmTool`)|llm_projection.rs:161,165|`JsonValue`|—|tool_parameter_schema / tool_response_schema|JSON Schema object|public||
|path / name / schema / purpose (`UiSurface`)|llm_projection.rs:254-269|`String`|`dbus_object`, `route`|ui_surface_path / …|string|public|Another `path`-like field; `purpose` names renderer stack (json-render etc.)|

## 4. `chat_response` — committed records + method contracts

### ChatTurn / ConversationSummary / ResponseCommittedSignal / state (chat_response.rs:148-365)

|field_as_found|source (file:line)|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|principal_id|chat_response.rs:153,245; chat_response_methods.rs:527,590|`String`|`on_behalf_of`, `owner_principal_id`, `responder_principal_id`|principal_id|opaque id (validated ≤512 B)|private (`x-opdbus-private`, readOnly)|Scope owner; conversation spans logins, scope = principal (chat_response.rs:6-9)|
|session_id|chat_response.rs:158,266; methods.rs:528,591|`String`|`owner_session_id`, `session` |session_id|opaque id|private|Provenance only — explicitly *not* the scope|
|session_genesis|chat_response.rs:163,271; methods.rs:529,592|`String`|—|session_genesis_hash|lowercase hex SHA-256 (test uses `"ab"×32`, methods.rs:765)|private|Named `*_genesis` not `*_hash`, but is a 64-hex binding; canonical name should say hash|
|conversation_id|chat_response.rs:168,250,301; methods.rs:53,62,73…|`String`|caller label in chat_router `ChatInput.conversation_id`; synthetic `trigger:{id}` in ctl|conversation_id|canonical lowercase hyphenated UUIDv7 (validated, methods.rs:328-333)|public (per schemars "public")|Bridge-minted|
|turn_id|chat_response.rs:173,302; methods.rs:75,84,120,150,181,228|`String`|—|turn_id|UUID|public|For a response turn, = the human turn it answers (chat_response.rs:170-173) — id reuse across record kinds|
|response_id|chat_response.rs:183,303; methods.rs:106,170|`String`|—|response_id|UUID|public|"Bridge-minted id of this committed turn" — on `ChatTurn` it identifies the turn itself, on outputs the response; near-synonym of `turn_id`|
|responder_principal_id|chat_response.rs:188; methods.rs:645|`String`|`principal_id`, `on_behalf_of`|responder_principal_id|opaque id|public per schemars (chain actor)|The agent/human author; part of turn_hash preimage|
|turn_index|chat_response.rs:178,304; methods.rs:77,108,172,232,331|`u64`|`first_turn_index`|turn_index|u64, zero-based|public||
|opened_at / committed_at / started_at / updated_at|chat_response.rs:193,198,261,291; methods.rs:55,238|`String`|i64 epochs in project/generation|opened_at / committed_at / started_at / updated_at|RFC3339 UTC string|public|Correctly RFC3339-described here — the model for canonical form|
|prev_turn_hash / turn_hash / head_turn_hash / genesis_hash|chat_response.rs:203,208,276,286,305-306|`String`|`content_hash`|prev_turn_hash / turn_hash / head_turn_hash / genesis_hash|lowercase hex SHA-256|hash-only|Hash chain: `turn_hash = SHA256(canonical ["opdbus.chat_response.turn.v1", prev_turn_hash, conversation_id, turn_index, turn_id, content_hash, responder_principal_id])` (methods.rs:304-323); genesis domain `opdbus.chat_response.genesis.v1` (methods.rs:17-18)|
|content_hash|chat_response.rs:213,307; ctl_plane_chatbot.rs:363|`String`|—|content_hash|lowercase hex SHA-256 of canonical JSON content|hash-only|Public commitment to private content — core provenance primitive|
|request_event_id / request_event_hash|chat_response.rs:218,223,311-312|`u64`/`String`|`event_id`/`event_hash` (spec)|request_event_id / request_event_hash|u64 / lowercase hex hash|hash-only (hash); id public|Global chain linkage — ties turn to the admitted method call; the receipt anchor for provenance|
|canvas_schema_hash|chat_response.rs:228; methods.rs:113|`Option<String>`|`source_schema_hash`, `schema_hash`|canvas_schema_hash|lowercase hex SHA-256 \| null|hash-only|Sealed json_render schema the canvas was validated against|
|content (`ResponseContent`)|chat_response.rs:234; 68-139|`enum {UserResponse, Clarification, Refusal, HumanMessage}` (tag=`kind`)|`message`, `output`, `decision_output`|response_content|tagged enum|private (`x-opdbus-private`)|**Response format**: discriminated by `kind`; variants carry `message`/`question`/`reason` + `message_type`/`category` enums|
|message_type (`MessageType`)|chat_response.rs:45-50,77; methods.rs:91|enum `Success\|Info\|Warning\|Error` snake_case|`response_mode`, `kind`|message_type|enum|private (inside content)|Presentation only|
|category (`RefusalCategory`)|chat_response.rs:55-61; methods.rs:157|enum `Dangerous\|NotAllowed\|MissingInfo\|PolicyViolation\|NotSupported`|`exit_reason`, `outcome_class`, `failure`|refusal_category|enum|private||
|owner_key (`ResponseCommittedSignal`)|chat_response.rs:299-300; methods.rs:234|`String`|`owner_key` (different derivation in ctl doorbell)|owner_key|lowercase hex SHA-256 — **derivation must be fixed in canonical spec**|hash-only|`sha256(principal_id)` here (chat_response.rs:299); ctl doorbell uses `sha256(principal, session, genesis)` — same name, different preimage, §7(a)|
|revision|chat_response.rs:309,364; methods.rs:111,175,203,309|`u64`|`document_revision`, `episode_revision`, `catalog_revision`|scope_revision|u64 monotonic|public|Durable scope revision; also the wake `seq`|
|title|chat_response.rs:256; methods.rs:46|`Option<String>`|—|title|bounded string (≤256 B)|private||
|conversations / turns / conversation_count / turn_count / *_truncated (`ChatResponseState`)|chat_response.rs:327-358|`BTreeMap<String,…>`/`u64`/`bool`|—|same|map/count/bool|private maps; counts public|Private principal-scoped snapshot, absent from public projections|

### Method inputs/outputs (chat_response_methods.rs:40-253)

|field_as_found|source (file:line)|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|message (`SubmitTurnInput`)|methods.rs:64|`String`|`prompt`, `goal_text`, `content`|message_text|bounded string (≤1 MiB)|private||
|response_mode|methods.rs:66|`String`|`message_type`, `kind`, `response_mime_type`, `StructuredOutput`|response_mode|enum `text\|application` (doc-only, methods.rs:65)|private|Unbounded string for a 2-value enum — prime mis-normalisation|
|message / related_actions / canvas (`RespondToUserInput`)|methods.rs:89-100|`String`/`Vec<String>`/`Option<Map>`|`tools_consulted`, `actions`, `tools`|message_text / related_action_set / canvas_spec|private|Canvas validated against sealed json_render profile; invalid canvas rejects whole response|
|question / context / options / required_fields (`RequestClarificationInput`)|methods.rs:125-143|`String`/`Option<String>`/`Vec<String>`|—|clarification_question / clarification_context / option_set / required_field_set|private||
|reason / alternatives (`CannotPerformInput`)|methods.rs:155-163|`String`/`Vec<String>`|`reason` (RunRecord), `status_reason`|refusal_reason / alternative_set|private||
|turn_hash / canvas_schema_hash (`RespondToUserOutput`)|methods.rs:109,113|`String`/`Option<String>`|—|turn_hash / canvas_schema_hash|hex SHA-256|hash-only|Commit receipt returned to responder|
|pending_turns / pending_count; owner_principal_id, opened_at (`PendingTurnInfo`)|methods.rs:228-252|`Vec<PendingTurnInfo>`/`u64`|`principal_id`|pending_turn_set / pending_turn_count; owner_principal_id|private (`owner_principal_id`); owner_key hash-only|`owner_key` here = "sha256 of principal" for routing the response (methods.rs:233)|
|conversations_truncated / turns_truncated / first_turn_index (`List/GetConversationOutput`)|methods.rs:201,219-220|`bool`/`u64`|—|same|bool/u64|public||

## 5. `ctl_plane_chatbot` (`tched_router.Chat`) — episodes, knowledge, config, methods

### ReasoningEpisode / RunRecord / KnowledgePush (ctl_plane_chatbot.rs:246-513)

|field_as_found|source (file:line)|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|principal_id / session_id / session_genesis|ctl_plane_chatbot.rs:248-252,448-453|`String`|same names in chat_response (identical semantics)|principal_id / session_id / session_genesis_hash|opaque ids; genesis = hex SHA-256|private (readOnly, `[PII]`)|Verified scope triple; also `EpisodeScope` (op-cozo-store ctl_plane_chatbot_episodes.rs:18-21)|
|on_behalf_of|ctl_plane_chatbot.rs:254|`String`|`principal_id`, `responder_principal_id`|on_behalf_of_principal_id|opaque id|private|Delegation provenance; currently always == `principal_id` (methods.rs:78,124,603)|
|episode_id|ctl_plane_chatbot.rs:261|`String`|`push_id`, `turn_id`, `job_id`|episode_id|UUIDv7|public|Time-ordering id|
|started_at / ended_at / duration_ms|ctl_plane_chatbot.rs:268-282|`String`/`String`/`u64`|`queued_ms`, `duration_ms` (RunRecord)|started_at / ended_at / duration_ms|RFC3339 UTC / u64 ms|public|Described "ISO-8601" not RFC3339 (ctl:263,270) — tighten canonical type|
|trigger (`Trigger`)|ctl_plane_chatbot.rs:181-187,289|enum `Goal\|ToolResult\|Interrupt\|Replan\|SystemEvent`|`trigger` (String in KnowledgePush), `trigger_id`|trigger_kind|enum|public|Typed here, free string in KnowledgePush (ctl:480, example `ErrorAssistance`)|
|exit_reason (`ExitReason`) / outcome_class (`OutcomeClass`)|ctl_plane_chatbot.rs:191-211,296,335|enums snake_case|`failure`, `category`, `status`|exit_reason / outcome_class|enums|public|Feed Significance rule|
|goal_text / reasoning_summary / decision_output|ctl_plane_chatbot.rs:304,312,328|`Option<String>`|`prompt`, `message`, `summary`, `goal_text` (methods CompletedChatEpisode)|goal_text / reasoning_summary / decision_output|bounded strings \| null|private (`x-opdbus-private`)|Omitted before embedding when `pii_flagged` (ctl:364-370; methods.rs:347-357)|
|tools_consulted|ctl_plane_chatbot.rs:320|`Vec<String>`|`related_actions`, `actions`, `tools`|tool_call_set (`plugin.method` strings)|string set|public|Method names, bounded 256 (methods.rs:24)|
|confidence|ctl_plane_chatbot.rs:343|`Option<f64>`|`score`, `relevance_score`|confidence|f64 ∈ [0,1] \| null|public||
|plugin_id|ctl_plane_chatbot.rs:350|`Option<String>`|`embedding_plugin`, `llm_plugin`, method `plugin.method`|plugin_id|opaque id \| null|public|e.g. `"chat_router"`, `"persona"` (methods.rs:91,137)|
|conversation_id|ctl_plane_chatbot.rs:357; methods.rs:28,92|`Option<String>`|UUIDv7 conversation_id (chat_response); caller label (chat_router)|conversation_id|opaque id \| null — **format conflicts**: free-form, and synthesised as `trigger:{trigger_id}` for agent runs (methods.rs:92)|public|Weakest id in slice; cannot join to chat_response conversations reliably|
|content_hash|ctl_plane_chatbot.rs:363,512|`String`|`content_hash` (turn content)|episode_content_hash / push_content_hash|lowercase hex SHA-256 (computed with `content_hash=""` preimage convention, ctl:359,508)|hash-only|Self-referential hash convention must be canonicalised|
|pii_flagged|ctl_plane_chatbot.rs:370|`bool`|`sensitive`, `is_secret`, `x-opdbus-private`|pii_flagged|bool|public|Conservative default from native Chat; gates embedding PII omission|
|run (`RunRecord`): status/trigger_id/topic_ref/queued_ms/duration_ms/tokens_in/tokens_out/actions[].method,ok,duration_ms,error/reason|ctl_plane_chatbot.rs:383-420|`RunStatus` enum + scalars|`topic_ref` ← blake3 digest; `method` = `plugin.method`|run_status / trigger_id / topic_hash (**blake3, not SHA-256** — rename or re-hash) / queued_ms / duration_ms / token_count_in / token_count_out / action_set|status public; topic_ref hash-only; error/reason private|`topic_ref` is the one non-SHA256 hash in slice (ctl:409); token fields are the only typed usage record (contrast `ChatOutput.usage: JsonValue`)|

### Config / state / method contracts (ctl_plane_chatbot.rs:33-63,518-666; ctl_plane_chatbot_methods.rs:154-328)

|field_as_found|source (file:line)|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|embedding_plugin / qdrant_collection / vector_dims / output_dtype / input_type / dedup_window_hrs / queue_alert_threshold / nesting_policy / vectorization_enabled (`CtlPlaneChatbotConfig`)|ctl_plane_chatbot.rs:36-62|`String`/enums/`u32`/`bool`|`llm_plugin`, `chat_model` (state duplicates)|same snake_case; `vector_dims` → embedding_dimension_count|typed; `vector_dims: MatryoshkaDim` serialises as `"256"\|"512"\|"1024"\|"2048"` (numeric strings, ctl:100-109)|public|Several are `const`/readOnly reserved-legacy with no implemented behaviour (ctl:38,50,54,58) — dead-weight candidates|
|llm_plugin / chat_model (`CtlPlaneChatbotState`)|ctl_plane_chatbot.rs:547,554|`String`|`selected_provider`, `selected_model`, `provider`/`model`|— (deprecated; canonical = chat_router `selected_provider_id`/`selected_model_id`)|string|public|Explicitly legacy: "Native Chat routing is selected through the tched_router contract" (ctl:542,550) — chatbot holds **no** model selection of its own|
|episodes / knowledge (state maps), episode_count, knowledge_count, *_truncated, episode_revision|ctl_plane_chatbot.rs:521-539|`BTreeMap<String,…>`/`u64`/`bool`|`revision` (chat_response)|episode_map / knowledge_push_map / counts / scope_revision|private maps|Private identity-scoped; absent from public projections|
|running / reasoning_active / embedding_queue_depth / last_vectorized_at / vector_id / significance|ctl_plane_chatbot.rs:632-665|`bool`/`u32`/`Option<String>`/`Option<Significance>`|—|same; last_vectorized_at → RFC3339|public|Mostly "reserved, no producer implemented" (ctl:641-659)|
|level / rule (`Significance`, `SignificanceLevel`)|ctl_plane_chatbot.rs:214-217,424-439|enum **without** `rename_all` (serialises `Contextual`/`Signal`) + `Option<String>`|`SignificanceLevel` example `&"Signal"` (ctl:427)|significance_level|enum `contextual\|signal`|public|Only PascalCase-serde enum in slice; rule string is provenance for the classification decision|
|push_id / source_id / created_at / trigger / trigger_reason / relevance_score / content / suggested_action (`KnowledgePush`)|ctl_plane_chatbot.rs:460-512|`String`/`f64`/`JsonValue`/`Option<String>`|`episode_id`, `source_id` (idempotency)|push_id (zero-padded revision string, ctl:454-456) / source_push_id / created_at (RFC3339) / trigger_name / trigger_reason / relevance_score / knowledge_content / suggested_action|ids public; content/trigger_reason/suggested_action private; content_hash hash-only|`source_id` is the cross-retry idempotency key — provenance-relevant|
|expected_revision / config / revision / changed / publication / diagnostic (`SetConfig*`)|ctl_plane_chatbot_methods.rs:172-194|`u64`/`CtlPlaneChatbotConfig`/`bool`/`ConfigPublication` enum/`Option<String>`|`expected_revision` (GenerationRevisionInput)|expected_revision / config / revision / changed / publication_status / diagnostic|public (config may embed private fields)|CAS config write|
|episode (`ClassifySignificanceInput/Output`)|ctl_plane_chatbot_methods.rs:199-206|`ReasoningEpisode`/`Significance`|—|episode / significance|record|private (episode)|Pure decision function — a routing-decision record candidate|
|after_episode_id / limit / next_episode_id (`ListEpisodes*`, `QueryContext*`)|ctl_plane_chatbot_methods.rs:212-224,287,303|`Option<String>`/`u32`|cursor vs `first_turn_index`|after_episode_id / result_limit / next_episode_id|opaque cursor / u32|public|Keyset pagination|
|owner_key / topic / listener / password_prefixes / knowledge_paths (`GetDoorbellOutput`)|ctl_plane_chatbot_methods.rs:250-258|`String`/`Vec<String>`|`owner_key` (chat_response derivation)|owner_key (**same conflict as §4**) / wake_topic / listener_uri / password_prefix_set / knowledge_path_set|owner_key hash-only; topic/listener public; password_prefixes public (prefixes only, methods.rs:255)|`topic = opdbus/wake/<owner_key>` — tagged-routing wake channel; owner_key is also MQTT client id|
|query / scan_limit / score / results / scanned_episodes / indexed_episodes / unvectorized_episodes / incompatible_embeddings / more_episodes (`QueryContext*`)|ctl_plane_chatbot_methods.rs:283-309|`String`/`u32`/`f32`/`Vec<ContextMatch>`/`bool`|—|query_text / scan_limit / match_score / …counts|query private; counts public|Retrieval diagnostics|
|vector / dimensions / model / provider_model / provider / config_revision (`VectorizeOutput`)|ctl_plane_chatbot_methods.rs:320-328|`Vec<f32>`/`u32`/`String`/`Option<String>`/`EmbeddingProvider` (typed enum)/`u64`|`provider` (String), `model` vs `provider_model`|embedding_vector / embedding_dimension_count / embedding_model_id / provider_model_id / embedding_provider / config_revision|vector private; ids public|**Only typed-provider field in slice** (`EmbeddingProvider` enum vs `String` everywhere else); `model` vs `provider_model` duplicates the model/model_id split|

## 6. (a) Duplicate / synonym fields across the slice

1. **Provider identity (4 spellings):** `Provider.id` (llm_projection.rs:19) = `provider_id` (chat_router.rs:419,490,811) = `provider` (llm_projection.rs:69,115; chat_router.rs:468) = `selected_provider` (chat_router.rs:344). Plus `upstream_provider`, `reported_provider`, `provider_model`, typed `EmbeddingProvider`.
2. **Model identity (5 spellings):** `model` (llm_projection.rs:81,119; chat_router.rs:410,471,777) = `model_id` (chat_router.rs:420,497) = `selected_model` (chat_router.rs:348) = `chat_model` (ctl_plane_chatbot.rs:554, legacy) = `reported_model`/`provider_model` (generation.rs:119; ctl_methods.rs:325). `ModelAssignments.*` fields (ovs_routing…) are a sixth, inverted form: capability-named fields holding model ids.
3. **Agent identity (4 spellings):** `agent_alias` (generation.rs:42) = `agent_id` (project.rs:263) = `selected_agent` (project.rs:130,358) = `agent_type` (source of `known_agent_ids()`, project.rs:366); set form `allowed_agents`.
4. **Mode (3 concepts, 1 name):** `ChatMode` enum (project.rs:17) vs `response_mode: String` (methods.rs:66, rendering text/application) vs `MessageType`/`ResponseContent.kind` (presentation/response format) vs `StructuredOutput.response_mime_type`/`schema_validation`. "Mode" also appears as persisted-mode in `GetOwnAgentRouteOutput` with no input.
5. **Conversation id (3 semantics, 1 name):** bridge-minted UUIDv7 (chat_response) vs caller correlation label "never identity" (chat_router.rs:475) vs free-form/`trigger:{trigger_id}` synthetic (ctl_plane_chatbot_methods.rs:92).
6. **`owner_key` (1 name, 2 derivations):** `sha256(principal_id)` (chat_response.rs:299; methods.rs:233) vs `sha256(principal, session, genesis)` (ctl_plane_chatbot_methods.rs:249). Joining on this name across plugins is unsafe.
7. **Session (2 semantics):** account-lifetime principal identity (project.rs:49-51) vs verified per-login session provenance (chat_response.rs:155, ctl:249). Same `session_id` name.
8. **Revision (5 names):** `revision`, `document_revision`, `generation_revision`, `episode_revision`, `catalog_revision`, plus `deleted_revision`/`expected_revision` — all u64 monotonic counters scoped differently.
9. **URI/endpoint (5 names):** `uri`, `endpoint` (×2), `grpc_target`, `listener`, `dbus_object`/`path` — only `uri` carries a credential-free guarantee.
10. **Hash fields:** `content_hash` (turn content vs episode self-hash vs push self-hash — different preimages, ctl:359,508 vs methods.rs:300), `turn_hash`/`head_turn_hash`/`genesis_hash`/`prev_turn_hash`, `canvas_schema_hash` vs `source_schema_hash` vs `schema_hash`, `session_genesis` (a hash not named `*_hash`), `topic_ref` (blake3, not SHA-256), `prompt_digest` (algorithm unstated), `owner_key` (a hash named as a key).
11. **Status/kind/type:** typed enums (`GenerationStatus`, `RunStatus`, `ProjectStatus`, `Trigger`, `ExitReason`, `OutcomeClass`) vs untyped `String` (`ChatRouterState.status`, `Provider.kind`, `ModelRoute.kind/status/transport`, `Router.role/scope`, `KnowledgePush.trigger`).
12. **Content text:** `message`, `content`, `output`, `goal_text`, `decision_output`, `summary`, `prompt`, `reasoning_summary` — all private authored text with plugin-local names.
13. **Capability:** `capability_ceiling` vs `effective_capabilities` vs `capability_id` (ConfigurableOptionRpc, chat_router.rs:115) vs sealed capability strings in method decls (`cap.software.tched-router.*@v1`).

## 7. (b) Mis-normalised fields

- **Unbounded `String` where an enum/id/hash belongs:** `response_mode` (2 values), `Provider.kind`, `ModelRoute.kind/status/transport`, `Router.role/scope/emits` classification inputs, `ChatRouterState.status`, `KnowledgePush.trigger` (example `ErrorAssistance`, PascalCase free text), `ChatInput.provider` (id|route|alias union), `ChatInput.model` (id|hint union), `hint`, `classification_rules: JsonValue`, `ChatOutput.usage: JsonValue`, `api_key: Option<JsonValue>`.
- **Mixed-case conventions:** `SignificanceLevel` serialises PascalCase (`Contextual`/`Signal`, ctl:214-217,427) while every sibling enum is `snake_case`; `MatryoshkaDim` serialises as numeric strings (`"1024"`); `ChatMode::NotebookLm` → `notebook_lm` is fine but UI `MODES` list duplicates it as literals (json_render_gallery.rs:32).
- **Ambiguous names:** `reference` (chat_router.rs:409,429 — neither id/hash/URI), `route` (classifier tag, llm_projection.rs:23, vs `ModelRoute` record vs `ResolveRouteOutput.route`), `source` vs `oscal_source` duplicate pair (llm_projection.rs:47-51), `id` bare (Provider), `turn_id` reused as response's answered-turn id, `response_id` vs `turn_id` on the same record.
- **Timestamp split:** RFC3339 strings (chat_response, ctl episodes) vs unitless `i64` epochs (`Project.created_at`, `ProjectInvitation.expires_at`, `GenerationJob.created_at/updated_at`).
- **Empty-string sentinels:** `memory_namespace = ""` for `UiRender`; `content_hash = ""` as hash preimage convention; `ChatInput.provider/model = ""` meaning "use selected".
- **Secret in projection:** `ModelRoute.api_key: Option<JsonValue>` (llm_projection.rs:101).
- **Legacy/dead fields carried in live state:** ctl `llm_plugin`, `chat_model`, `qdrant_collection`, `dedup_window_hrs`, `queue_alert_threshold`, `nesting_policy`, `embedding_queue_depth`, `last_vectorized_at`, `vector_id` — documented as reserved/unimplemented (ctl:542-659), plus chat_router `incus_container` compat field.
- **`read_only` conflation:** schema-extension `readOnly=true` (record immutability, dozens of fields) vs the single data field `ResolvedChatContext.read_only` (project.rs:355, role-derived) vs namespace `read_only` elsewhere — canonical spec must separate `read_only` (access) from record immutability.

## 8. (c) Tagged-routing & provenance relevance

- **Tag inputs (never grants):** `Provider.route` + `aliases` + `kind`, `ModelRoute.hint/kind`, `Router.emits` (emitted tags), `Router.classification_rules` (task-kind → route, currently raw JSON — needs typing for tag→control resolution), `SelectorPolicy` weights/`default_effort`, `UiSurface.path/schema`. OSCAL linkage is already on every LLM field via `x-oscal-subid` (e.g. llm_projection.rs:18,22) and `ConfigurableOptionRpc.capability_id`/`subid` (chat_router.rs:115-119).
- **Grant side (the only authority):** `effective_capabilities = capability_ceiling ∩ verified caller grants`, Reader-filtered (project.rs:154-173); `allowed_agents`/`selected_agent` gate which agent may act; method-level sealed capabilities (`cap.software.tched-router.agent.route.self.read@v1` etc., chat_router.rs:2176-2186). `GetOwnAgentRouteInput` is empty by design — route derives from verified caller + persisted `ChatMode`, so tags/inputs cannot steer it.
- **Receipts / chain linkage:** `request_event_id` + `request_event_hash` on every committed turn and on `ResponseCommittedSignal` (chat_response.rs:218-223,311-312) — the bridge to the global event chain / `Outcome{operation_id,event_hash}` in the design spec. `ResponseCommittedSignal` is deliberately content-free (ids + hashes + `owner_key` + `revision`) for public verification.
- **Hash-chain provenance:** conversation `genesis_hash` → `prev_turn_hash`/`turn_hash` chain with domain tags `opdbus.chat_response.genesis.v1`/`turn.v1` (methods.rs:17-18,286-323); `content_hash` commits to private content; `canvas_schema_hash`/`source_schema_hash` commit to the schema used; episode/push `content_hash` (self-hash with empty-field convention) and `prompt_digest` commit to private inputs. `RunRecord.topic_ref` is blake3 — either canonicalise to SHA-256 `*_hash` or explicitly exempt it.
- **Decision records:** ctl `Significance{level, rule}` + `ClassifySignificanceInput/Output` (pure, auditable decision + the rule that fired), `Project::effective_capabilities()` derivation, and `RunRecord.actions[]` (`plugin.method`, `ok`, `duration_ms`, `error`) form the per-episode decision/action timeline; `tools_consulted`/`related_actions` are the lighter-weight equivalents in episodes/turns.
- **Routing channels:** `PendingTurnInfo.owner_key` routes responses to responders; ctl doorbell `topic = opdbus/wake/<owner_key>` + `knowledge_paths` (methods.rs:250-258) is the wake/routing surface — but its `owner_key` derivation differs from chat_response's (§6.6), which must be fixed before either can serve as a cross-plugin routing key.
- **Identity provenance triple:** `(principal_id, session_id, session_genesis)` recurs identically in chat_response records, ctl episodes/pushes, and `EpisodeScope` — the strongest existing candidate for the canonical provenance scope key; `on_behalf_of` adds delegation, currently always self.