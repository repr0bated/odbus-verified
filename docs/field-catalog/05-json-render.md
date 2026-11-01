# Slice 5 — JSON render, canvas, and catalog

Source: FldJsonRender field inventory (read-only, cited). Integrated verbatim; canonical proposals are proposals, not renames applied to code.

Sealed/live verification is pending (door unreachable). Read-only inventory; no files changed.

**Path key:** `OP=/srv/git/odbus-verified/crates/op-plugins/src/state_plugins/`; `TS=/srv/git/3tched.com/src/json-render/`

## 1. Spec element — three incompatible admitted shapes

### `runtime_contract::spec_schema / element_schema` (general json-render spec)
|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|root|OP/json_render_runtime_contract.rs:34| string minLength 1 (required) | root (all) | root_element_id | string (element id) | public | keys `elements` map |
|elements|OP/json_render_runtime_contract.rs:35| object map → element oneOf | elements | elements | map<element_id, element> | public | |
|state|OP/json_render_runtime_contract.rs:36| object (optional) | state | state | object | private | spec-owned mutable state; only here + CanvasProfile |
|type|OP/json_render_runtime_contract.rs:53| const = ComponentDecl.name | type / component_type (GeneratedCanvas) | component_type | string (catalog name) | public | see §9 naming conflict |
|props|OP/json_render_runtime_contract.rs:54| per-component props_schema, each prop widened to literal ∪ expression | props | props | object (closed per component) | private | expressions: `$state/$item/$bindState/$bindItem/$template/$index/$computed/$cond/$format/$math/$concat/$count/$truncate/$pluralize/$join/$t` (:89-102) |
|children|OP/json_render_runtime_contract.rs:55| string[] — **required** (:66) | children | child_element_ids | string[] (element ids) | public | required here, optional in both Rust canvas shapes below |
|visible|OP/json_render_runtime_contract.rs:56,197-223| boolean \| comparison \| array \| `$and/$or` of visibilityCondition | visible | visible | visibility_condition | public | **only** in this shape; absent from both canvas validators and GeneratedCanvas |
|on|OP/json_render_runtime_contract.rs:57| object keyed by component.events → action binding {action,params,confirm,onSuccess,onError,preventDefault} (:133-137) | on | event_bindings | map<event, action_binding> | private | confirm:{title,message,confirmLabel,cancelLabel,variant} (:180-183) — camelCase inside |
|repeat|OP/json_render_runtime_contract.rs:58-63| {statePath (req), key} | repeat | repeat | {state_path, key?} | private | |
|watch|OP/json_render_runtime_contract.rs:64| object, additionalProperties = action binding | watch | watch_bindings | map<string, action_binding> | private | |
|statePath / value (setState etc.)|OP/json_render_runtime_contract.rs:146-169; OP/json_render.rs:1314,1329,1343,1371| string minLength 1 / any | statePath, clearStatePath, screen, index | state_path, clear_state_path, screen_id, index | string (JSON Pointer) / u32 | private | built-in actions: setState, pushState, removeState, push, pop, validateForm (json_render.rs:1311-1394) |

### `GeneratedCanvas / GeneratedCanvasElement` (application-mode generation)
|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|root|OP/json_render_generation.rs:17|String|root|root_element_id|string|public||
|elements|OP/json_render_generation.rs:18|BTreeMap<String, GeneratedCanvasElement>|elements|elements|map<element_id, element>|public||
|state|OP/json_render_generation.rs:20|BTreeMap<String, Value> (default, skipped if empty)|state|state|object|private||
|target_contracts|OP/json_render_generation.rs:22|BTreeMap<String,String>|— (unique to this record)|target_contracts|map<string,string>|private|extra top-level field the other three shapes reject (deny_unknown_fields + closed key sets)|
|component_type (`type`)|OP/json_render_generation.rs:28-29|String, `#[serde(rename="type")]`|type|component_type|string|public|Rust-side rename already signals the conflict|
|props|OP/json_render_generation.rs:30|BTreeMap<String, Value>|props|props|object|private|unvalidated at this layer; ApplicationValidator compiles props_schema (json_render_application.rs:48-65)|
|children|OP/json_render_generation.rs:32|Option<Vec<String>>|children|child_element_ids|string[]|public|optional here vs required in runtime schema|
|on|OP/json_render_generation.rs:34|Option<BTreeMap<String, Value>>|on|event_bindings|map<event, binding>|private|no `visible/repeat/watch` fields exist on this struct|

