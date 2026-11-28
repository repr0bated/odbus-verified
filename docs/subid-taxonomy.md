# Subid Taxonomy — GhostBridge Operational Routing

This document describes the subid taxonomy (pattern, categories, rules) and
how routing tags use it. The registry of actual subids is the sealed catalog
(see "Registry location"); the vocabulary is `crates/op-oscal-subid/src/lib.rs`.
Originally derived from `oscal-subids-report.md` for the GhostBridge host
consolidation (subids -> tags -> OpenFlow flow rules + xray routing rules).

## Dual-identifier model

- `uuid` is the immutable machine identity (OSCAL-style).
- `subid` is the stable human-readable operational taxonomy key.
- Compliance mappings (`control_refs`, `statement_refs`, `control_source`) live in
  metadata arrays, never inside the `subid` string.

## Canonical subid pattern

```
<category>.<component-type>.<subject>.<verb>[.<facet>][@vN]
```

- `category` — one of `src`, `prj`, `sch`, `mut`, `obs`, `evt`, `exp`.
- `component-type` — an OSCAL noun, or one local noun. OSCAL: `this-system`, `system`, `interconnection`, `software`, `hardware`, `service`, `policy`, `physical`, `process-procedure`, `plan`, `guidance`, `standard`, `validation`. Local: `network`, `agent`, `container`, `data`, `privacy`, `security`, `storage`. Aliases `ui`, `llm`, and `agents` stand for `software`, `service`, and `software`.
- `subject` — lowercase hyphenated noun (e.g. `xray`, `cognitive`).
- `verb` — lowercase hyphenated action (e.g. `serve`, `route`, `monitor`).
- `facet` — optional qualifier for routing variants.
- `@vN` — optional version, default `@v1`.

## Routing tags

A routing tag is a short stable label used by both OpenFlow and xray. Each tag
is declared in the subid registry and maps to one or more subids.

Tags are not a separate registry: a tag is valid only when it names a subid
that the sealed catalog already carries (see "Registry location").

> **Retired (2026-11-22).** The table that stood here (tags `xray-tls`,
> `xray-reality`, `grpc-bridge` -> "zeroclaw gateway on 127.0.0.1:8090"
> `exp.service.zeroclaw.serve@v1`, `cognitive`, `qdrant`, `netmaker-*`,
> `nextdns`) described the pre-consolidation GhostBridge/zeroclaw host. It is
> not a registry and was never sealed: 127.0.0.1:8090 is no longer a zeroclaw
> gateway (the door is the op-grpc-bridge on 10.0.0.2:8090), and runtime
> membership of every subid comes only from the sealed catalog. Read subids
> from the catalog (`oscal_subid_registry` discovery), not from this page.

## Subid → tag → xray rule

Gemma reads each subid, extracts its tag, and emits an xray routing entry:

```json
{
  "tag": "qdrant",
  "subdomains": ["qdrant.ghostbridge.tech"],
  "outbound": "to-qdrant"
}
```

The shuttle merges these entries into `routing.rules` before the catch-all rule.

## Subid → tag → OpenFlow rule

Gemma also emits OpenFlow rules for the `ovsbr0` privacy bridge. The rules are
passed to the OpenFlow controller as structured match/action pairs. The exact
match syntax follows the controller's existing privacy-flow convention.

Example:

```json
{
  "tag": "netmaker-api",
  "match_fields": { "dl_type": "0x0800", "nw_proto": "6", "tp_dst": "28081" },
  "actions": ["output:netmaker-api"],
  "priority": 100
}
```

## Registry location

The canonical subid registry is the **sealed plugin catalog**
(`/var/lib/opdbus/plugin-blobs`, manifest `.manifest.json`; versioned by the
blob `FORMAT_VERSION`, `catalog_hash`/`previous_catalog_hash`/`generation` and
each plugin's `schema_hash`). Every sealed method, field and event carries its
subid there (`x-oscal-subid`, `MethodDecl.subid`).

- Runtime membership comes only from the sealed catalog; the
  `oscal_subid_registry` plugin discovers and annotates existing subids but
  **cannot mint one** (`crates/op-plugins/src/state_plugins/oscal_subid_registry.rs:1-7`).
- The taxonomy vocabulary (categories, component types, validation) has one
  source, `crates/op-oscal-subid/src/lib.rs:1-21`, re-exported by
  `op-plugins` `common::oscal`, `op_blob::subid` and `op-plugin-lint`.
- A new subid is a sealed schema change: it enters through a plugin schema and
  a catalog reseal, never through a side file.
- Retired: `/etc/ghostbridge/subid-registry.json` and the "Gemma watches this
  file (or receives it via D-Bus /dev/shm)" path. No component reads it; it is
  not a registry.
- Grouping (owner, 2026-11-22): chats and ledger entries are grouped by the
  existing subids they already carry (sled ledger `subid` field, episode
  fields' `x-oscal-subid`); grouping never invents a subid.

## Rules

- `subid` is immutable per subject; material changes require a new `@vN`.
- `mut.*` records must carry `actor_id` and `capability_id`.
- `evt.*` records must carry `event_id` or `event_hash`.
- All records must carry a paired `uuid`.
