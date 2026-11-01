# Slice 2 — Schema tags and routing decisions

Source: FldTagRouting field inventory (read-only, cited). Integrated verbatim; canonical proposals are proposals, not renames applied to code.

Sealed/live verification is pending (door unreachable).

Sources: `S1=crates/op-plugins/src/state_plugins/schema_tag_registry_methods.rs`, `S0=crates/op-plugins/src/state_plugins/schema_tag_registry.rs`, `D1=crates/op-plugins/src/state_plugins/routing_decision_methods.rs`, `D0=crates/op-plugins/src/state_plugins/routing_decision.rs`, `RD=crates/op-grpc-bridge/src/routing_dispatch.rs`, `RS=crates/op-grpc-bridge/src/routing_store.rs`. Sensitivity defaults `private` because `RegistryState`/`DecisionState` fields are `x-opdbus-private` (`S1:488-495`, `D1:234-237`) and registry test asserts `public_state.paths.is_empty()` (`S0:228-231`); hash values themselves are `hash-only` when disclosed.

## `schema_tag_registry` (`S0:114-183`: methods `list_tags,get_tag,register_tag_semantics,get/set_protected_method_marker,list/get/author/update/revoke_policy`; caps `S0:17-22`)

### `OpenFlowBindingRef` / `OpenFlowBindingRequest`

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|binding_id|`S1:27,62`|String|`binding_id`|binding_id|opaque id|private|validated ≤192B,no controls (`S1:38,68`)|
|revision|`S1:28`|u64|`revision,rule_revision,registry_revision,policy_revision`|binding_revision|u64 (>0)|private|per-record revision, not registry revision|
|plugin_id|`S1:29,77,86,110ff`|String|`plugin_id`|plugin_id|opaque id|private|in `Ref` must equal `openflow` (`S1:40-43`)|
|method_key|`S1:30`|String|`method_key,transport_name`|method_key|opaque id|private|enum-checked only here: `add_flow\|modify_flow\|delete_flow` (`S1:44-50`)|
|schema_hash|`S1:31`|String|`schema_hash,catalog_hash,source_sha256`|schema_hash|lowercase hex sha256|hash-only|**not** hash-format-validated; only ≤192B (`S1:51`)|
|network_tag_id|`S1:33,63`|u32|`tag_id,network_tag_id`|network_tag_id|u32 (>0)|private|must equal owning `TagDescriptor.tag_id` (`S1:165-168`)|

### `SchemaTagAssociation{,Request}`, `TagDescriptor`, `ResolvedTagRef`

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|plugin_id / method_key|`S1:77-78,86-87`|String|—|plugin_id / method_key|opaque id|private|`Request` omits hash; resolver supplies it (`S1:57-58` analogous)|
|schema_hash|`S1:79`|String|`schema_hash`|schema_hash|lowercase hex sha256|hash-only|only in resolved `Association`, not `Request`|
|tag_id|`S1:80,110,122,571,610`|u32|`tag,tag_id,network_tag_id`|tag_id|u32 (>0)|private|numeric tag identity everywhere except `ClassificationProposal.tag`|
|name|`S1:111,124`|String|`name,tag`|tag_name|bounded string|private|registry name; gate compares proposal label to this (`D1:577-587`)|
|description|`S1:112`|String|—|tag_description|bounded string ≤512B|private||
|default_disposition|`S1:113,613`|Option\<TagDisposition\>|`disposition,default_disposition`|default_tag_disposition|enum `local_only\|requires_residual_validation` (`S1:388-391`)|private||
|revision|`S1:114,123`|u64|`revision`|tag_revision|u64 (>0)|private||
|associations|`S1:115,614`|Vec\<SchemaTagAssociation{Request}\>|—|tag_association[]|array|private|≤256, deduped on (plugin,method,hash) (`S1:143-162`)|
|openflow_binding|`S1:116,616`|Option\<OpenFlowBinding{Ref,Request}\>|—|openflow_binding|object/null|private||

### `ProtectedMethodMarker{,Request}`, `ProtectedMethodMarkerKeyInput`, `SetProtectedMethodMarkerInput`, marker outputs

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|plugin_id/method_key/schema_hash|`S1:177-179,187-189,203-205`|String|—|same|id/id/lowercase-hex-sha256|private/hash-only(marker hash)|key = exact triple; lookup in `RD:528-531`|
|requires_policy|`S1:180,190`|bool|—|requires_policy|bool|private|drives `applicable_policy_missing` if no applicable rule (`D1:670-680`)|
|revision|`S1:181`|u64|—|marker_revision|u64 (>0)|private||
|marker|`S1:196,211,217`|ProtectedMethodMarkerRequest / Option\<Marker\> / Marker|—|protected_method_marker|object|private|`SetInput.marker`+`expected_revision:u64` (`S1:195-197`)|
|expected_revision|`S1:197,615,669,678,687`|u64|`expected_revision`|expected_revision|u64|private|optimistic concurrency on all mutations|