### Canvas validators (element key sets actually admitted)
|validator|source|admitted top-level|admitted element keys|notes|
|---|---|---|---|---|
|PinnedCanvasValidator (vercel-chat-canvas-static-v1)|OP/json_render_canvas_profile.rs:247-280|root, elements, state (≤3 keys)|type, props, children, **on** (≤4 keys; type/props/children all required)|literal props only (`literal_props_only`, :41); no visible/repeat/watch; element id: 1-80 chars `[A-Za-z0-9_-]` (chat_canvas.rs:310-318, shared pattern)|
|ChatCanvasValidator (3tched-chat-canvas-v1)|OP/json_render_chat_canvas.rs:232-264|**exactly** root, elements (2 keys)|type, props, children (children optional)|static only: no on/visible/state; components must have empty events (:154-157)|

## 2. Core records — `json_render.rs`

### JsonRenderState (OP/json_render.rs:47-119)
|field_as_found|source|declared_type|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|
|ui_authoring|:52|UiAuthoring|ui_authoring|object|public||
|canvas_profile|:57|CanvasProfile|canvas_profile|object|public|plugin-owned profile|
|chat_canvas_profile|:63|ChatCanvasProfile|chat_canvas_profile|object|public|imported from UI export|
|status|:67|String|status|enum string|public|free string, default `"catalog_snapshot"` (:588)|
|source|:71|JsonRenderSource|source|object|public|provenance, below|
|config|:75|JsonRenderConfig|config|object|public||
|spec_contract|:79|SpecContract|spec_contract|object|public|below|
|packages|:83|Vec<JsonRenderPackage>|packages|array|public||
|core_exports|:87|Vec<ApiExport>|core_exports|array|public||
|renderers|:91|Vec<RendererSurface>|renderers|array|public||
|components|:95|Vec<ComponentDecl>|components|array|public|upstream shadcn inventory (catalog_v019)|
|actions|:99|Vec<ActionDecl>|actions|array|public||
|validation_checks|:103|Vec<NamedCapability>|validation_checks|array|public|names incl. camelCase minLength/maxLength/equalTo/lessThan/greaterThan (:1398-1426)|
|directives|:107|Vec<NamedCapability>|directives|array|public|`$state…$t` vocabulary (:1439-1474)|
|catalog_revision|:112|u64|catalog_revision|u64|public|provenance: durable active-catalog revision, 0 until bridge projects|
|active_catalog_entry_names|:118|Vec<String>|active_catalog_entry_names|string[]|public|routing: names usable for normal chat composition|

### ComponentDecl / ActionDecl / SpecContract / others
|record.field|source|declared_type|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|
|ComponentDecl.name/.category/.props/.props_schema/.slots/.events/.description|OP/json_render.rs:350-372|String/String/Vec<String>/JsonValue/Vec<String>/Vec<String>/String|component_name, category, prop_names, props_schema, slot_names, event_names, description|string, string, string[], JSON Schema, string[], string[], string|public|props (name list) duplicates props_schema.properties — two sources of truth|
|ActionDecl.name/.params/.params_schema/.renderers/.mutates_state/.description|:382-398|String/Vec<String>/JsonValue/Vec<RendererTarget>/bool/String|action_name, param_names, params_schema, renderer_targets, mutates_state, description|…|public|same params/params_schema duplication|
|SpecContract.root_field/.elements_field/.state_field/.element_fields/.patch_ops|:231-247|String×3, Vec<String>×2|root_field, elements_field, state_field, element_fields, patch_ops|strings|public|element_fields default = [type, props, children, visible, on, repeat, watch] (:256-264) — superset of every validator's admitted set|
|JsonRenderSource.docs_url/.repository_url/.commit/.summary|:128-140|String×4|docs_url, repository_url, source_commit, summary|string, url, git sha string, string|public|provenance; commit pinned const e2d00f… (:41)|
|JsonRenderConfig.expose_catalog/.expose_prompt_surface/.schema_mode/.default_renderer|:162-174|bool/bool/JsonSchemaMode{Standard,Strict}/RendererTarget (kebab-case enum)|same|bool/bool/enum/enum|public|deny_unknown_fields|
|JsonRenderPackage.folder/.package_name/.version/.role|:284-296|String×4|folder_name, package_name, package_version, package_role|strings|public||
|ApiExport.name/.package/.kind/.category|:306-318|String×4|export_name, package_name, export_kind, category|strings|public|`package` vs `package_name` inconsistency vs JsonRenderPackage/RendererSurface|
|RendererSurface.id/.package_name/.target/.exports|:328-340|String/String/String/Vec<String>|renderer_id, package_name, renderer_target, export_names|…|public||
|NamedCapability.name/.category/.description|:408-416|String×3|capability_name, category, description|strings|public||
|MethodSurface.name/.side_effect/.required_capability/.subid/.available|:426-440|String×4, bool|method_name, side_effect, required_capability, method_subid, available|…|public|routing: capability gate per method|
|Inputs: config / name / package_name / spec:JsonValue / mode:Option<JsonSchemaMode> / mode:Option<PromptMode>+custom_rules|:449-492|—|config, component_name, package_name, spec, schema_mode, prompt_mode, custom_rules|…|private (spec)|deny_unknown_fields on all inputs|
|Outputs: Health{status,package_count,component_count,action_count,source_commit,catalog_kind} :496-502; Config{config,revision} :506; SetConfig{+changed} :513; ValidationResult{valid,issues,checked_elements} :564-567; SchemaExport{mode,schema} :571; PromptSurface{mode,renderer,prompt} :578|—|*_count → u64 counts; source_commit → git sha|public; prompt private|revision (u64) is provenance for config mutations|

