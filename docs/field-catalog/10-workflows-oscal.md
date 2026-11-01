# Slice 10 — Workflows, cron, cognitive, MCP, model registry, OSCAL

Source: FldWorkflows field inventory (read-only, cited). Integrated verbatim; canonical proposals are proposals, not renames applied to code.

Sealed/live verification is pending (door unreachable) — all rows below are source-declared types only, not live-blob-verified.

## 1. `workflows` — `workflows_plugin.rs` (`W=` `/srv/git/odbus-verified/crates/op-plugins/src/state_plugins/workflows_plugin.rs`)

### `WorkflowsState` / `StartWorkflowInput` / `QueryWorkflowsInput`

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|status|W:17|`String`|WorkflowSummary.status, WorkflowExecutionStatus|workflow_state_status|enum string|public|Unbounded; state default `"active"` W:152 vs execution-status enum W:70-81|
|workflows|W:21|`serde_json::Value`|`WorkflowSummary[]` in output|workflow_definition_list|array\<WorkflowSummary\>|private|Untyped in state, typed only on query output|
|config|W:25|`serde_json::Value`|—|workflow_scheduler_config|object (typed record)|private|Keys only in default literal: `max_concurrent,step_timeout_secs,execution_capacity,max_steps,retry_count` W:154|
|workflow_id|W:32,43,125,134,142|`String`/`Option<String>`|`id` in WorkflowSummary W:50|workflow_id|opaque id|public|Same concept, two names (see §9)|
|params|W:38|`Option<serde_json::Value>`|steps encoded as `agent.operation\|{JSON}` strings W:33-35|workflow_execution_params|object, private|private|Explicitly `x-opdbus-private=true` W:37; steps are string-encoded tool refs, not structs|
|execution_id|W:45,85,103,115,127,135,143|`Option<String>`/`String`|—|workflow_execution_id|opaque id|public|Session-owned execution lookup W:44|

### `WorkflowSummary` / `WorkflowExecutionSummary` / outputs

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|id|W:50|`String`|`workflow_id` elsewhere|workflow_id|opaque id|public|Rename for joinability|
|name|W:51|`String`|—|workflow_name|string|public||
|steps|W:52|`Vec<String>`|`WorkflowAgentStep[]` (cognitive)|workflow_step_ref_list|`Vec<workflow_step_ref>`|private|Bare strings; cf. structured step in cognitive §4|
|triggers|W:53|`Vec<String>`|cron schedule/trigger|workflow_trigger_list|`Vec<enum: push\|manual\|cron\|webhook>`|public|Free strings; defaults show `push,manual,cron,webhook` W:153|
|status|W:54|`String`|`WorkflowExecutionStatus` enum W:70|workflow_status|enum string|public|Summary uses string, execution uses enum — conflict|
|workflows / count|W:59-60|`Vec<WorkflowSummary>`/`usize`|—|workflow_list / workflow_count|array / u64|public||
|executions / executions_truncated|W:63,65|`Vec<WorkflowExecutionSummary>`/`bool`|—|workflow_execution_list / workflow_execution_list_truncated|array / bool|private|Caller-session history only W:61-62|
|status (execution)|W:87,105,117|`WorkflowExecutionStatus`|`queued,running,pause_requested,paused,cancel_requested,cancelled,succeeded,failed,outcome_unknown,interrupted` (snake_case W:69-81)|workflow_execution_status|enum|public|Only properly-enumerated status in this plugin|
|revision|W:88,106|`u64`|toolset `generation`, registry `revision`|workflow_execution_revision|u64|public|Optimistic-concurrency token; name differs per plugin (§9)|
|accepted_at|W:89,118|`String`|created_at, refreshed_at|workflow_execution_accepted_at|RFC3339 UTC timestamp|public|String, no format constraint declared|
|completed_steps|W:90|`usize`|`steps_completed` (cognitive W:567)|workflow_execution_completed_step_count|u64|public|Name/order variant|
|in_flight|W:92,109|`bool`|—|workflow_execution_in_flight|bool|public|"may already have caused effects" W:91|
|reason|W:93|`Option<String>`|`rule` (etched Verdict), `diagnostic` (cron)|workflow_execution_status_reason|string|private||
|episode_committed / episode_error|W:96,98|`bool`/`Option<String>`|—|workflow_execution_episode_committed / _error|bool / string|private|Private SHM episode publication W:94-98 — provenance-adjacent, no hash/id emitted|
|accepted|W:114|`bool`|`ok` (cognitive), `success`|workflow_execution_accepted|bool|public||
|force|W:146|`bool` (default false)|—|workflow_cancel_force|bool|public|CancelWorkflowInput only|

Method refs: sealed keys `start_workflow,query_workflows,pause_workflow,resume_workflow,cancel_workflow` with native names `StartWorkflow,query_workflows,PauseWorkflow,ResumeWorkflow,CancelWorkflow` and capabilities `workflows.write`/`workflows.read`, OSCAL subids `mut/obs.software.plugin.workflows.*@v1` (W:248-302). Etched taxonomy's `workflows/start_workflow` + `workflows.write` matches exactly (§8).

## 2. `cron` — `cron.rs` (`C=` `.../state_plugins/cron.rs`)