### `CanonicalMappingRef` / `CanonicalMappingRequest` (provenance)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|mapping_id|`S1:225,241`|String|—|canonical_mapping_id|opaque id|private||
|subid|`S1:226,242`|String|`subid`|operation_subid|opaque id|private|join key to `ResolvedOperationRef.subid` (`D1:636,657`)|
|generation|`S1:227`|String|—|mapping_generation|opaque id (currently String)|private|compared for equality across policies (`D1:659-667`); not numeric|
|source|`S1:228`|String|—|mapping_source|bounded string / enum|private|free string|
|source_ref|`S1:229`|String|—|mapping_source_ref|bounded string / URI|private|free string|
|source_sha256|`S1:230`|String|`source_sha256,schema_hash`|source_sha256|lowercase hex sha256 (64)|hash-only|only hash with format check: 64 hex, case not enforced (`S1:274-278`)|
|direction|`S1:231,243`|String|—|mapping_direction|enum|private|type is String but validated to `maps_to\|covered_by\|merged_into\|renumbered_to\|split_into\|reverse:*` (`S1:250-262`)|
|resolved|`S1:232`|bool|—|mapping_resolved|bool|private|unresolved ⇒ `Hold/canonical_mapping_unresolved` (`D1:644-649`); absent from `Request` by design (`S1:235-237`)|

### `PolicyRule` (tagged enum `kind`, `S1:368-384`), `TagDisposition` (`S1:388-391`)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|kind=DenyMethod{plugin_id,method_key,reason}|`S1:370-374`|String,String,String|`reason,revocation_reason`|rule.plugin_id/method_key/reason|id/id/bounded text|private|explicit deny wins first (`D1:596-599`)|
|kind=TagConstraint{tag_id,disposition}|`S1:375-378`|u32,TagDisposition|`disposition,default_disposition`|rule.tag_id, rule.tag_disposition|u32/enum|private|`LocalOnly` denies egress; `RequiresResidualValidation` denies egress without residual validation (`D1:688-705`)|
|kind=EgressRule{destination_class}|`S1:380`|String|`destination_class`|destination_class|enum (currently free String ≤192B)|private|matched exactly vs `GateInput.destination_class` (`D1:712`)|
|kind=EgressRule{allowed_provider_routes}|`S1:381`|Vec\<String\>|`provider_route,allowed_provider_routes`|allowed_provider_route[]|bounded string[] (1–64)|private|singular in `GateInput.provider_route` (`D1:195`)|
|kind=EgressRule{required_policy_revision}|`S1:382`|u64|`rule_revision,policy_revision`|required_policy_revision|u64 (>0)|private|must equal snapshot `rule_revision` (`D1:717`)|

### `PolicyRecord`, `PolicySnapshot`, `RegistryState`

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|owner_id|`S1:435,478`|String|`owner_id,policy_owner_id,principal_id`|policy_owner_id|opaque id|private|snapshot/record owner must match gate `policy_owner_id` (`D1:491-496,535-540`)|
|policy_id|`S1:436,622,668,677,686`|String|—|policy_id|opaque id|private|deduped in snapshot (`D1:530-547`)|
|revision|`S1:437`|u64|`revision,rule_revision,policy_revision`|policy_revision|u64 (>0)|private||
|active|`S1:438`|bool|—|policy_active|bool|private|invariant: `active == revocation_reason.is_none()` (`S1:453-456`)|
|revocation_reason|`S1:439`|Option\<String\>|`reason`|revocation_reason|bounded text/null|private|set by `RevokePolicyInput.reason:String` (`S1:688`)|
|rule|`S1:440,670,679`|PolicyRule|—|policy_rule|tagged enum|private||
|canonical_mappings|`S1:441`|Vec\<CanonicalMappingRef\>|`canonical_mappings`|canonical_mapping[]|array ≤64, single generation|private|requests in inputs are `Vec<CanonicalMappingRequest>` (`S1:671,680`)|
|rule_revision|`S1:479,695`|u64|`rule_revision,registry_revision,policy_revision,revision`|policy_rule_revision|u64|private|in `PolicySnapshot` and `PolicyMutationOutput`; = `RegistryState.revision` at snapshot (`RD:383`)|
|marker|`S1:480`|Option\<ProtectedMethodMarker\>|—|protected_method_marker|object/null|private||
|records|`S1:481`|Vec\<PolicyRecord\>|`records,decisions`|policy_record[]|array|private|snapshot ≤256 at gate (`D1:524`)|
|revision / tags / policies / protected_methods|`S1:489-495`|u64/Vec\<TagDescriptor\>/Vec\<PolicyRecord\>/Vec\<Marker\>|—|registry_revision/tag[]/policy[]/protected_method[]|u64/arrays ≤10,000|private|state contract|