## 3. Canvas validation limits — four conflicting limit sets

|limit record|source|fields & values|canonical|notes|
|---|---|---|---|---|
|JsonLimits|OP/json_render_validation_bounds.rs:9-13|max_depth, max_nodes, max_bytes (usize)|json_max_depth/nodes/bytes|generic gate|
|GraphLimits|:17-22|max_elements, max_edges, max_depth, max_expanded_nodes|graph_max_*||
|CanvasProfile defaults|OP/json_render_canvas_profile.rs:79-89|json {64, 65_536, 1 MiB}; graph {4096, 16_384, 64, 4096}|—|validator also caps any sealed profile at exactly these ceilings (:145-163) and requires exactly 8 components, 1 action (:164-174)|
|json_render_methods validate_spec|OP/json_render_methods.rs:22-26,341-343,454-457|MAX_SPEC_BYTES 4 MiB, MAX_ELEMENTS 4096, edges 16_384, depth 128, expanded 65_536, nodes 262_144|—|**conflicts** with CanvasProfile: 4× bytes, 2× depth, 16× expanded nodes|
|ChatCanvasProfile|OP/json_render_chat_canvas.rs:45-48|u64 max_bytes/max_elements/max_depth; imported values 200_000 / 256 / 20|—|validator ceiling 1 MiB/4096/64 (:139-141)|
|ApplicationProfile Limits|OP/json_render_application.rs:18; profile JSON:3-7|max_bytes 200_000, max_elements 256, max_depth 20 (usize)|—|same numbers as chat canvas, different field type (usize vs u64)|
|props-schema self-limit|canvas_profile.rs:192-196; chat_canvas.rs:164-168|JsonLimits{depth 32, nodes 4096, bytes 65_536} for each component props_schema|—|identical in both validators|

## 4. Registered component fields

### CanvasProfile.components (8, OP/json_render_canvas_profile.rs:472-566) — ComponentDecl shape (§2)
container{className}, autoTile{columns,gap,rowHeight,boardId,reorderable,fit,className; event reorder}, autoTileItem{size,contentType,tileId,className; event resize}, heading{text,level∈1..3}, text{text,className}, badge{text,label,tone∈default/ok/warn/danger/info}, row{className}, stat{label,value,unit,trend∈up/down/flat}. All props_schema properties are **required incl. nullable ones** (e.g. container requires className, :478 in file order). Sole action: setState{statePath,value} mutates_state (:64-77).

