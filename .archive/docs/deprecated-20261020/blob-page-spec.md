# Blob / Plugin Page Spec

Status: draft (2026-08-11) · supersedes nothing · consumes `docs/subid-projection-catalog.md`

## Goal

One page per sealed plugin blob where the operator sees the blob **unpacked** —
sections, schema, live state, methods, subids — with **multiple display modes**
over the same artifact. The page is the review surface for the subid →
projection program: gaps, joins, and roles must be *visible*, not greppable.

## Placement

Two front ends, same data source (`/dev/shm/opdbus/plugin-blobs/`, read via
`op-blob::catalog` — never re-hashed, never re-derived by consumers):

1. **Operator console** (`crates/op-web/ui`, egui) — new `Route::Plugin(String)`
   alongside the existing schema-driven routes (Inspector, State, Grpc).
   Rendering reuses `schema.rs` (schema+value tree, virtualized, no
   hand-written forms).
2. **op-web HTTP** (`crates/op-web/src/handlers/`) — JSON endpoints so the
   chatbot/gallery path and external tools get the same unpacked view.
   `ui_model.rs::resolve_plugin_by_prefix` already does manifest lookup;
   extend rather than duplicate.

## Data flow

```
GET /api/plugins                     -> read_manifest_plugin_ids_shm()      (id list + schema hashes)
GET /api/plugins/{id}                -> read_plugin_schema_shm(id)          (schema section)
GET /api/plugins/{id}/state          -> live state section of the blob      (present-state values)
GET /api/plugins/{id}/projection     -> catalog projection view             (roles, joins, gaps)
```

All four are read-only SHM reads. The projection view is computed by the single
projection function (see "Projected mode" below) — the page never computes its
own mapping.

## Display modes

Same blob, three lenses, switchable in-page:

### 1. Raw
Sealed sections as canonical JSON: schema / state / metadata / audit.
For diffing and audit. No interpretation.

### 2. Structured
The existing `schema.rs` tree: fields with types, descriptions, `required` /
`read_only` semantics, constraints; live values folded in; methods with their
arg schemas. This mode already works today for any schema — the page is mostly
routing + section assembly.

### 3. Projected
The subid-mapping lens. Per field: subid, category, computed role
(display-value, text-control, collection-view, …). Per method: subid and its
subject-join to a field. Rows with no subid render as explicit **GAP** entries
(this is what makes the 133-field / 570-method work lists visible in the UI
instead of in a script). Category-mismatched rows (e.g. `obs.*` field that is
read-write and has a setter, like `snowball.retention`) render as conflicts.

Projected mode consumes the seal-time role facet (planned: role computed once
by the sealer, sealed into the blob). Until that lands, the mode reads the
projection from the same single function the sealer will use — one
implementation, imported both places, no table drift.

## Dogfood option

The plugin page can itself be emitted as a json-render DSL spec and admitted
to the gallery like any generated element. If admitted, "the gallery is the
contract" then covers the system's own introspection pages, and the console
renders the page through `catalog/interpret.rs` instead of a bespoke route.
Decide after mode 3 lands; the page works either way.

## What this page is not

- Not a writer. No mutations, no sealing, no promotion actions. Promotion is a
  separate deliberate flow (`op-blob` sealer only).
- Not a second source of truth. If the page and the tree disagree, the blob
  wins; the page only reads.

## Milestones

1. HTTP endpoints (4 routes above) + console route, modes Raw/Structured.
2. Projected mode wired to the projection function; GAP/conflict rendering.
3. Seal-time role facet in blobs; page switches to reading it.
4. (Optional) page-as-gallery-spec dogfood.

## Review value

This page is how the big tagging batches get eyeballed: after tagging
zeroclaw's 125 methods, open `/plugins/zeroclaw` in Projected mode and the
joins, gaps, and `options`-bag identity-only methods are all on one screen.
