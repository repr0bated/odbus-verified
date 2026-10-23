# json-render catalog — generated, not written here

This file used to list component types and props by hand. It no longer does,
because a hand-written list is a second declaration of the vocabulary: when it
drifts from the app's catalog, the model is taught components the renderer does
not have and the admission gate rejects everything generated. That is exactly
what had happened — this doc still described `status_pill`, `kv_pair` and
`metric_card`, none of which exist in the catalog.

The vocabulary now has one source: `src/json-render/catalog/` in the dashboard UI
repo. Two artifacts are exported from it into this repo:

| File | Contents | Consumed by |
|---|---|---|
| `schemas/json-render/catalog.schema.json` | per-component prop schemas, slots, actions, visibility schema | `op_gallery_gen::CatalogGuard` (the admission gate) |
| `schemas/json-render/catalog.prompt.md` | the catalog's own system prompt: output contract, directives, state model, all components | `GenerationContext::build_system_message` (what the model is told) |
| `schemas/json-render/catalog.manifest.json` | sha256 of both, plus counts and names | digest check at load; a stale export refuses to load |

Regenerate after any catalog change:

```sh
cd /srv/git/operation-dashboard-ui-07
npx vite-node scripts/export-catalog-schema.mts
```

The gate and the prompt come from the same export, so they cannot disagree about
what exists — `catalog_guard::tests::every_component_the_gate_enforces_appears_in_the_prompt`
fails if they ever do.

To read the current vocabulary, read the artifact:

```sh
jq -r '.componentNames[]' schemas/json-render/catalog.manifest.json
jq '.components.statCard' schemas/json-render/catalog.schema.json
```