### ChatCanvasProfile.components (23, imported OP/json_render_chat_canvas_contract.rs:9; parsed chat_canvas.rs:69-74)
ExportComponent fields: name, description, slots, props_schema (→ ComponentDecl with category="chat_canvas", events=[], props derived from props_schema.properties keys, :87-100). Names/props identical to TS dashboard+chat subset: dashboardPanel…dashboardTopology, notebookHeader, notebookSourceList, container, pageHeader, heading, text, badge, row, stat (see §7). Evidence: ChatCanvasEvidence{repository, source_revision, profile_sha256, assurance="source_checked"} (chat_canvas.rs:29-36); CanvasImplementationEvidence adds catalog_sha256, registry_sha256, package_lock_sha256, core_version, react_version (canvas_profile.rs:17-27) — all `*_sha256` lowercase hex, **hash-only** sensitivity.

### catalog_v019 upstream components (36, OP/json_render_catalog_v019.rs:8-298)
Same ComponentDecl record; names PascalCase: Card, Stack, Grid, Separator, Tabs, Accordion, Collapsible, Dialog, Drawer, Carousel, Table, Heading, Text, Image, Avatar, Badge, Alert, Progress, Skeleton, Spinner, Tooltip, Popover, Input, Textarea, Select, Checkbox, Radio, Switch, Slider, Button, Link, DropdownMenu, Toggle, ToggleGroup +2. Props camelCase inside props_schema JSON (maxWidth, centered, className, defaultValue, defaultOpen, openPath…). **Not admitted by either canvas validator** — reference inventory only (state default_components, json_render.rs:1307-1309).

### ApplicationProfile (OP/json_render_application.rs:10-35; JSON :1-1273)
ApplicationProfile{format="3tched.application-profile.v1", limits, components, actions}; Component{name, description, slots, events, props_schema, required_bindings: Vec<String>, string_item_fields: BTreeMap<String,Vec<String>>} (:21-32); Action{name, description, params_schema} (:35). Component set = chat-canvas 23 **plus** applicationOverlay{panel,title,collapsed; events open/close}, applicationInput{label,value; event change; required_bindings=[value]}, applicationButton{label; press}, applicationSelect{label,value,options,valueField,labelField; change; required_bindings=[value]; string_item_fields={options}} (profile JSON components 24-27). Actions: setState{statePath,value}, **invokePlugin{plugin, method, capability, arguments, resultStatePath?}** — the only capability-routed action in the slice; exists only in this JSON profile, not in ActionDecl defaults.

### TS RegisteredComponent (TS/references/registered-components.ts:4-8)
|field|source|type|canonical|sensitivity|notes|
|---|---|---|---|---|---|
|name|:5|string|registered_component_name|string|public|two name grammars conflict: definitions require `NAME=/^[A-Z][A-Za-z0-9]{1,48}$/` (:10, used :86) but promoted adoption uses `CATALOG_COMPONENT_NAME=/^[A-Za-z][A-Za-z0-9]*(?:[ ._'-][A-Za-z0-9]+)*$/` + ≤80 chars (:12, :45)|
|description|:6|string (≤400, :88, :52)|description|string|public||
|spec|:7|Spec (validated root/elements)|spec|spec object|private|MAX_COMPONENTS=32 (:13) vs Rust chat profile ≤256 components, canvas profile exactly 8|

## 5. schema_renderer records (OP/schema_renderer.rs, schema_renderer_methods.rs)
|record.field|source|declared_type|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|
|TargetPluginInput.plugin_id|:17 (schema_renderer.rs)|String|plugin_id|opaque id|public|unbounded string for an id|
|GalleryLookupInput.gallery_id / .rendering_id|:27-28|String|gallery_id, rendering_id|opaque ids|private|**renamed in transit**: rendering_id → `spec_id` when calling get_ui_spec (methods:48); gallery_id must equal const `"workspace"` (methods:20,39)|
|RenderSchemaOutput.plugin_id/.schema_version/.schema_hash/.renderer_schema_hash/.canvas_profile_id/.rendering/.field_count/.method_count|:34-42|String×5, Value, u64×2|plugin_id, schema_version, schema_hash→target_schema_sha256, renderer_schema_sha256, canvas_profile_id, rendering, field_count, method_count|hashes: lowercase hex sha256|hash-only (hashes); rendering private|schema_hash algorithm not named in field; renderer hash validated as 64 lower-hex only in canvas_profile.rs:112-117|
|ValidateSchemaOutput{plugin_id, schema_version, schema_hash, renderer_schema_hash, valid, errors, checked_fields, checked_methods}|:47-55|…|same|hash-only/public|summary caps: MAX_SUMMARY_ITEMS 512 fields+methods, MAX_ID_BYTES 128 (methods:17-18,242)|
|GalleryDescription.gallery_id/.document_revision/.renderer_schema_hash/.rendering_count|:61-65|String,u64,String,u64|gallery_id, document_revision, renderer_schema_sha256, rendering_count|hash-only for hash|document_revision is provenance|
|GetRenderingOutput.gallery_id/.rendering: Option<UiSpecRecord>/.render_admitted|:76-80|…|render_admitted|bool|private| stale artifacts inspectable, not renderable (:78)|
|GalleryTarget.method/.capability/.args|methods:22-26|&str/String/Value|target_method, required_capability, arguments|…|private|routing triple produced by plan_gallery_method|