### `CronSchedule` / `CronJobRecord` / `CronStateV2`

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|kind=Cron { expr, tz }|C:62-67|`enum(tag="kind")`: `String, Option<String>`|`expression` C:113, `schedule:String` in create/update|schedule_cron_expression / schedule_timezone|cron expr string / IANA tz string|public|Three representations of one schedule (§9)|
|kind=At { at }|C:68-70|`String`|—|schedule_at|RFC3339 UTC timestamp|public|Not admitted by create endpoint C:215-216|
|kind=Every { every_ms }|C:71-73|`u64`|—|schedule_interval_ms|u64 (ms)|public|Only field with explicit unit suffix in slice|
|id|C:112|`String`|`job_id` C:164,187,202,259,309,333,345|job_id|opaque id|public|Record uses `id`, every API uses `job_id`|
|expression|C:113|`String`|`schedule.expr`|cron_expression|cron expr string|public|Duplicates `schedule`|
|schedule|C:114|`CronSchedule`|`schedule:String` C:224,245,267|schedule|tagged enum|public||
|command|C:115,248,273|`String`|ShellInput.command|job_command|string|private|Shell execution payload|
|prompt|C:117,227,275|`Option<String>`|—|job_prompt|string|private|Agent-job payload; capability doc warns prompts leak via cron.read C:508|
|name|C:119,223,244,265|`Option<String>`|—|job_name|string|public||
|job_type|C:120,218|`CronJobType{Shell,Agent}` (lowercase)|tagged by `job_type` in CreateJobInput C:218|job_type|enum `shell\|agent`|public||
|session_target|C:121,231|`CronSessionTarget{Isolated,Main}`|—|job_session_target|enum `isolated\|main`|public||
|model|C:123,233|`Option<String>`|`model_id` (registry), `ModelEntry.id`|model_id|opaque model id|public|Bare `model`, no provider qualifier — ambiguous (§9)|
|agent_alias|C:124|`String`|`agent` C:221,242,263; `agent_type` (cognitive)|agent_id|opaque id|public|Three names for agent ref|
|enabled|C:125,277|`bool`/`Option<bool>`|—|job_enabled|bool|public||
|delivery.mode/channel/to/thread_id/best_effort|C:98-106|`String/Option<String>/Option<String>/Option<String>/bool`|`CronEventDelivery` enum|delivery_mode / delivery_channel / delivery_recipient_id / delivery_thread_id / delivery_best_effort|enum / string / opaque id / opaque id / bool|private|`mode` unbounded string while sibling delivery concept is an enum C:37-39; `to` is ambiguous|
|delete_after_run|C:127,237|`bool`/`Option<bool>`|—|job_delete_after_run|bool|public||
|allowed_tools|C:129,235|`Option<Vec<String>>`|`tools` (MCP ToolsetConfig)|job_allowed_tool_name_list|`Vec<tool_name>`|public|Tool refs as bare names, unbounded count vs `MAX_CRON_TOOL_NAMES=256` C:18|
|uses_memory|C:130,239,279|`bool`|memory_* methods|job_uses_memory|bool|private||
|source|C:131|`String`|—|job_source|string (enum?)|public|Unbounded; provenance-relevant but unstructured|
|shell_output_format|C:132,252,281|`CronShellOutputFormat{Wrapped,Raw}`|—|job_shell_output_format|enum `wrapped\|raw`|public||
|created_at / next_run / last_run|C:133,134,136|`String`/`String`/`Option<String>`|accepted_at, first_seen|job_created_at / job_next_run_at / job_last_run_at|RFC3339 UTC|public|No format declared; `next_run` lacks `_at`|
|last_status / last_output|C:138,140|`Option<String>`|`status`, `output`|job_last_run_status / job_last_run_output|enum / string|private|Output is content-bearing|
|scope|C:152,180,192,210,287,308,344|`CronScope{GlobalDaemon}`|—|cron_scope|enum `global_daemon`|public|Single-variant enum|
|event_stream.sequencing / .delivery|C:47-48|`CronEventSequencing{Unsequenced}`/`CronEventDelivery{BestEffort}`|—|cron_event_sequencing / cron_event_delivery|enum|public|Explicitly no event ids/replay cursor C:41-43 — provenance gap vs hash stream|
|jobs|C:157|`Vec<CronJobRecord>`|—|job_list|array|private|OSCAL subids `sch/obs/mut.service.cron.*` C:148-157|

### `CronRunRecord` / inputs / mutation outputs

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|id|C:163|`i64`|`execution_id:String`, etched `seq:integer`|cron_run_id|i64 / opaque id|public|Numeric run id vs string ids everywhere else|
|job_id|C:164|`String`|`id` C:112|cron_job_id|opaque id|public||
|started_at / finished_at|C:165-166,337-338|`String`|—|run_started_at / run_finished_at|RFC3339 UTC|public||
|status|C:167,332|`String`|`MutationDisposition,RunDisposition` enums|run_status|enum string|public|Free string beside typed dispositions|
|output|C:169,335|`Option<String>`/`String`|last_output|run_output|string|private||
|duration_ms|C:171,336|`Option<i64>`/`i64`|MCP `duration_ms:u64`|run_duration_ms|u64 ms|public|Signedness conflict (§9)|
|limit|C:205|`u32` (default 20, range 1..=100)|registry `limit:Option<u32>`|history_limit|u32|public||
|agent (create/update)|C:221,242,263|`String`/`Option<String>`|`agent_alias` C:124|agent_id|opaque id|public||
|schedule / tz / clear_tz (update)|C:267-271|`Option<String>`|CronSchedule struct|schedule_cron_expression / schedule_timezone / schedule_timezone_clear|cron string / IANA tz / bool|public|Patch splits tz out of schedule|
|disposition|C:288,312,346|`MutationDisposition{Confirmed,ProviderCompletedUnverified,Indeterminate}` / `RunDisposition{Completed,Indeterminate}`|etched `Verdict`|mutation_disposition|enum|public|Provenance-grade outcome typing — good pattern|
|job (mutation readback)|C:292|`Option<CronJobRecord>`|—|job|record|private|Actual readback, not echo C:289-290|
|diagnostic|C:294,352|`Option<String>`|`reason`, `error`, `readback_error`|mutation_diagnostic|string|private|Three error-detail names in one file (+C:319)|
|provider_completed / absent_on_readback / readback_error|C:311,317,319|`bool`/`Option<bool>`/`Option<String>`|—|delete_provider_completed / delete_absent_on_readback / delete_readback_error|bool/bool/string|public|Receipt-like delete confirmation — provenance-relevant|
|success|C:334|`bool`|`accepted, ok`|run_succeeded|bool|public||

Method refs: `list_jobs,get_job,get_history,create_job,update_job,delete_job,enable_job,disable_job,run_job` (inferred enable/disable from subids C:479-496), caps `cron.read`/`cron.invoke` (C:425-510). Etched `cron/create_job`+`cron.invoke` matches.

## 3. `cognitive` + `cognitive_agents` (`G=` `.../cognitive.rs`, `A=` `.../cognitive_agents.rs`)

