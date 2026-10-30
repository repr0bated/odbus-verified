# Maintained compliance contracts and scaffold boundary

Read-only discovery on 2026-09-30. Maintained package: `/srv/git/repos-bulk/compliance-index`, Git HEAD `c651f896472f963a37754bcc886f9fe85eebf6cc`. No graph publication, embeddings, vector query, paid inference or network changes were performed. Current UI live integration is unavailable: no declared sealed bridge method exposing these maintained query contracts was found in the inspected surfaces.

## Authority and captured evidence

Consulted package `AGENTS.md`, discoverable `compliance:mappings`, both pinned inputs, compiler, store, CLI and vector-link implementation. Actual file hashes match `config/sources.lock.json`:

- `data/security-policy-templates-mappings.json`: `30919fdab75b80288ae222a1aabe712e02c9c7a9e6d684800837672a8022d27a`; 143 source records, 1,240 `requirements` assertions and 286 `controls` assertions.
- `data/nist-sp800-53-rev5.json`: `01f37cf90ea99d92242c936cbfbdebcc338eef1f71454e2acac36cc56e9bc062`; upstream revision `78650f02ad9321bb7b817846f8fbd4f2bcd620de`, catalog 5.2.0.

All four local captures pin graph generation `38cdcec0a21a316013fcc13efe8682347fceb628a2b5be362ab21197c08f7173`. `registry.json` reports 3,047 assertions, 557 historical assertions and 1,738 unresolved targets; its source list retains only two examples, with `source_count` preserving the original count. `policy-route.json` retains three real mappings and their endpoints from a truncated route, including `/implements/5/controls` and `/implements/1/requirements`. Absolute original corpus paths are redacted; point URIs, pointers and hashes remain. `ac2-route.json` retains the complete returned tag→control→document route. `ac2-scope.json` has no current verified vector groups; no vector query was issued.

Captures are review fixtures, not a replacement graph or proof of live enforcement. Every envelope has `captured_at`, `command`, `source_package`, `snapshot_kind`, and `result`; use `result` for the actual maintained response. The policy sample additionally records selection and original counts/truncation.

## Existing maintained read contracts

Run commands sequentially from the maintained package using the existing parent virtual environment. Every command takes exclusive nonblocking `.state/writer.lock`; opening the same embedded RocksDB outside that ownership convention is unsafe. A concurrent pair of our read commands demonstrated lock refusal; sequential retry succeeded.

```sh
../.venv/bin/python -m compliance_index mappings
../.venv/bin/python -m compliance_index mappings --since GENERATION
../.venv/bin/python -m compliance_index route TAG --depth 4 --limit 200
../.venv/bin/python -m compliance_index scope --tag TAG
```

These are package interfaces, **not browser endpoints or declared plugin methods**.

`mappings` returns `{generation, changed, schema_version, retrieval_policy, assertions, historical_assertions, unresolved_targets, kinds, sources, contract}`. `changed` compares the supplied `--since`; omission means true, not proof of a new publication. Source entries are `{source, sha256, assertions}`.

`route` returns `{generation, tag, retrieval_policy?, nodes, paths, truncated}`. An unknown tag returns empty nodes/paths with `truncated:false` and omits `retrieval_policy`. Bounds: depth 0..8; limit 1..10,000. Historical/unresolved edge statuses are excluded by default; `--include-historical` explicitly broadens selection. Source-asserted mappings can still have `unresolved_reference` endpoint nodes.

Node: `{id, namespace, version, kind, local_id, title, status, data}`. Path: `{src, dst, kind, status, evidence}`. Evidence includes mandatory `{source, sha256}` and source-dependent pointer, revision, original path or lifecycle status text. Canonical IDs encode namespace, explicit version, kind and local ID. `paths` is the traversal's discovery tree, **not all graph edges**; even a nontruncated route is bounded by depth.

`scope` returns `{generation, collection, allowed_groups, qdrant_filter}`. Nonempty groups require:

```json
{"must":[{"key":"group_id","match":{"any":["EXACT_ALLOWED_GROUP_ID"]}}]}
```

Empty `allowed_groups` means `qdrant_filter:null` and zero eligible results: do not issue an unfiltered query. Missing active context publication errors instead of allowing retrieval. Compare captured registry/route/scope generations; reject mixed generations rather than pretending multiple CLI calls form one snapshot. The current tag scope internally routes at depth 4 and limit 10,000; it does not expose route truncation, so future live integration must preserve an explicit completeness limitation.