Generated rendering elements reuse chat-catalog types text/heading with props {text, className} / {text, level:3} (methods:253-274) — field/method counts rendered as text, no structured tag fields.

## 6. Gallery records (OP/json_render_gallery.rs) — provenance-bearing
Candidate{:124-140}: candidate_id, principal_id (**private**), title (≤200 B, :26), mode (∈ MODES personal/project/control_plane/accountability/notebook_lm/ui_render :32-39 — String, not enum), prompt: Option<String> (≤8 KiB, **private**), spec: Value (**private**), spec_hash, profile_sha256 (**hash-only**), created_at: String, promoted_entry_id: Option<String>.
CatalogEntry{:193-211}: entry_id, name, description (≤2000 B), spec, spec_hash, profile_sha256, from_candidate_id, promoted_by (**private**), promoted_at: String, status: String ("active"|"retired" in comment only), retired_at/retired_by/retire_reason: Option.
Inputs: SaveCandidateInput{spec,title,mode,prompt?} :58-67; PageInput{before: Option<String>, limit: u32 (0=default 50, max 100)} :77-84; PromoteInput{candidate_id,name,description} :100-106; RetireInput{entry_id,reason} :116-120. Summaries drop principal_id/prompt/spec (CandidateSummary :144-152) — correct sensitivity split already present.
Authoring: UiAuthoring{format, instructions, prompt_document_paths, documents} and AuthoringDocument{path, content, repository, revision, source_path, sha256, upstream_sha256?, adaptation?} (json_render_authoring.rs:15-61) — provenance-complete; sha256/upstream_sha256 **hash-only**.

## 7. TS catalogs (schema field names only)

Shared per-component record shape (all three files): `{props: z.object(...).strict? , slots: string[], events?: [...] as const, description: string}` — maps to Rust ComponentDecl{props_schema, slots, events, description} minus name/category. dashboard/chat use non-strict z.object (unknown props pass Zod, closed only after JSON-Schema export); ui-elements uses `.strict()` everywhere.

**chat-catalog.ts** (`chatCatalog`, :15-192): catalogComponent, notebookHeader, notebookSourceList imported from dashboard-elements; inline: container{className}, pageHeader{title,subtitle}, interfaceGeneration{}, workbenchNavigation{}, tchedRouterPicker{className}, chatModeSelector{}, antigravityChatContainer{streamId,title,mode∈live/accountability,placeholder,workspace,active,open} (:51-59), menuBar{label}, menuSection{id,label}, menuItem{label,route,wip,placeholder}, generatedRegion{view,title,open,views:record, status:record; events missing/retry}, routeView{view,title,views,status; missing/retry}, chatMessage{role∈user/agent/system, content}, chatThread{streamId}, autoTile{…same as Rust canvas}, autoTileItem{…}, heading{text,level∈1|2|3}, text{text,className}, badge{text,label,tone∈default/ok/warn/danger/info}, row{className}, stat{label,value,unit,trend}. Actions (:182-191): setState{statePath,value:unknown}, generateView{view,retry} — generateView has **no Rust counterpart** in ActionDecl defaults.
Types: `shortText=z.string().max(400)`, `text=max(20_000)` (dashboard-elements.ts:7-8); most chat props are `.nullable()` (key required, value nullable) matching Rust required+nullable schemas.