Focus: workflow/tool/model/OSCAL fields; memory structs included only where they carry ids/tags/hashes relevant to routing/provenance.

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|WorkflowAgentStep.agent_type / .operation / .arguments / .condition / .name|G:537-541; duplicated A:114-118|`String/String/serde_json::Value/Option<String>/Option<String>`|`CatalogAgentMethod.agent_type/operation` A:161-162; cron step strings|workflow_step_agent_type / _operation / _arguments / _condition / _name|string / string / object / string / string|private|Exact duplicate struct in two files (G:536 vs A:113) — synonym/dup (§9). `arguments` untyped|
|WorkflowAgentInput.operation (tag)|G:546-559|`enum(tag="operation"): RunWorkflow,ListSteps,Validate { steps:Option<Vec<WorkflowAgentStep>>, context:Option<String> }`|CatalogWorkflowInput.steps/context A:124-125|workflow_operation / workflow_step_list / workflow_context|enum / array / string|private|Method+input fused; no workflow id at all — steps are the identity|
|WorkflowAgentOutput.workflow_agent / steps_completed / results / valid / errors|G:565-580; A:131-151|`String/usize/Vec<Value>/bool/Vec<Value>`|`ok` A:131 vs `success` elsewhere|workflow_agent_name / workflow_steps_completed_count / workflow_step_result_list / workflow_valid / workflow_error_list|string/u64/array/bool/array|private|Untagged enum G:562; A-file splits into 3 named outputs — same fields, different shape|
|CatalogAgentMethod.agent_type / operation / category / declaration|A:161-164|`String/String/String/MethodDecl` (non-serializable, Debug+Clone only)|ToolDefinition (MCP)|agent_operation_category / agent_method_declaration|string/string/string/object|public|Catalog descriptor; `declaration` is the sealed method ref|
|CatalogPersonaInput.question / path / context|A:19-21|`String/Option<String>/Option<String>`|SpecialistInput G:603-609|persona_question / persona_path / persona_context|string/string/string|private||
|CatalogExecutionOutput.agent_type / operation / stdout / stderr / exit_code|A:27-31|`String/String/String/String/i32`|RustProOutput/AgentOperationOutput G:920-936 (`success,agent_name,data,metadata`)|execution_agent_type / _operation / _stdout / _stderr / _exit_code|string/string/string/string/i32|private|Two incompatible execution-output shapes in same slice|
|InvokeToolInput.tool_name / arguments|G:805,810|`String/serde_json::Value`|`name` (ToolDefinition), `toolset_id`, `tools[]`|tool_name / tool_arguments|tool name / object|private|Arguments explicitly unvalidated by bridge G:808|
|InvokeToolOutput.success / tool_name / result / error|G:818-823|`bool/String/Option<Value>/Option<String>`|ExecutionResult (MCP)|tool_invocation_succeeded / tool_name / tool_result / tool_error|bool/string/object/string|private||
|ToolsetsInput.operation / toolset_id|G:682,684|`ToolsetsOperation{List,Select}`/`Option<String>`|`id` (ToolsetConfig)|toolset_operation / toolset_id|enum/opaque id|public|Selection "never changes server-side grants" G:678-679 — routing-relevant|
|ToolsetsOutput.operation / toolset_generation / relist_required / result|G:877-880|`String/u64/bool/Value`|`generation` (MCP), `revision` (registry)|toolset_operation / toolset_generation / toolset_relist_required / toolset_result|string/u64/bool/object|public|`operation` downgraded enum→String on output|
|SchemaReadInput.plugin_id|G:693|`String`|`plugin` (elevation/etched)|plugin_id|opaque id|public||
|SchemaReadOutput.plugin_id / uri / schema_hash / oscal_subid_count / oscal_subids / schema|G:886-891|`String/String/String/usize/Vec<String>/Value`|etched control/subid strings|plugin_id / schema_uri / schema_hash / oscal_subid_count / oscal_subid_list / schema_document|string/string/lowercase hex sha256/u64/array/object|hash-only (`schema_hash`), rest public|Key provenance record: sealed schema digest + OSCAL tags per plugin|
|OscalSubidsInput.plugin_id / prefix|G:700,704|`Option<String>`|—|plugin_id / oscal_subid_prefix|string/string|public||
|OscalSubidEntry.subid / category / sources[{plugin_id,schema_hash}]|G:897-906|`String/String/Vec<OscalSubidSource>`|TAG-TAXONOMY tag/control|oscal_subid / oscal_category / oscal_subid_source_list|string/string/array|hash-only (nested schema_hash)|Runtime tag authority surface; `category` values not enumerated here|
|SpecialistInput.operation / question / path / domain_hints|G:603-614|`AdvisorOperation{Consult,Review,Explain,Recommend,Advise,Analyze,Help}`/String/Option<String>/Vec<String>|CatalogPersonaInput|specialist_operation / _question / _path / _domain_hint_list|enum/string/string/array|private||
|SpecialistOutput.answer / persona{name,domains,piece_ids,sources} / context_used / provider / model|G:620-636|`String/{String,Vec<String>,Vec<String>,Vec<String>}/usize/String/String`|`provider_id/model_id` (registry); `piece_ids` unique here|specialist_answer / persona_name / persona_domain_list / persona_piece_id_list / persona_source_list / specialist_context_line_count / provider_id / model_id|string/…/u64/opaque ids|private (answer/persona), public (provider/model)|`provider`+`model` bare strings, not the registry's `provider_id`/`model_id` pair; persona kept "for audit" G:617 — provenance record without hash/receipt|
|Memory*{namespace,namespace_kind,key,value,tags,key_pattern,limit,container_id}|G:386-482|`Option<String>/Option<NamespaceKind>/Option<String>/Option<Value>/Vec<String>/Option<String>/i64/Option<String>`|MemoryEntrySummary.namespace_id G:862|memory_namespace_id / memory_key / memory_value / memory_tag_list / memory_key_pattern / memory_limit / memory_container_id|string/string/JSON/array/string/i64/string|private|Scoped out of detailed inventory except: `namespace` (input) vs `namespace_id` (output G:862) and exact-tag filters G:420 are tag-routing-relevant|
|MemoryEntrySummary.id / access_count / updated_at|G:861-866|`String/i64/String`|CronRunRecord.id:i64|memory_entry_id / memory_entry_access_count / memory_entry_updated_at|opaque id/i64/RFC3339|private||
|SourceInfo.id / title / source_type / tags / created_at; Citation.text/source/page|G:351-360,341-345|`String/Option<String>/SourceType/Vec<String>/Option<String>`; `Option<String>×3`|—|source_id / source_title / source_type / source_tag_list / source_created_at|opaque id/string/enum/array/RFC3339|private|Citation `source` is a document id as bare string|
|GetHealthOutput.bridge_owned / execution_model / auth_status / queries_remaining / queries_limit|G:829-833|`bool/String/String/i64/i64`|`AuthStatus` enum G:139 (state side)|bridge_owned / execution_model / auth_status / queries_remaining_count / queries_limit_count|bool/string/enum/i64/i64|public|Output flattens enum→String vs CognitiveState G:1081|