Store schemas already exist in `compliance_index/store.py:13`: `cp_nodes`, `cp_edges`, `cp_documents`, `cp_generations`, `cp_active`. Vector links are `cp_context_groups{generation,group_id => context_hash,members}`, `cp_context_sets{generation => manifest_hash,groups}` and `cp_embeddings{group_id,collection => record}`. Scope accepts only verified records whose context hash and document membership match the active generation (`embedding.py:244`). Qdrant payloads carry group/context hashes, document member IDs/source hashes, settings, part/chunk identity, text/text hash, repos and paths (`embedding.py:192`). Graph-only changes can reuse these vectors; semantic similarity must not create mapping edges.

## Existing schema and policy surfaces

`oscal_subid_registry` reads exact manifest-pinned sealed schema membership. Review annotations cannot create membership or redefine method contracts (`oscal_dispatch.rs:1`). Existing JSON inputs/outputs:

- `resolve`: `{plugin_id,schema_method_key}` → `{catalog_hash,binding,subid}`; absent matches return null fields.
- `lookup`: `{subid}` → `{catalog_hash,entry}`; entry includes sealed bindings and optional `{revision,control_source,control_refs,statement_refs}` annotation.
- `export`: `{plugin_id?,category?,limit?,cursor?,catalog_hash?}` → `{catalog_hash,entries,next_cursor}`; limit defaults 50, maximum 100; cursor requires matching catalog hash.
- `register`: `{subid,expected_revision,control_source?,control_refs?,statement_refs?}` → `{subid,annotation_revision,catalog_hash,binding_count}`. This is a review mutation requiring `oscal.invoke`; read methods require `oscal.read`. Do not seed annotations or capabilities during scaffold discovery.

Bindings retain `{plugin_id,schema_hash,schema_version,kind,schema_key,transport_name}`. A schema map key may differ from transport name. The maintained compiler's `add_registry` (`compiler.py:358`) accepts explicit control/statement references only when their exact canonical IDs already exist; absent references remain issues. Its configured input is `/srv/git/odbus/deploy/config/subid-registry.json`, distinct from the inspected checkout and runtime annotation document. They are not automatically synchronized.

`tched_router.ResolveRoute` is `{hint:string}` → `{route:ModelRoute}` and resolves an existing hint or model exactly (`tched_router.rs:397,715,2348`). `gemma_brain.route` delegates to that method and rechecks outer and nested grants (`gemma_brain_dispatch.rs:46`). This does not infer compliance or network policy from a tag. The old cognitive `search_compliance` queries `repos_specs_docs_voyage_4_lite` with a default filter (`semantic_search.rs:41`); it is not the maintained compliance scope contract.

OpenFlow `add_flow`/`delete_flow`/`modify_flow` use exact flow members and `openflow.write`; a mutation receipt records before/after, verification and observation time (`openflow.rs:1548,1750`). OVSDB schema exposes read/monitor/transact methods with owner-scoped projection. Xray native dispatch currently supports stats/add_user/remove_user/add_inbound; restart/trace methods explicitly return unavailable (`mutation_engine.rs:6176`). These declarations do not prove deployment, grants or policy satisfaction. No routes or rules were changed here.

## Minimal scaffold and regressions

1. Render small typed captures with capture time, graph generation, explicit mapping direction/status/evidence, endpoint resolution, historical selection and truncation. Display live integration as unavailable. Keep identities/grants, compliance source authority, desired policy and observed enforcement distinct.
2. Validate captures at the UI boundary: consistent generation; unique canonical nodes; every displayed path endpoint present; source/hash evidence present; explicit empty scope; exact nonempty group filter; bounded payload/count/string sizes. Treat recent-event semantic retrieval as independently scoped event context, never compliance mapping authority or identity evidence.
3. Only after a real sealed method exists, integrate through the existing ChatAdapter and canonical capability admission. Reuse the maintained package/store; do not create a parallel graph, direct browser CLI/HTTP route, inferred tag→control→flow edge, or new embedding job. Desired network-policy proposals require separate explicit source evidence and verified enforcement observation before a completion claim.

Meaningful fixtures: both mapping fields survive; unresolved endpoint stays unresolved; historical ASVS stays historical; reversed traversal stays labeled `reverse:maps_to`; mixed generations fail; null/empty scope issues no query; malformed/widened/mismatched group filters fail; truncated/subset captures cannot claim completeness; absent live method stays unavailable. Source/test success alone cannot establish deployed enforcement.