**dashboard-elements.ts** (`dashboardElementSchemas`, :27-158): dashboardPanel{title,subtitle?,tone}, dashboardDisclosure{title,summary?,initiallyOpen?}, dashboardMetric{label,value,detail?,tone?}, dashboardKeyValues{title?,rows:{label,value}[]≤100}, dashboardTable{caption,columns:{id,label}[]1-16,rows:{id,cells[]}≤500,emptyLabel?}, dashboardTabs{label,initialTab?,tabs:{id,label,content}[]≤12}, dashboardFormPreview{title,fields:{id,label,type∈text/email/number/textarea/select/toggle,value?,options?,checked?,help?}≤30}, dashboardProgress{label,value 0-100,detail?}, dashboardBarChart{title,bars:{label,value 0-100}≤30}, dashboardEventLog{title,entries:{time,title,detail?,tone?}≤100}, dashboardStatus{label,detail?,tone}, dashboardCode{title,language?,code}, dashboardTree{title,nodes:treeNode{label,detail?,children?}≤100}, dashboardTopology{title,nodes:{id,label,detail?,x 0-100,y 0-100}≤100,edges:{from,to,label?}≤200}, notebookHeader{title,sourceCount int 0-100_000,url?,emoji?}, notebookSourceList{sources:{id,title}[]≤200}, catalogComponent{name: string ≤80 regex CATALOG_COMPONENT_NAME}. `tone=z.enum([neutral,info,success,warning,danger])` (:6). `dashboardCatalog` = same components, actions {} (:163-166).