Method refs (sample): `memory_store/memory_recall/…` caps `cognitive.memory.write`/`cognitive.read` (G:1167-1273), `specialist` (G:1330), `toolsets` (G:1360). No `operation_id`/`event_hash`/receipt field anywhere in this slice.

## 4. `mcp` — `mcp.rs` + `mcp_methods.rs` (`M=`, `MM=`)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|McpConfig.servers|M:22|`Option<HashMap<String,McpServerConfig>>`|key = server name|mcp_server_config_by_name|map\<server_name, record\>|private|Server identity lives only in map key — no `server_id` field|
|McpConfig.tool_groups / compact_mode|M:26,30|`Option<ToolGroupsConfig>`/`Option<CompactModeConfig>`|ConfigureToolGroupsInput.toolsets|mcp_tool_group_config / mcp_compact_mode_config|record|public||
|McpServerConfig.command / args / env / enabled / transport|M:36-52|`String/Option<Vec<String>>/Option<HashMap<String,String>>/bool/String(="stdio")`|ShellInput.command|mcp_server_command / _arg_list / _env_var_map / _enabled / _transport|string/array/map/bool/enum `stdio\|sse\|http`|private (`env` may hold secrets)|`transport` unbounded string, values only in doc comment M:50|
|ToolGroupsConfig.enabled / max_tools / access_zone / trusted_networks|M:58-70|`Vec<String>/usize/Option<String>/Option<Vec<String>>`|`enabled:Vec<String>` here = group ids; ToolsetConfig.id elsewhere|tool_group_enabled_id_list / tool_group_max_tool_count / tool_group_access_zone / tool_group_trusted_network_list|array/u64/enum/array|public|`access_zone` values (`localhost,trusted_mesh,private_network,public`) doc-only M:64; network prefixes = provenance/trust-relevant|
|ToolsetConfig.id / temperature / provider / requires_provider_health / tools|M:84-88|`String/ToolsetTemperature{Hot,Warm,Cold}/String/bool/Vec<String>`|`toolset_id` G:684; `provider_id` registry|toolset_id / toolset_temperature / toolset_provider_id / toolset_requires_provider_health / toolset_tool_name_list|opaque id/enum/opaque id/bool/array|public||
|ConfigureToolGroupsInput.expected_generation; Output.generation|M:94,101|`u64`|workflows `revision`, registry `revision`|toolset_expected_generation / toolset_generation|u64|public|CAS token; three names for version counters (§9)|
|CompactModeConfig.enabled / meta_tools|M:109,113|`bool/Vec<String>` (default list M:128-136)|—|compact_mode_enabled / compact_mode_meta_tool_name_list|bool/array|public|Meta-tools are method refs as strings|
|ToolDefinition.name / description / input_schema / schema_version / category / tags / namespace|M:141-151|`String/String/Value/String/String/Vec<String>/String`|InvokeToolInput.tool_name; OscalSubidEntry.category|tool_name / tool_description / tool_input_schema / tool_schema_version / tool_category / tool_tag_list / tool_namespace|string/string/JSON object/string/string/tag array/string|public|Canonical tool catalog record. `schema_version` default is a JSON-Schema draft URL M:154-156, not a version; `namespace` default `system.v1` M:158-160. `tags` here are free tool tags ≠ OSCAL control tags ≠ memory tags (§9)|
|ExecutionResult.success / output / error / duration_ms|M:165-169|`bool/Value/Option<String>/u64`|ManualRunResult|tool_execution_succeeded / _output / _error / _duration_ms|bool/JSON/string/u64 ms|private||
|McpState.config / toolsets|M:204,208|`serde_json::Value`(private field)/`Option<ConfigureToolGroupsOutput>`|—|mcp_config / mcp_toolset_projection|object/record|private|State is untyped JSON + typed projection|
|McpMethodPlan.target_method / required_capability / args|MM:12-14|`&'static str/String/Value`|`Operation.plugin/method/capability` (etched)|target_method_name / required_capability / method_args|string/capability string/object|private (args)|Facade routing record: `get_config→GetMcpConfig`, `configure_server→PatchConfig` MM:67-87 — PascalCase native method names vs snake_case facade (conflict, §9)|

Method refs: `get_config` cap `mcp.read`, `configure_server`/`configure_tool_groups` cap `mcp.write`, subids `obs/mut.software.plugin.mcp.*@v2` (M:234-255).

## 5. `model_registry` — `model_registry.rs` (`R=`)