Method I/O wrappers: `ListTagsInput{limit:u16(1-100),cursor:Option<String>}` (`S1:562-566`), `ListTagsOutput{tags,next_cursor}` (`S1:583-586`), `TagIdInput{tag_id}` (`S1:570-572`), `GetTagOutput{tag:Option}` (`S1:603-605`), `RegisterTagInput{tag_id,name,description,default_disposition,associations,expected_revision,openflow_binding}` (`S1:609-617`), `RegisterTagOutput{tag,registry_revision:u64}` (`S1:700-703`), `ListPoliciesInput/Output` mirrors tags (`S1:633-644`), `PolicyIdInput{policy_id}` (`S1:621-623`), `GetPolicyOutput{policy:Option}` (`S1:661-663`), `Author/UpdatePolicyInput{policy_id,expected_revision,rule,canonical_mappings:Vec<Request>}` (`S1:667-681`), `RevokePolicyInput{policy_id,expected_revision,reason}` (`S1:685-689`), `PolicyMutationOutput{policy,rule_revision}` (`S1:693-696`).

## `routing_decision` (`D0:99-119`: `list_decisions,get_decision,record_shadow_decision` only; no dispatch/approve method)

### `CandidateOperation`, `ClassificationProposal`, `ClassificationOrigin`

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|plugin_id/method_key/schema_hash|`D1:17-19`|String|—|same|id/id/lowercase-hex-sha256|private/hash-only|must match resolved op exactly (`D0:137-148`)|
|tag|`D1:33`|Option\<String\>|`tag,name,tag_id`|classification_tag_label|bounded string|private|**mis-normalized**: free label, not `tag_id`; resolved by name equality (`D1:581`)|
|asserted_origin|`D1:35`|ClassificationOrigin enum|`asserted_origin`|`classification_asserted_origin`|enum `declaration\|user_selection\|keyword_match\|model_proposal` (`D1:43-48`)|private|explicitly caller-asserted, not verified (`D1:34`)|
|taxonomy_revision|`D1:36`|Option\<u64\>|`taxonomy_revision,revision`|taxonomy_revision|u64|private|must pin current revision else `taxonomy_revision_stale` (`D1:469-474`)|
|classifier_version|`D1:37`|Option\<String\>|—|classifier_version|bounded string|private|free string, no format|
|uncertainty|`D1:38`|Option\<f64\>|—|classifier_uncertainty|f64 ∈[0,1]|private|properly bounded (`D1:58-63`)|

### `VerifiedCaller`, `ResolvedOperationRef`

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|principal_id|`D1:76`|String|`principal_id,actor`|principal_id|opaque id|private|bridge-derived, never in proposal DTO (`D1:327-328`)|
|session_id|`D1:77`|String|`session_id,session`|session_id|opaque id|private||
|session_genesis (bridge only)|`RS:34`|String|`genesis,session_genesis`|session_genesis|opaque id/hash|private|absent from `VerifiedCaller`; carried in `RoutingOwner`/`CallContext.genesis` (`RD:28`)|
|owner_id|`D1:78`|String|`owner_id,policy_owner_id`|owner_id|opaque id|private|owner-scope key (`D1:261-271`)|
|container|`D1:79`|Option\<String\>|—|container_id|opaque id/null|private|set `None` by bridge (`RD:396`)|
|project_scope|`D1:80`|Option\<String\>|—|project_scope|opaque id/null|private|set `None` by bridge (`RD:397`)|
|capabilities|`D1:81`|BTreeSet\<String\>|`capabilities,required_capability,capability`|capability[]|set of capability ids (≤256)|private|grant source = compiled profile via bridge (`RD:398-400`), never tags|
|plugin_id/method_key|`D1:109-110`|String|—|same|opaque id|private||
|transport_name|`D1:111`|String|`transport_name,method_key`|transport_name|opaque id|private|sealed-catalog transport alias|
|schema_hash / catalog_hash|`D1:112,114`|String|`schema_hash,catalog_hash,source_sha256`|same|lowercase hex sha256|hash-only|no hex/length validation (only ≤192B, `D1:120-131`)|
|schema_version|`D1:113`|String|—|schema_version|bounded string (semver?)|private|no format validation|
|subid|`D1:115`|String|—|operation_subid|opaque id|private|joins `CanonicalMappingRef.subid`|
|required_capability|`D1:116`|Option\<String\>|`capability,capabilities`|required_capability|capability id/null|private|missing grant ⇒ `Denied/capability_denied` (`D1:509-520`); `None`=no capability required — conflates “none required” vs “unknown”|