**ui-elements.ts** (`uiElementSchemas`, 86 components, :14-834; helpers short≤400/label trim 1-400/text≤20_000/tone as dashboard/fragment=#…≤200/options label[]1-100 :3-11). Grouped by prop signature:
- label-only, slots[default]: uiStack, uiInline, uiCenter, uiCluster, uiCard, uiPanel, uiSection, uiWell, uiInset (:15-69); +columns int1-6: uiGrid (:25-34); +ratio 0.25-4: uiAspectRatio (:75-84); label required: uiScrollArea (:70-74)
- label-only, no slots: uiDivider (:85-89); size int0-128: uiSpacer (:90-96); legend+disabled, slots: uiFieldset (:97-103)
- text-only: uiTitle, uiSubtitle, uiParagraph, uiLead, uiCaption, uiLabel, uiHelperText, uiInlineCode, uiKeyboardKey, uiAddress, uiMark (:104-158); code+language?: uiCodeBlock (:159-163); text+attribution?: uiBlockquote (:164-168); text(short)+expansion: uiAbbreviation (:169-173); datetime+text (both short): uiTimestamp (:174-178)
- title?+message: uiAlert, uiNotice, uiSuccessMessage, uiWarningMessage, uiErrorMessage, uiInfoMessage, uiToast (:179-213); label+tone?: uiStatusBadge, uiStatusDot (:214-223); label+value 0-100: uiProgress, uiMeter (:224-237); label: uiSpinner (:238-242); label+lines?1-10: uiSkeleton (:243-252); title+description?: uiEmptyState (:253-257); message: uiValidationMessage (:258-262)
- inputs, all events[change] except uiButton/uiIconButton[press]: label+disabled?: uiButton (:263-270); label+icon∈add/close/search/settings/more+disabled?: uiIconButton (:271-282); label+initialPressed?+disabled?: uiToggleButton (:283-294); label+initialValue?(short)+disabled?+required?: uiTextInput, uiSearchInput, uiEmailInput, uiPasswordInput, uiUrlInput, uiTelInput, uiNumberInput, uiDateInput, uiTimeInput, uiDateTimeInput, uiMonthInput, uiWeekInput, uiColorInput (:295-476, identical signature ×13); label+initialValue?(text)+rows?2-20+disabled?: uiTextarea (:477-489); label+initialChecked?+disabled?: uiCheckbox, uiSwitch (:490-513); label+options+initialValue?+disabled?: uiRadioGroup (:514-526), uiSelect (:527-539); label+options+disabled?+initialValues?[]≤100: uiMultiSelect (:540-552); label+initialValue?0-100+disabled?: uiRange (:553-564); label+multiple?+disabled?: uiFileInput (:565-576); label+submitLabel, slots, event submit: uiForm (:577-583)
- label+items(text[]≤100): uiUnorderedList, uiOrderedList, uiSearchResults (:584-593, 829-833); items{term,description}≤100, **no label**: uiDescriptionList (:594-602); caption+columns(label[]1-16)+rows(short[][]≤100): uiTable (:603-618); name: uiAvatar (:619-623); label+names[]≤20: uiAvatarGroup (:624-628); label: uiTag (:629-633); label+tags(short[]≤100): uiTagGroup (:634-638); label+items{label,href:fragment}1-20: uiBreadcrumbs (:639-651); label+pages1-100+initialPage?, change: uiPagination (:652-667); label+tabs{label,content}1-20, change: uiTabs (:668-681); label+items{title,content}≤30: uiAccordion (:682-691); title+initiallyOpen?, slots: uiDisclosure (:692-698); label, slots: uiToolbar, uiButtonGroup (:699-709); label+href: uiLink (:710-714); label+filename(/^[a-zA-Z0-9_-]+\.txt$/≤80)+content: uiDownloadLink (:715-728); label+steps[]1-20+current int0-19: uiStepper (:729-743); label+items{title,detail,time}≤100: uiTimeline (:744-755); label+initialValue?1-5+disabled?, change: uiRating (:756-767); label+value(short)+detail?: uiStatistic (:768-774); amount+currency+period? (all short): uiPrice (:775-781); label+value(text): uiKeyValue (:782-786); title+description?, slots: uiListItem (:787-791); title+message: uiNotification (:792-796); title+message+tone?, slots: uiBanner (:797-803); label+text: uiTooltip (:804-809); label+title+text, slots: uiPopover (:810-814); trigger+title+description?+closeLabel, slots, event close: uiDialog (:815-828)

## (a) Duplicate / synonym fields across the slice
- `heading{text,level}` in chat-catalog:144-151, application/chat-canvas profiles, CanvasProfile (canvas_profile.rs:476ff) vs v019 `Heading{text,level∈h1..h4 string}` (catalog_v019.rs:99-105) vs ui-elements has **no** heading — uiTitle/uiSubtitle{text}. Level is number 1-3 (TS/Rust canvas) vs string enum (v019).
- `text` component: chat{text,className} vs v019 `Text{text,variant∈body/caption/muted/lead/code}` (catalog_v019.rs:107-113) vs uiParagraph etc.{text}.
- `badge`: chat/canvas badge{text,label,tone∈default/ok/warn/danger/info} vs v019 Badge (props_schema in catalog_v019) vs uiStatusBadge{label,tone∈neutral/info/success/warning/danger} — three tone vocabularies; dashboard `tone` enum = ui tone ≠ chat badge tone.
- Columns/rows: dashboardTable columns:{id,label}[], rows:{id,cells[]} vs uiTable columns:label[], rows:string[][] vs v019 Table columns:string[], rows:string[][] (catalog_v019.rs:91-97) vs Grid/autoTile `columns:number`.
- `value`: stat.value (string), dashboardMetric.value (shortText), uiProgress/uiMeter.value (number 0-100), dashboardProgress.value (number), action param value (unknown), applicationInput.value (bound state) — one name, four types.
- `name`: ComponentDecl.name, CatalogEntry.name, RegisteredComponent.name, catalogComponent.props.name, ApiExport.name, MethodSurface.name — component identity vs display name conflated; catalogComponent.name is a *reference*, others are declarations.
- `id` fields: streamId, boardId, tileId, sourceCount… plus element-map keys (unnamed id), menuSection.id, dashboardTable column/row `id`, topology node `id` — all String/shortText, no shared id type.
- `status`: JsonRenderState.status, HealthOutput.status, generatedRegion/routeView status:record<string,string>, CatalogEntry.status, dashboardStatus component — unrelated semantics.
- rendering_id ↔ spec_id (schema_renderer_methods.rs:48); gallery_id ↔ workspace const; `package` (ApiExport) ↔ package_name (elsewhere); props (Vec<String> names) ↔ props_schema.properties (ComponentDecl, ActionDecl params↔params_schema).

## (b) Mis-normalized fields
- **camelCase wire names throughout** vs snake_case Rust structs: className, statePath, clearStatePath, openPath, confirmLabel, cancelLabel, preventDefault, onSuccess/onError, streamId, boardId, tileId, contentType, sourceCount, initiallyOpen, initialValue(s)/initialChecked/initialPressed/initialPage/initialTab, submitLabel, closeLabel, valueField/labelField, resultStatePath, minLength/maxLength (validation checks), catalog_v019 props (maxWidth, defaultValue…). Canonical proposal: keep wire name but register canonical snake_case alias (`class_name`, `state_path`, …) in the tagged schema; do not silently rename on the wire.
- **PascalCase component names** (v019 Card/Heading, RegisteredComponent NAME regex) vs camelCase (chat/dashboard: heading, dashboardPanel) vs ui-prefixed camelCase (uiStack) vs application-prefixed (applicationInput) — same registry namespace, four conventions; RegisteredComponent definitions demand PascalCase while promoted entries allow spaces/apostrophes (registered-components.ts:10 vs :12).
- **Unbounded strings where id/hash/enum/timestamp belongs:** plugin_id, gallery_id/rendering_id (only length-checked at call sites), Candidate.created_at / CatalogEntry.promoted_at/retired_at (String, no RFC3339 format constraint in the record), Candidate.mode / CatalogEntry.status (String with comment-only enum), JsonRenderState.status, schema_hash (algorithm unnamed; only renderer_schema_hash is length/hex-validated, canvas_profile.rs:112-117), GalleryDescription.renderer_schema_hash. Canonical: `*_id` opaque bounded string, `*_hash`/` *_sha256` lowercase hex sha256 (64), timestamps RFC3339 UTC.
- **Required-nullable props:** Rust canvas/chat schemas require every prop key incl. nullable (className, subtitle…), TS uses `.nullable()` (required) in chat-catalog but `.optional()` in dashboard/ui-elements for the same concepts (subtitle, detail, tone) — exported JSON Schemas therefore disagree on `required` for identical components (dashboardPanel in chat-canvas contract requires only title; TS dashboardPanel subtitle optional — consistent there, but badge/stat require all four in Rust profiles while TS chat badge/stat also require keys with nullable values; ui-elements prefers optional). Pick one: nullable-required (canvas profiles) is the de-facto canonical.
- `level`: number (chat) vs string enum (v019) vs uiTimestamp `datetime` (short string, no format). `time` in dashboardEventLog entries and uiTimeline items is shortText, not a timestamp.
- Limits typed usize (Rust internal) vs u64 (ChatCanvasProfile) for identical max_bytes/max_elements/max_depth; page `limit: u32` + `before: Option<String>` cursor named `before`/`next_before` — canonical `cursor`/`next_cursor`.
- `props_schema` JsonValue/Value everywhere: unbounded nested string keys ($ref banned only by validator scan, not by type).

## (c) Tagged routing / provenance relevance
- **No generic `tags[]` field exists in any record in this slice.** Routing signals present instead: MethodSurface.required_capability + GalleryTarget{method,capability,args}; invokePlugin{plugin,method,capability,arguments,resultStatePath} (application profile only); Candidate.mode (6-value chat-mode tag, String-typed); CatalogEntry.status; JsonRenderState.active_catalog_entry_names + catalog_revision; profile ids as format tags (`3tched.canvas-profile.v1`, `3tched.application-profile.v1`, `vercel-chat-canvas-static-v1`, `3tched-chat-canvas-v1`, authoring format `threetched-ui-authoring-v1`).
- **Provenance hashes/receipts:** source.commit + JSON_RENDER_SOURCE_COMMIT; CanvasImplementationEvidence{catalog_sha256, registry_sha256, package_lock_sha256, source_revision, assurance="source_checked"}; ChatCanvasEvidence{profile_sha256, source_revision}; Candidate/CatalogEntry{spec_hash, profile_sha256, from_candidate_id, promoted_by/at, retired_by/at}; schema_renderer{schema_hash, renderer_schema_hash, document_revision, field_count/method_count as receipt counts}; AuthoringDocument{sha256, upstream_sha256, revision, adaptation}; ConfigOutput/SetConfigOutput{revision, changed}. All hash fields are plain String in the records — validators, not types, enforce hex/length (canvas_profile.rs:112-117; gallery text() bounds :254-264). Canonical: rename/alias to `*_sha256` (lowercase hex sha256) and mark **hash-only**; principal_id, promoted_by, retired_by, prompt, and gallery/application `spec`/`rendering` payloads are **private** and already excluded from summary records.
- Admission receipts: GetRenderingOutput.render_admitted, ValidationResultOutput{valid,issues,checked_elements}, ValidateSchemaOutput{valid,errors,checked_fields,checked_methods} — decision records exist but carry no signer/hash of the validator profile beyond canvas_profile_id + renderer_schema_hash on RenderSchemaOutput.