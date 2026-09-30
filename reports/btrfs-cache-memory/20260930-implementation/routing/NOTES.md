# Real contracts for local routing/UI scaffolding

Read-only source and captured sealed-catalog inspection only. No network mutation, inference, local model installation, configuration change or grant change. The accompanying `*-sealed-contract.json` files contain selected real input/output DTOs, exact source schema hashes, capabilities, side effects and OSCAL method subids; they are evidence snapshots, not a replacement schema authority.

## Model-agnostic Gemma surface

`gemma_brain_methods.rs:14-114,248-305` defines a private revisioned `GemmaTaxonomy` and `GemmaTag {id,label,description,keywords,perspectives}`. Classification is deterministic local substring matching, counting matched keywords and sorting score descending then tag ID; it does not call a model. Input is bounded to 64 KiB, taxonomy to 256 tags, keyword count to 64, result limit to 64. Registration uses `expected_taxonomy_revision` and validates before mutation. This is an actual local classifier, not evidence of an installed Gemma model/runtime.

Declared methods: `classify`, `analyze_intent` and `register_tag` require `gemma.invoke`; `list_tags`/`list_perspectives` require `gemma.read`. The private workspace dispatcher validates owner/session, sealed schema and workspace activation (`ui_workspace_dispatch.rs:373-425`). A UI may stage a taxonomy input, but should not claim classification was executed unless an admitted call returned its revision/results.

`gemma_brain.route` is a facade for `tched_router.ResolveRoute {hint}` (`gemma_brain_dispatch.rs:13-44,56-97`). It requires both the outer `gemma.invoke` grant and nested exact `cap.software.3tched-router.route.resolve@v1` grant under a verified session. `ResolveRoute` matches a declared model-route hint or model ID (`tched_router.rs:2344-2360`). It does not accept `schemaTag`, provider/model candidate pair or execution role.

`GemmaRouting {subid_classification,tag_routing,xray_config_path}` exists at `gemma_brain.rs:29-42`; these fields have no current Rust callers outside declarations. Their comments do not establish a tag-to-OpenFlow/Xray router. The retained Gemma plugin name is not a local Gemma model identity DTO.

## Schema authority and local draft shape

`PluginSchema` owns method argument/result schemas, required capability, side effect and method subid (`plugin_schema.rs:129-145,264-312`). Explicit `schema.tags` are separate from OSCAL method subids and from the Gemma keyword taxonomy. Captured sealed Gemma/OpenFlow/OVSDB/Xray/tched_router schemas all currently have `tags: []`; do not fabricate selectable schema tags from network labels. Their captured category is also `uncategorized` despite some source annotations.

Use a local typed draft such as `{schemaTag,candidateProviderId,candidateModelId,executionRole}` only as a proposed association. Options for candidate IDs come from the actual registered provider/model catalog. A real schema reference should also retain plugin ID, source schema hash and selected method/subid. If the UI allows a free-form proposed `schemaTag`, label it as a draft; a real declared tag picker is presently empty. Local intent tags do not grant capabilities or verify compliance controls.

Existing mutation `tched_router.SetOvsRoutingModel {model_id}` has exact capability `cap.software.3tched-router.model-assignments.ovs-routing.set@v1`. It durably assigns a model role through `tched_router_legacy_dispatch.rs:57-88`, but does not accept provider/tag/role binding and does not install flows. Do not invoke it when saving a local draft.

For a source plugin scaffold, reuse `plugin_scaffold.rs.template:1-75` and `plugin_scaffold_helpers.rs`, deriving the input/output/state from Rust DTOs through schemars. Do not introduce a parallel JSON contract as production authority. A future draft-registration method needs its own real DTO, owner/revision binding and separate declared capability; no current method publishes this four-field association.

## Distinct network execution roles

| Role | Existing contract | Boundary |
|---|---|---|
| OVS topology/configuration | `ovsdb_bridge.list_dbs/get_schema/monitor/monitor_cond` (`ovsdb_bridge.rs:23-61,472-523`), topology state from `ovsdb_topology.rs` | OVSDB manages bridge/port/interface configuration. Monitor is constrained to non-secret topology tables/columns. `ovsdb.read` authorizes these reads; `ovsdb.write` authorizes actual topology changes. |
| OpenFlow dataplane | `openflow.get_datapath_health {bridge}`; exact `add_flow {bridge,flow}`, `modify_flow {bridge,expected,replacement}`, `delete_flow {bridge,expected}` (`openflow.rs:1651-1784`) | Packet matches/actions/table/priority. Mutation uses full exact members and native compare/exchange readback (`openflow_methods.rs:104-155,172-213`), with `openflow.write`. Read health requires `openflow.read`. No schema-tag/model intent is a flow argument. |
| Xray proxy | `xray.get_stats {name}`, `add_user`, `remove_user`, `add_inbound` (`xray.rs:306-366,478-634`) | Application proxy handlers and counters. Supported handlers are VLESS/VMess/Trojan, TCP TLS or explicit loopback plaintext. Runtime handler mutations require `xray.write`; counters require `xray.read`. Receipts do not claim bootstrap-file persistence. General tag-routing/restart/tracing methods are absent. |

Minimal read examples: OpenFlow health `{"bridge":"ovsbr0"}`; OVSDB schema `{"db_name":"Open_vSwitch"}`; Xray counter `{"name":"inbound>>>example>>>traffic>>>uplink"}` (the counter must actually exist). These are input-shape examples, not executed observations.

The UI can present OVS/OpenFlow dataplane and Xray proxy roles as distinct local planning choices. Neither a saved draft nor keyword classification changes packet enforcement, authorization, provider selection or proxy bootstrap state. Actual activation requires the specific admitted typed network methods and their verified readback separately.