### `GateInput`, `GateResult`, `GateDiagnostic`, enums

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|caller / policy_owner_id / operation|`D1:174,177,178`|VerifiedCaller/String/ResolvedOperationRef|`owner_id,policy_owner_id`|verified_caller/policy_owner_id/operation_ref|object/id/object|private|backend-supplied only|
|classification / current_taxonomy_revision / resolved_tag / tag_associations|`D1:179-186`|Option\<Proposal\>/Option\<u64\>/Option\<ResolvedTagRef\>/Vec\<Association\>|—|same|object/u64/object/array|private|`resolved_tag` from registry, never proposal (`D1:182-184`)|
|policy_snapshot|`D1:189`|Option\<PolicySnapshot\>|—|policy_snapshot|object/null|private|`None`≠unprotected; ⇒Hold (`D1:187-188,488-490`)|
|policy_read_succeeded / marker_lookup_succeeded|`D1:190-191`|bool|`policy_read_succeeded,marker_lookup_succeeded`|same|bool|private|bridge hardcodes `true,true` (`RD:409-410`); failure ⇒ Hold `policy_unavailable`/`marker_lookup_unavailable` (`D1:476-487`)|
|requested_effect|`D1:192`|RequestedEffect enum|—|requested_effect|enum `local\|egress` (`D1:141-144`)|private|bridge hardcodes `Local` (`RD:411`) — egress path unreachable via bridge today|
|residual_validation_passed|`D1:193`|bool|—|residual_validation_passed|bool|private|bridge hardcodes `false` (`RD:412`)|
|destination_class / provider_route|`D1:194-195`|Option\<String\>|`destination_class,allowed_provider_routes`|same|enum/string|private|bridge hardcodes `None,None` (`RD:413-414`)|
|outcome|`D1:166`|GateOutcome enum|`outcome`|gate_outcome|enum `authorized_local\|authorized_egress\|denied\|unresolved\|hold` (`D1:148-154`)|private|verdict vocabulary; `Hold`≈pending/missing-evidence, `Unresolved`≈missing mapping/tag/taxonomy|
|rule_revision|`D1:167,222`|Option\<u64\>|`rule_revision,policy_revision`|gate_rule_revision|u64/null|private|`DecisionRecord.policy_revision` is this value renamed|
|diagnostic{code,detail}|`D1:158-161,168`|Option\<GateDiagnostic\>|—|gate_diagnostic.code/detail|bounded string (code should be enum)|private|codes seen: `invalid_context,invalid_classification,taxonomy_unavailable,taxonomy_revision_stale,policy_unavailable,marker_lookup_unavailable,owner_mismatch,marker_mismatch,capability_denied,policy_revision_unknown,policy_snapshot_oversized,policy_record_invalid,policy_snapshot_duplicate,tag_association_mismatch,resolved_tag_invalid,resolved_tag_unassociated,classification_resolution_mismatch,classification_tag_unresolved,method_denied,classification_unresolved,canonical_mapping_missing,canonical_mapping_unresolved,canonical_mapping_generation_mismatch,applicable_policy_missing,tag_constraint,egress_policy_denied,egress_policy_missing,context_changed` (`D1:428-746,750-760`)|