### `ProviderConfig` / `ControlPlaneRouting` / `ObservedModel`

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|provider_id (field `id`, serde rename+alias `id`)|R:154-155|`String`|`id` (etched ModelEntry), `provider` (cron/Specialist)|provider_id|opaque id|public|Wire name already canonical; Rust field name is not. Compatibility alias is a synonym source|
|native_provider_id|R:159,708,791|`Option<String>`|`reference/family` (NativeProviderProfile); etched — none|native_provider_id|opaque id (`family.alias`)|public|Binding metadata, "not evidence live router has it" R:157|
|inference_url_prefix|R:164,711,794|`Option<String>`|models_url|provider_inference_url_prefix|HTTPS URL prefix string|public|Route-check namespace R:160-162|
|native_profile_overrides|R:168|`BTreeMap<String,String>`|recorded_profile, control_plane_profile|native_profile_override_by_model_id|map\<model_id, profile\>|public|Keys are model ids (unnamed)|
|name|R:169,714|`String`/`Option<String>`|—|provider_name|string|public||
|models_url|R:171,729|`String`/`Option<String>`|inference_url_prefix|provider_models_url|HTTPS URL|public|Key "only ever sent to models_url" R:175|
|format|R:172|`ModelListFormat{Openai,Anthropic}`|—|provider_model_list_format|enum `openai\|anthropic`|public||
|auth|R:173|`ProviderAuth{None,Bearer,XApiKey}`|credential|provider_auth_method|enum `none\|bearer\|x_api_key`|public||
|key_file / key_env|R:177,182|`Option<String>`|credential_slot, slot|provider_key_file_path / provider_key_env_var_name|path string / env-var name|private|Paths/names only, never values — correctly hash/name-only by design|
|credential / credential_slot|R:188,194,722,727|`Option<String>`|`kind`+`slot` in CredentialGateOp R:678-684|provider_credential_gate_kind / provider_credential_slot_id|enum `key\|oauth\|off` / opaque id|private|Values doc-only (R:183-194); `credential_slot` for oauth = `family:profile`|
|control_plane{profile, alternatives}|R:197,209-211|`Option<ControlPlaneRouting>`|`control_plane_profiles[]` R:745, `control_plane_profile` R:814, `profile` R:825|control_plane_profile / control_plane_alternative_profile_list|`family.alias` strings|public|Singular/plural/per-model variants of one concept|
|enabled|R:199|`bool`|—|provider_enabled|bool|public||

### `NativeProviderProfile` / `CredentialGateOp` / `ProviderStatus` / `ModelRecord` / outputs

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|reference / family / model / oauth / selectable|R:645-654|`String/String/String/bool/bool`|`native_provider_id`, `profile`|native_profile_ref / native_profile_family / native_profile_model_id / native_profile_oauth / native_profile_selectable|string/string/model id/bool/bool|public|`reference` is `family.alias` split redundantly into family+model R:646-649|
|CredentialGateOp.provider_id / kind / slot|R:678,680,684|`String/String/Option<String>`|`credential/credential_slot` R:188-194|credential_gate_provider_id / credential_gate_kind / credential_gate_slot_id|opaque id/enum/opaque id|private|`kind` unbounded string for a 4-value enum (`key,oauth,off,remove` R:679)|
|MaintainCredentialsInput.operations / Output.applied|R:692,698|`Vec<CredentialGateOp>` (≤32)|—|credential_gate_operation_list / _applied_list|array|private||
|ProviderStatus.provider_id / configured / enabled / status / error|R:704,716,717,731,733|`String/bool/bool/String/Option<String>`|ProviderRefresh.status R:867|provider_id / provider_configured / provider_enabled / provider_status / provider_error|id/bool/bool/enum `never\|ok\|error\|removed`/string|public|`status` string-enum, values doc-only R:730|
|refreshed_at / succeeded_at|R:736,739|`Option<String>` (RFC3339 per docs R:734-739)|RefreshOutput.refreshed_at R:880 (non-Option)|provider_refreshed_at / provider_last_succeeded_at|RFC3339 UTC|public|Only timestamps in slice with stated format — still String type|
|available_models / known_models|R:740-741,870|`u64`|added/dropped R:872-874|provider_available_model_count / provider_known_model_count|u64|public||
|control_plane_profiles|R:745|`Vec<String>`|control_plane.profile|provider_control_plane_profile_list|array|public||
|ListProvidersOutput.providers / revision|R:751,753|`Vec<ProviderStatus>/u64`|`generation` (MCP)|provider_list / provider_catalog_revision|array/u64|public||
|ModelRef{provider_id, model_id}; ListModelsInput.provider_id/include_unavailable/query/after/limit|R:759-760,768-780|`String/String; Option<…>/Option<bool>/Option<String>/Option<ModelRef>/Option<u32>`|ObservedModel.model_id|model_provider_id / model_id / model_include_unavailable / model_query / model_after_cursor / model_page_limit|ids/bool/string/cursor/u32 (1..=500)|public|Composite-key cursor — good pagination pattern|
|ModelRecord.provider_id / model_id / name / context_length / prompt_price / completion_price / available / first_seen / last_seen / recorded_profile / control_plane_profile|R:786-814|`String/String/Option<String>/Option<u64>/Option<String>/Option<String>/bool/String/String/Option<String>/Option<String>`|ModelEntry{id,provider,capabilities} (etched)|model_provider_id / model_id / model_name / model_context_length_token_count / model_prompt_price_usd_per_token / model_completion_price_usd_per_token / model_available / model_first_seen_at / model_last_seen_at / model_recorded_profile / model_control_plane_profile|ids/string/u64 tokens/decimal-string USD/bool/RFC3339/profile|public|Prices are provider-stated strings R:540-545 (unit = USD/token, unstated in name); `context_length` unit (tokens) unstated; timestamps lack `_at` on seen fields vs `first_seen/last_seen`|
|SetModelProfileInput.provider_id / model_id / profile; Output.model / revision|R:820-825,831-832|`String/String/Option<String>`; ModelRecord/u64|native_profile_overrides|model_profile_provider_id / _model_id / _profile|ids/`family.alias`|public|`profile` validated as `family.alias` R:839|
|ProviderRefresh.provider_id / status / error / available_models / added / dropped; RefreshOutput.refreshed_at / revision / providers|R:865-882|`String/String/Option<String>/u64/u64/u64; String/u64/Vec<…>`|—|refresh_provider_id / refresh_status / refresh_error / refresh_available_model_count / refresh_added_model_count / refresh_dropped_model_count / refreshed_at / catalog_revision|…/enum `ok\|error\|removed`|public||
|ModelRegistryState.providers_path / secrets_dir / max_providers / max_models_per_provider|R:921,924,926,928|`String/String/u64/u64`|—|provider_config_path / provider_secrets_dir / max_provider_count / max_model_per_provider_count|path/path/u64/u64|private (paths)|OSCAL subids `obs.service.model-registry.*@v1` R:920-928|

Etched-side counterpart `ModelEntry{id,provider,capabilities}` (§8) has **no** context/prices/availability/profile fields — catalog-routing selection cannot express registry constraints.