### `ExecutionReference`, `DecisionRecord`, `DecisionState`, method I/O

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|operation_id / event_id|`D1:201-202`|String|—|operation_id/event_id|opaque id (operation_id is UUID in bridge `RD:98`)|private|receipt linkage|
|event_hash / terminal_event_hash|`D1:203,205`|String|`event_hash,terminal_event_hash`|same|lowercase hex sha256|hash-only|no hash-format validation (`D1:307-315`)|
|terminal_event_id|`D1:204`|Option\<String\>|—|terminal_event_id|opaque id/null|private||
|observed_state_ref|`D1:206`|Option\<String\>|—|observed_state_ref|opaque ref/null|private|free string|
|outcome|`D1:207`|Option\<String\>|`outcome,gate outcome`|execution_outcome|enum (currently free String)|private|**mis-normalized** vs typed `GateOutcome`|
|decision_id|`D1:213,385`|String|—|decision_id|UUIDv7 string|private|generated `Uuid::now_v7()` in bridge (`RD:417`), typed as plain String|
|verified_identity|`D1:214`|VerifiedCaller|`verified_identity,caller,verified caller`|verified_caller|object|private|renamed vs `GateInput.caller`|
|operation_ref|`D1:215`|ResolvedOperationRef|`operation,operation_ref,candidate`|operation_ref|object|private||
|classification_proposal / classification_origin_verified|`D1:216,219`|Option\<Proposal\>/bool|`classification,classification_proposal`|same|object/bool|private|verified flag invariantly `false` in this slice (`D1:282-285`)|
|resolved_tag / policy_owner_id / policy_revision / canonical_mappings / gate_result / openflow_binding / execution_ref / shadow_mode|`D1:220-227`|Option\<\>/Option\<String\>/Option\<u64\>/Vec\<Ref\>/GateResult/Option\<Ref\>/Option\<Ref\>/bool|—|same (`policy_revision→gate_rule_revision`)|typed|private|`execution_ref` never populated by shadow path [INFERENCE from `D0:126ff` signature lacking it]|
|revision / records|`D1:235-237`|u64/Vec\<DecisionRecord\>|`records,decisions`|decision_revision/decision_record[]|u64/array ≤10,000|private|state|

Method I/O: `RecordDecisionInput{candidate,classification_proposal,shadow_mode:bool=default true}` (`D1:331-336`; non-shadow rejected `D1:347-351`), `RecordDecisionOutput{decision}` (`D1:357-359`), `ListDecisionsInput{limit,cursor}` / `ListDecisionsOutput{decisions,next_cursor}` (`D1:363-367,396-399`), `DecisionIdInput{decision_id}` (`D1:384-386`), `GetDecisionOutput{decision:Option}` (`D1:403-405`), `RecheckInput{previous:GateInput,current:GateInput}` / `RecheckOutput{result:GateResult}` (`D1:409-418`) — internal only, not a registered method (`D0:99-119`); recheck invalidates on caller/operation/policy change, “stale approvals are never reused” (`D1:749-760`).

## `routing_dispatch.rs` (field-bearing only)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|CallContext{actor,capability,session,genesis}|`RD:25-28`|String|`actor/principal_id,capability,session/session_id,genesis/session_genesis`|principal_id/capability/session_id/session_genesis|opaque ids|private|internal, non-serialized; verified session required (`RD:117-119`)|
|CallContext{admitted,engine,plugin,method,operation_id}|`RD:29-33`|PluginSchema/MutationEngine/&str/String/String|`plugin/plugin_id,method/method_key`|admitted_schema/—/plugin_id/method_key/operation_id|internal/id|private|internal only|
|PreparedShadow{input,operation,registry_state,policy_owner}|`RD:225-228`|RecordDecisionInput/ResolvedOperationRef/RegistryState/RegistryOwner|—|record_decision_input/operation_ref/registry_state/policy_owner|typed|private|shadow-only preparation|
|RoutingAdmission{decision,registry,registry_owner}|`RD:232-234`|Option\<RoutingReservation\>/Option\<RoutingReservation\>/Option\<RegistryOwner\>|—|decision_reservation/registry_reservation/registry_owner|object/null|private|reservations from `routing_resources` (out of scope)|