## 6. `elevation` — `elevation.rs` (`E=`)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|ProtectedAction.action_id|E:49|`String` (polkit-style, E:37-41 constants)|—|protected_action_id|opaque id (`org.opdbus.v1.*`)|public||
|ProtectedAction.plugin / methods / description|E:50,51,52|`String/Vec<String>/String`|`Operation.plugin/method` (etched, singular method)|protected_action_plugin_id / protected_action_method_name_list / protected_action_description|string/array/string|public|Plural `methods[]` vs etched singular `method` per Operation|
|AuthorizeTaskInput.plugin / method / args / password|E:164,165,166,169|`String/String/JsonValue/String` (≤1024 bytes E:35,193)|McpMethodPlan|authorize_plugin_id / authorize_method_name / authorize_method_args / authorize_root_password|string/string/object/string|private (args+password; password `x-secret` E:168)|Ticket binds exact plugin+method+canonical args E:7-10 — core provenance/elevation record. No ticket id returned|
|AuthorizeTaskOutput.action_id / expires_at|E:175,177|`String/String` (RFC3339, E:176)|ticket_ttl_seconds|authorize_action_id / authorize_ticket_expires_at|id/RFC3339 UTC|public|Expiry only; issuance time and ticket id absent — receipt must come from bridge audit, not this output|
|ListProtectedActionsOutput.actions / ticket_ttl_seconds; ElevationState.protected_actions / ticket_ttl_seconds|E:187,188,212,214|`Vec<ProtectedAction>/i64` (const 60, E:34)|`duration_ms`, `every_ms`|protected_action_list / elevation_ticket_ttl_second_count|array/i64 s|public|Signed i64 vs u64 durations elsewhere|

Method refs: `authorize_task` cap `elevation.authorize`, `list_protected_actions` cap `elevation.read` (E:31-32,241-259). Protected targets include `chat_router{config_set,config_patch,PatchConfig,config_init,config_migrate}` and `json_render{promote_candidate,retire_catalog_entry}` (E:62-90) — note mixed snake/Pascal method names in one list.

## 7. etched-in-schema — `catalog-routing/src/model.rs`, `decide.rs`, `catalogs/compliance.rs`, `catalogs/models.rs` (`T=`)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|Request.actor|T:model.rs:28|`String`|`principal` (specgen GrantRow), `member_key` (org_bridge)|request_actor_id|opaque id|private|Identity ref, no session/genesis linkage (cf. index-entry identity §8)|
|Request.surface|T:model.rs:29|`Surface{Web,TeamChat,Teams,Email}` (no serde rename — PascalCase wire)|qualifier `surface:web` (TAG-TAXONOMY:19-20)|request_surface|enum `web\|team_chat\|teams\|email`|public|Enum variant casing ≠ tag spelling — mis-normalized (§10)|
|Request.action|T:model.rs:30|`ActionKind{Read,Write,Publish}`|qualifiers `action:read/write/publish`|request_action|enum `read\|write\|publish`|public|Same casing issue; duplicated as string tags in `tags[]` — dual representation|
|Request.tags|T:model.rs:31|`Vec<String>`|OSCAL tag, memory tags, tool tags|request_tag_list|array\<control-or-qualifier tag\>|private|≥1 control tag required (model.rs:24-25); qualifiers `action:*,approval:root,surface:*,data:*,model:*` never match controls (compliance.rs:5-7)|
|Request.payload_ref|T:model.rs:32|`String`|artifact.ref (index)|request_payload_ref|opaque ref string|private|Unbounded; no hash — payload itself never carried, provenance by ref only|
|Profile.id / controls|T:model.rs:39,40|`String/Vec<String>` (e.g. `etched-pt-1`)|OSCAL control ids|profile_id / profile_control_id_list|opaque id/array|public|Compiled profile; grants arrive separately (model.rs:35-36)|
|Grants.capabilities|T:model.rs:45|`Vec<String>` (exact match, model.rs:50-52)|sealed caps `cap.governance.*@v1`, `cron.invoke`, `workflows.write`, `elevation.authorize`|grant_capability_list|array\<capability string\>|private|Two capability namespaces coexist: `cap.*@v1` (governance) vs bare `plugin.verb` (cron/workflows/elevation/mcp) — conflict (§9)|
|Operation.plugin / method / capability|T:model.rs:62,63,64|`String/String/Option<String>`|ProtectedAction.plugin/methods; McpMethodPlan|operation_plugin_id / operation_method_name / operation_required_capability|string/string/Option\<capability\>|public|`capability=None` = unsealed → refuse `method-not-sealed:<plugin>.<method>` (decide.rs:36-46)|
|Verdict::Allow / Refuse{missing_capability,rule} / NeedsApproval{have,need,rule}|T:model.rs:71,73-74,77-79|`enum`|cron dispositions|decision_verdict / decision_missing_capability / decision_rule / decision_approval_have_count / decision_approval_need_count|enum/…/u32|public|Refusals name capability+rule (model.rs:67-68); root ⇒ have:0,need:2,rule:`two-person-root` (decide.rs:25-34). **No `decision_ref`, `operation_id`, or `event_hash` on Decision** (model.rs:85-88) — the spec's `Outcome` receipt fields (spec §Basis) do not exist in this type; receipt linkage is downstream only|
|Decision.operations / verdict|T:model.rs:86,87|`Vec<Operation>/Verdict`|—|decision_operation_list / decision_verdict|array/enum|public||
|ControlMatch.control / operations|T:compliance.rs:82,83|`String/Vec<Operation>`|Profile.controls|matched_control_id / matched_operation_list|string/array|public|Taxonomy rows: `pii→etched-pt-1, access-control→etched-ac-1, audit→etched-au-1, retention→etched-si-1, integrity→etched-in-1` (compliance.rs:18-77), mirroring TAG-TAXONOMY.md:11-15|
|ModelEntry.id / provider / capabilities|T:models.rs:11,12,13|`String/String/Vec<String>`|`model_id/provider_id` (registry); cron `model`|model_id / provider_id / model_capability_list|opaque ids/array|public|Selection: first entry covering all `model:*` tags (models.rs:19-29). Field-name conflict with registry (§9)|

Other types-only files in scope: `GrantRow{principal,capability,effect,decision}` (specgen.rs:13-16), `ProvenanceRow{capability,role,position,event}` (specgen.rs:145-148 — capability←role←position←chart-event provenance chain, all `&'static str`), `FieldRule{path,class:FieldClass{Identifier,Content,Pii}}` + `SchemaView{schema,purposes,fields,pii_gate}` (projection.rs:49-62) — the slice's only field-level sensitivity classification, keyed by JSON pointer.

## 8. etched-in-schema — `oscal/TAG-TAXONOMY.md` + `schema/index-entry.schema.json` (`X=`)

Taxonomy (TAG-TAXONOMY.md:9-20): `tag` (string, e.g. `pii,audit,access-control,retention,integrity`), `control` (`etched-*`), sealed operations as `plugin/method` strings, evidence column naming receipt types. Qualifiers `data:private|public, action:read|write|publish, approval:root, surface:team-chat|web` — note `surface:team-chat` (kebab) vs `Surface::TeamChat` (§7).

### `EtchedIndexEntry` (X:index-entry.schema.json)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|format|X:10|`const "etched.index-entry/1"`|—|index_entry_format|const string|public||
|seq|X:11|`integer ≥0`|CronRunRecord.id:i64|index_entry_seq|u64|public|Append-only position per identity|
|prev_hash|X:12|`string, ^[0-9a-f]{64}$`|schema_hash (unpatterned)|index_entry_prev_hash|lowercase hex sha256|hash-only|64 zeros for seq 0. Best-normalized hash in slice|
|kind|X:13|`enum[contract,token,attestation,ipfs_object,domain,offer,transaction,other]`|—|index_entry_kind|enum|public||
|artifact.chain_id / ref / description|X:19,20,21|`integer/string/string ≤500`|payload_ref|artifact_chain_id / artifact_ref / artifact_description|EIP-155 u64 / string (tx hash\|address\|CID\|URL) / string|public|`ref` is four types in one string — mis-normalized (§10)|
|identity.session_id / genesis / origin_session_id|X:29,30,31|`uuid / ^[0-9a-f]{64}$ / uuid`|actor, principal|identity_session_id / identity_genesis_hash / identity_origin_session_id|uuid / lowercase hex sha256 / uuid|hash-only (genesis), private (session linkage)|Only identity-binding record in slice|
|receipt.operation_id / event_hash / plugin / method / required_capability|X:39,40,41,42,43|`uuid / string / string / string / string`|Operation.plugin/method/capability; spec Outcome|receipt_operation_id / receipt_event_hash / receipt_plugin_id / receipt_method_name / receipt_required_capability|uuid / lowercase hex sha256 / string / string / capability string|hash-only (event_hash), public rest|**`event_hash` has no hex pattern** (X:40) unlike prev_hash/genesis — normalization gap. This receipt is the provenance anchor: index entry → OP-DBUS operation|
|resource.gas_used / cost_usd_micros|X:50,51|`integer ≥0`|prompt_price strings (registry)|resource_gas_used / resource_cost_usd_micros|u64 / u64 µUSD|public|Explicit unit suffix — model for price fields|
|created_at|X:54|`string, format:date-time`|all backend `*_at:String`|index_entry_created_at|RFC3339 UTC|public|Only schema-enforced timestamp in slice|

## 9. (a) Duplicate / synonym fields across plugins (cross-repo)

|Concept|Variants (source)|
|---|---|
|Workflow/job id|`WorkflowSummary.id` (W:50) vs `workflow_id` (W:32); `CronJobRecord.id` (C:112) vs `job_id` (C:164+); `ModelEntry.id` (T:models:11) vs `model_id` (R:535,760,787); `ProviderConfig.id`→wire `provider_id` (R:155)|
|Model ref|`model` (C:123, G:636), `provider`+`model` (G:635-636), `ModelEntry{id,provider}` (T:models:11-12), `ModelRecord{provider_id,model_id}` (R:786-787), `NativeProviderProfile{reference,family,model}` (R:645-649), `native_provider_id`/`profile`/`recorded_profile`/`control_plane_profile(s)` (R:159,209,745,810,814)|
|Agent ref|`agent_alias` (C:124), `agent` (C:221), `agent_type` (G:537, A:114,161), `workflow_agent` (G:566)|
|Tool ref|`tool_name` (G:805), `name` (M:141), `tools[]` (M:88,129-var), `allowed_tools` (C:129), `meta_tools` (M:113), `toolset_id`/`id` (G:684, M:84)|
|Method ref|`method` singular (E:165, T:model:63, X:42), `methods[]` (E:51), `target_method` (MM:12), native `StartWorkflow/PatchConfig/GetMcpConfig` vs facade `start_workflow/configure_server/get_config` (W:248-250, MM:67-87, M:235-245)|
|Version/CAS counter|`revision` (W:88, R:753), `generation`/`expected_generation` (M:94-101), `toolset_generation` (G:878)|
|Schedule|`CronSchedule{expr,tz\|at\|every_ms}` (C:62-73) vs `expression`+`schedule` both on CronJobRecord (C:113-114) vs `schedule:String`+`tz`+`clear_tz` (C:224-271)|
|Status|`status:String` (W:17,54; C:138,167,332,731,867) vs typed enums `WorkflowExecutionStatus` (W:70), `Mutation/RunDisposition` (C:299,324)|
|Success flag|`accepted` (W:114), `success` (C:334, G:818, M:165), `ok` (G:845, A:131)|
|Error detail|`reason` (W:93), `diagnostic` (C:294), `error` (C:733, M:168), `readback_error` (C:319), `episode_error` (W:98), `rule` (T:model:74)|
|Tags|`tags` = OSCAL control tags (T:model:31), memory entry tags (G:403), tool tags (M:149), source tags (G:358) — four unrelated semantics, one name|
|Timestamps|`accepted_at,created_at,next_run,last_run,started_at,finished_at,first_seen,last_seen,refreshed_at,succeeded_at,updated_at,expires_at` — all `String`, only E:177/R:734/X:54 document RFC3339|
|Capability strings|`cap.governance.schema-tag-registry.read@v1` / `cap.identity.sled.self.read@v1` (compliance.rs:25,47) vs `cron.invoke`, `workflows.write`, `elevation.authorize`, `mcp.read`, `cognitive.read` — two namespaces for grants; index `required_capability` (X:43) accepts either, unnormalized|
|Workflow step|`WorkflowSummary.steps:Vec<String>` (W:52) vs `WorkflowAgentStep{agent_type,operation,arguments,condition,name}` duplicated verbatim in G:536-541 and A:113-118; cron embeds steps as `params.steps` strings (W:33-35)|
|Execution output|`CatalogExecutionOutput{stdout,stderr,exit_code}` (A:27-31) vs `RustPro/AgentOperationOutput{success,agent_name,data,metadata}` (G:920-936) vs `ExecutionResult{success,output,error,duration_ms}` (M:165-169)|