## `routing_store.rs` (field-bearing only)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|RoutingOwner{principal_id,session_id,session_genesis,owner_id}|`RS:32-35`|String|`session_genesis/genesis`|same|opaque ids, ≤192B (`RS:39-42`)|private|serialized `deny_unknown_fields`; `owner_id` derived from the triple [INFERENCE: field co-declared, derivation at `RS:39ff` truncated]|
|RegistryOwner{owner_id}|`RS:52`|String|`owner_id,policy_owner_id`|policy_owner_id|opaque id|private|configured policy owner (`RD:282-283`)|
|Owner::Decisions(RoutingOwner) / Registry(RegistryOwner)|`RS:56`|tagged enum `kind/scope`|—|owner_scope|tagged enum|private|document scoping|
|StoredState::Decisions(DecisionState)/Registry(RegistryState)|`RS:98`|tagged enum `kind/state`|—|stored_state|tagged enum|private||
|StoredDocument{format,owner,state}|`RS:114`|u32/Owner/StoredState|`format`|document_format_version|u32|private|persisted envelope|
|RoutingWake{plugin_id,revision,generation,schema_hash}|`RS:124-127`|&str/u64/u64/String|—|plugin_id/state_revision/generation/schema_hash|id/u64/u64/lowercase-hex|private/hash-only|change notification; `generation` here is wake generation, distinct from `CanonicalMappingRef.generation`|
|RoutingGuard/Inner/OwnerSlot fields|`RS:116-143`|internal handles|—|—|non-serialized|private|excluded from schema inventory: locks, pins, reservations only|

## (a) Duplicates / synonyms across slice
- Operation triple `plugin_id+method_key+schema_hash` repeated verbatim in `CandidateOperation`, `ResolvedOperationRef`, `SchemaTagAssociation{,Request}`, `ProtectedMethodMarker{,Request,KeyInput}`, `OpenFlowBindingRef`, `PolicyRule::DenyMethod` — one canonical `operation_ref` shape would serve all.
- Revision family: `revision` (tag/marker/policy/binding/state), `rule_revision`, `registry_revision`, `policy_revision` (in `DecisionRecord` = gate's `rule_revision`), `taxonomy_revision`, `required_policy_revision`, `expected_revision`.
- Tag identity: `tag_id:u32` vs `ClassificationProposal.tag:String` vs `TagDescriptor.name` vs `network_tag_id:u32` (alias of `tag_id`).
- Owner identity: `principal_id/actor`, `owner_id/policy_owner_id`, `session/session_id`, `genesis/session_genesis`.
- `records` (PolicySnapshot, DecisionState) vs `decisions` (ListDecisionsOutput) vs `policies/tags`.
- `reason` (DenyMethod, RevokePolicyInput) vs `revocation_reason` (PolicyRecord); `disposition` vs `default_disposition`; `provider_route` (singular) vs `allowed_provider_routes`; `caller` vs `verified_identity` vs `verified caller`.

## (b) Mis-normalized fields
- All `*_hash` (`schema_hash`, `catalog_hash`, `event_hash`, `terminal_event_hash`) are unbounded-format `String` (≤192B text only); only `source_sha256` enforces 64-hex, and not lowercase — canonical `lowercase hex sha256` unenforced anywhere.
- Free strings where enums belong: `ExecutionReference.outcome`, `GateDiagnostic.code`, `CanonicalMappingRef.direction` (validated but String-typed), `EgressRule.destination_class`, `provider_route(s)`, `schema_version`, `classifier_version`, `generation`, `observed_state_ref`.
- `ClassificationProposal.tag`: unbounded label string instead of `tag_id`; `cursor:Option<String>` and `decision_id:String` (actually UUIDv7) lack id typing; `required_capability:Option` conflates none/unknown.
- **No timestamp field exists in any record in this slice** — no `created_at`/RFC3339 field to canonicalize; provenance ordering rests solely on `revision` counters and hash links.
- No approval/human fields exist in this slice (grep: only comment “stale approvals are never reused”, `D1:749`): two-human/root approval for anchor-class routes is not representable here yet.

## (c) Tagged-routing / provenance relevance
- Tags never grant: gate checks `required_capability ∈ caller.capabilities` before policy allow (`D1:509-520`); bridge loads grants from compiled profile (`RD:398-400`), hardcodes local-only gate context (`RD:411-414`).
- Missing-capability verdict = `Denied/capability_denied`; missing evidence (policy/marker/mapping/tag/taxonomy/policy-rule/egress-rule) = `Hold`/`Unresolved` with codes listed above — fail-closed, never allow.
- Receipts: `ExecutionReference{operation_id,event_id,event_hash,terminal_event_*}` is the only receipt shape; `DecisionRecord` is shadow-only (`shadow_mode` enforced true) and bridge-generated `decision_id` is UUIDv7.
- Provenance chain available for private IP stream: `CanonicalMappingRef{source,source_ref,source_sha256,generation,resolved}` + `DecisionRecord{verified_identity,classification_origin_verified=false,gate_result,policy_revision,canonical_mappings}` + store `RoutingWake{schema_hash,generation}` — but no signer/wallet, anchor (`seq/merkle_root/chain_id/tx_ref`), or timestamp fields; those remain the spec's missing `web3` piece.