## 10. (b) Mis-normalized fields

- **Unbounded strings where enum/id/hash belongs:** `WorkflowsState.status`, `WorkflowSummary.status` (W:17,54) vs sibling enum; `CronDelivery.mode` (C:98), `CronJobRecord.source/last_status` (C:131,138), `McpServerConfig.transport` (M:52), `ToolGroupsConfig.access_zone` (M:66), `ProviderStatus.status` (R:731), `CredentialGateOp.kind` (R:680), `ToolsetsOutput.operation` (G:877, enum on input) — all doc-comment enums.
- **Hash fields without format:** `schema_hash` (G:888,898) and index `receipt.event_hash` (X:40) are plain `String` vs `prev_hash`/`genesis` patterned `^[0-9a-f]{64}$` (X:12,30). Canonical: all `*_hash` = lowercase hex sha256.
- **Mixed-case conventions:** etched `Surface`/`ActionKind` serialize PascalCase (model.rs:8-22, no `rename_all`) while their own qualifier tags are `surface:web`/`team-chat` (TAG-TAXONOMY:19); backend method lists mix `config_patch`/`PatchConfig` (E:65); cron enums use `lowercase`, workflow/cognitive `snake_case`, MCP state `@v2` subids vs `@v1` elsewhere.
- **Ambiguous names:** `to` (C:102), `at` (C:69), `id` (generic, everywhere), `name` (M:141 vs C:119 vs R:169), `profile` (R:209,825 = `family.alias`; T:model:38 = routing profile; unrelated), `category` (M:147, G:905, A:163 — three taxonomies), `format` (R:172 vs X:10), `source` (C:131 vs G:343), `context` (G:549, A:21 — grounding text, session, and workflow state).
- **Unit-less numerics:** `context_length` (tokens unstated, R:539), `prompt_price/completion_price` (USD/token as decimal strings, R:542-545) — contrast `cost_usd_micros` (X:51), `every_ms`/`duration_ms` (C:72,171; signedness differs C:171 `i64` vs M:169 `u64`), `ticket_ttl_seconds:i64` (E:188).
- **Polymorphic strings:** `artifact.ref` = tx hash | address | CID | URL (X:20); workflow step string `agent.operation|{JSON}` (W:34); `params`/`arguments`/`args`/`value`/`schema` as untyped `Value` (W:38, G:810, E:166, G:891) — the last is elevation-bound, so the ticket's canonical-args binding (E:7-10) is only as strong as bridge canonicalization, not this schema.
- **Singular/plural & optionality drift:** `methods[]` vs `method` (E:51 vs T:model:63); `refreshed_at: Option<String>` (R:736) vs `String` (R:880); `count` fields typed `usize` throughout vs registry `u64` counts.

## 11. (c) Tagged-routing / provenance relevance

- **Routing spine exists and matches across repos:** etched taxonomy (compliance.rs:18-77) names exactly the sealed backend methods/caps found here — `cron/create_job`+`cron.invoke` (C:453-459), `workflows/start_workflow`+`workflows.write` (W:248-254), `elevation/authorize_task`+`elevation.authorize` (E:241-246). `web3/anchor` remains `capability: None` (compliance.rs:74) = unsealed, fail-closed (decide.rs:36-46).
- **OSCAL subids are the backend tag layer:** every state struct carries `x-oscal-subid`/`x-oscal-category` (W:11-25, C:148-157, M:196-203, R:916-928, E:46,208-214), and cognitive exposes them as a queryable index with per-schema `schema_hash` provenance (G:885-915) — this is the bridge between OSCAL catalog tags and sealed schemas. Note taxonomy *control* tags (`etched-*`) and OSCAL *subids* (`mut.service.cron.job.create@v1`) are two distinct tag systems joined only via method/capability.
- **Receipt chain is typed only at the etched edge:** index `receipt{operation_id:uuid, event_hash, plugin, method, required_capability}` (X:37-44) + hash-linked `seq/prev_hash` (X:11-12) is the sole record that can join a public artifact to an OP-DBUS operation. Backend outputs in this slice emit **no** `operation_id`/`event_hash` (workflows return `execution_id`+`revision`, cron returns dispositions, elevation returns `action_id`+`expires_at`) — the join must be made by bridge audit machinery outside these contracts, and etched `Decision` itself carries no receipt ref (model.rs:85-88).
- **Elevation is the approval provenance record:** `AuthorizeTaskInput{plugin,method,args,password}` binds a one-time ticket to canonical args (E:161-170); `ProtectedAction` (E:47-53) is the polkit-mappable policy. But the output lacks ticket id/issued-at, so approval provenance depends on the bridge's ticket store, not this contract.
- **Sensitivity markers already present but sparse:** `x-opdbus-private` on workflow `params` (W:37), `x-secret` on password (E:168), key-material-by-path-only in registry (R:174-182), projection `FieldClass{Pii,Content,Identifier}` + `pii_gate` (projection.rs:41-62). Everything else (cron prompts/commands/delivery targets, MCP `env`, memory values, Specialist answer/persona) is private-by-content with no field-level marker — canonical schema should default these to `private`, with `schema_hash`/`prev_hash`/`genesis`/`event_hash` as the only `hash-only` fields and ids/statuses/counts as `public`.
- **Provenance gaps to close in canonical schema:** cron event stream explicitly has no event ids/sequencing (C:41-43); workflow episode publication reports only `episode_committed:bool` with no hash (W:96); `CronJobRecord.source` (C:131) and Specialist persona (G:619-624) are unstructured provenance; etched `ModelEntry` cannot carry registry provenance (first/last seen, recorded profile) needed to audit model selection.