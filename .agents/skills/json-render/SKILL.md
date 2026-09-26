---
name: json-render
description: >-
  Expert skill for building json-render.dev catalog components, page specs, and
  generative UI across three repos: /srv/git/json-render (library source),
  /srv/git/odbus (Rust backend + op-web Axum server), and
  /srv/git/3tched.com (separate chatbot-rendered UI repository).
  Covers the full stack: defineCatalog → Zod schema → React component →
  pageSpec → NAV_MANIFEST → factory plugin wiring → sealed blob pipeline →
  gallery/catalog promotion gate. Also covers the D-Bus plugin schema for
  json-render and schema-renderer in op-plugins.
---

# json-render Skill

## Local deployment policy — runit, not systemd

The user-designated UI repository is `/srv/git/3tched.com`, separate
from `/srv/git/odbus`. Inspect its actual catalog, registry and build/sync inputs;
the reference layouts below do not establish its implemented components.
`/srv/git/operation-dashboard-ui-07` and `crates/op-web/ui/` are retired UI paths.

The host uses runit. Backend runtime actions go through the authenticated D-Bus
plugin surface. Only for authorized operator service work use `sudo -n sv`
(for example `sudo -n sv status op-web`); never use `systemctl`, its shim, s6 or
service6. Edit versioned runit inputs, not `/run/runit/service`. Backend deployment
is btrfs send/receive of an explicitly prepared filesystem snapshot; there is no
deployment script. Never hand-copy binaries. Static UI synchronization does not itself
require an op-web restart. Do not create a parallel frontend backend or expose
broker credentials to the browser.

EMQX is the retained vendor-binary-backed `emqx` D-Bus plugin, independent of
retired Netmaker. Broker events may drive authorized live state updates while
the chatbot generates schema-pinned layout patches. A generated mode, component
or action never grants permissions: enforce verified session and project ACLs
server-side, including on streamed updates and reconnects.

## Overview — Three Repos, One System

```
/srv/git/json-render                   ← upstream library (source of truth for API)
/srv/git/odbus                         ← Rust backend
  crates/op-plugins/src/state_plugins/json_render.rs    D-Bus plugin schema
  crates/op-plugins/src/state_plugins/schema_renderer.rs
  crates/op-web/src/handlers/ui_model.rs                Axum gallery/catalog routes
/srv/git/3tched.com        ← active UI target; paths below are layout examples
  src/json-render/
    catalog/catalog.ts                 defineCatalog (app catalog contract)
    catalog/registry.tsx               defineRegistry (component implementations)
    catalog/components/                React component render functions
      shell.tsx   dashboard.tsx   network.tsx   primitives.tsx
    navigation/manifest.ts             NAV_MANIFEST (sidebar)
    navigation/capabilities.ts
    navigation/icons.ts
    pages/                             pageSpec files per route
    runtime/                           JsonRenderProvider, SpecPage, PageSpecOutlet
    shell/shellSpec.ts                 shell composition spec
    spec-builders.ts                   dataToSpec / streamToSpec helpers
```

---

## 1. Spec Wire Format

```typescript
interface Spec {
  root: string;                         // key of the root element
  elements: Record<string, UIElement>;  // FLAT map — no nesting
}

interface UIElement {
  type: string;                         // catalog component name
  props: Record<string, unknown>;       // validated by catalog Zod schema
  children?: string[];                  // keys into elements map
  slots?: Record<string, string[]>;     // named slot overrides
  visible?: VisibilityCondition;        // { $state: "path" } or literal bool
  on?: Record<string, ActionBinding>;   // event → action
  repeat?: { statePath: string; key?: string };
  watch?: Record<string, ActionBinding>;
}
```

**Critical:** `elements` is a flat key-value map. Children are string keys, not
nested objects. `dataToSpec`/`streamToSpec` in `spec-builders.ts` handle this.

Dynamic prop state references:
```typescript
{ collapsed: { $state: "/shell/navCollapsed" } }  // resolved at render time
{ collapsed: false }                               // literal value
```

---

## 2. Catalog Contract (reference layout)

**File:** `src/json-render/catalog/catalog.ts`

```typescript
import { defineCatalog } from "@json-render/core";
import { schema } from "@json-render/react/schema";
import { z } from "zod";

export const appCatalog = defineCatalog(schema, {
  components: {
    myComponent: {
      props: z.object({
        title: z.string(),
        subtitle: z.string().nullable(),  // nullable = omittable in specs
        variant: z.enum(["default","ok","warn","danger"]).nullable(),
      }),
      slots: ["default"],
      description: "One-line LLM prompt description.",
    },
  },
  actions: {
    navigate: {
      params: z.object({ route: z.string() }),
      description: "Navigate the router to a route path.",
    },
  },
});
```

**Rules:**
- Every component needs a `description` — feeds `catalog.prompt()` for generative runs.
- Use `z.nullable()` not `z.optional()` — spec JSON uses `null`, not missing keys.
- Never invent props outside the Zod schema — specs are validated before render.
- camelCase names in TypeScript catalog; lowercase-hyphen in egui DSL.

---

## 3. React Component Implementation

**Pattern:** Use `El<"componentName">` from `catalog/components/types.ts`.

```typescript
// src/json-render/catalog/components/myFeature.tsx
import type { El } from "./types";

export const MyComponentEl: El<"myComponent"> = ({ props, children, emit }) => (
  <div>
    <h2>{props.title}</h2>
    {props.subtitle && <p>{props.subtitle}</p>}
    <button onClick={() => emit("press")}>Go</button>
    {children}
  </div>
);
```

Register in `src/json-render/catalog/registry.tsx`:
```typescript
myComponent: myFeature.MyComponentEl,
```

Component anatomy:
- `props` — typed, already validated against Zod schema
- `children` — React nodes from `slots["default"]` or `children[]`
- `emit(eventName)` — fires action bound to `on.eventName` in the spec
- Read live data internally (hooks/stores) — specs stay pure composition

---

## 4. Adding a Page (reference layout)

### Step 1 — NAV_MANIFEST entry
**File:** `src/json-render/navigation/manifest.ts`

```typescript
{
  id: "factory",
  label: "Factory",
  route: "/factory",
  icon: "Factory",        // any Lucide icon name
  section: "Agent",       // must be in SECTION_ORDER
  order: 45,
  wip: true,
  wipReason: "Sessions tab not yet wired to factory.list_sessions.",
}
```

Sections (SECTION_ORDER): `"Chat"`, `"UI Model"`, `"Control"`, `"Agent"`,
`"Infrastructure"`, `"ZeroClaw"`, `"Settings"`.

### Step 2 — Page spec
**File:** `src/json-render/pages/factory.pagespec.ts`

```typescript
import type { Spec } from "@json-render/core";

export const factorySpec: Spec = {
  root: "root",
  elements: {
    root: {
      type: "container",
      props: {},
      children: ["header", "row1"],
    },
    header: {
      type: "pageHeader",
      props: {
        title: "Factory",
        subtitle: "Session management · model routing · computer inventory",
      },
    },
    row1: {
      type: "grid",
      props: { cols: 2, gap: 4, className: "grid-cols-1 lg:grid-cols-2" },
      children: ["sessions", "computers"],
    },
    sessions: { type: "factorySessionsPanel", props: {} },
    computers: { type: "factoryComputersPanel", props: {} },
  },
};
```

### Step 3 — Register in pages/index.ts
```typescript
export const PAGE_SPECS: Record<string, Spec> = {
  "/": overviewSpec,
  "/network": networkSpec,
  "/factory": factorySpec,
};
```

If still using a legacy React page, skip step 3 and register in `App.tsx` router instead.

---

## 5. Factory Plugin — gRPC Methods

Generated files in `src/grpc/gen/plugin_methods/`:

| Subid | Input | Output |
|---|---|---|
| `factory.list_sessions` | `{}` | `sessions: Value[]` |
| `factory.get_session` | `{}` | `session?: Value` |
| `factory.list_models` | `{}` | `models: Value[]` |
| `factory.list_computers` | `{}` | `computers: Value[]` |
| `factory.discover_byom` | `{}` | `{}` |
| `factory.set_autonomy` | `{}` | `{}` |

Call pattern (see AntigravityService in `src/grpc/client.ts`):
```typescript
import { getPluginMethodsClient } from "@/grpc/client";

const client = getPluginMethodsClient();
const { response } = await client.callSubid({
  subid: "factory.list_sessions",
  input: {},
});
// response.output is a protobuf Value — unwrap with valueToJson()
```

---

## 6. Factory / odbus Architecture Rules

From `FACTORY-HANDOFF.md` and `CLAUDE.md` in `/srv/git/odbus`:

- Plugin IS schema. Schema lives in the plugin `.rs` file, not in `plugin_schema_defs.rs`.
- D-Bus object existence = system existence. Plugin objects at `/org/opdbus/v1/plugins/<name>`.
- `uuid` = machine identity. Never replace with `subid`.
- `subid` = OSCAL operational taxonomy key. Must be an OSCAL prop value, not remarks.
- `mut.*` records require `actor_id` and `capability_id`.
- `evt.*` records require `event_id` or `event_hash`.
- No `bool success` fields — use gRPC status codes.
- No JSON string payloads — use `google.protobuf.Struct`.
- Compliance mappings belong in metadata arrays, not inside the `subid` string.

tched_router context:

- `tched_router` is the OP-DBUS scaffolding for the autonomous `zeroclaw` binary.
- `op-grpc-bridge`, not that binary, owns the `:8090` TLS fabric. Cognitive MCP
  is in-process; do not start a standalone cognitive MCP runit service.
- The factory examples above are historical; verify current methods, input
  types, capabilities and bus names from the sealed plugin surface before use.

---

## 7. Gallery → Catalog Promotion Pipeline

```
tched_router reads plugin PluginSchema blob
  ↓ (sealed blob at /dev/shm/opdbus/plugin-blobs/<plugin>.<hash>.blob)
gemma_brain / op-gallery-gen generates Spec
  ↓
/api/gallery-gen/* (op-web Axum: ui_model.rs) → /dev/shm/ui-specs.json
  ↓
Gallery page (/gallery) — human reviews
  ↓ human promotes
Catalog page (/catalog) — approved specs only
  ↓
<SpecPage> renders in React dashboard
```

Schema hash pinning (critical — see `.specs/generative-ui-catalog/vision.md`):
- Gallery entries MUST carry the `schema_hash` they were drawn against.
- Catalog entries are PINNED to that hash.
- A reseal identifies which approved elements are stale.
- Never promote without verifying hash matches the current sealed blob.
- Plugin presence comes from the current sealed manifest; counts change with reseals.

---

## 8. D-Bus json_render Plugin

**File:** `crates/op-plugins/src/state_plugins/json_render.rs`

Publishes the full json-render surface at `/org/opdbus/v1/plugins/json_render`:
- `packages` — `@json-render/*` package inventory
- `components` — catalog component declarations
- `actions` — action type declarations
- `validation_checks` — built-in validation rules
- `directives` — `$format`, `$math`, `$concat`, `$count`, etc.
- `methods` — D-Bus method surface
- `inspector_fields` — uncapped fields from authoritative packages
- `source` — provenance (docs URL, repo URL, commit hash)

This blob is what generative runs read to know the available surface area.

---

## 9. spec-builders Utility

**File:** `src/json-render/spec-builders.ts`

```typescript
import { dataToSpec, streamToSpec } from "@/json-render";

// Any JSON → renderable spec (uses card/kv/container)
const spec = dataToSpec(myJsonObject, "My Label");

// List of streaming messages → spec
const spec = streamToSpec(messageArray, "Stream Title");
```

Use when rendering raw gRPC response data without a custom component.

---

## 10. Checklist: Adding a New Feature Page

- [ ] Add `NavItem` to `NAV_MANIFEST` in `navigation/manifest.ts`
- [ ] Create `src/json-render/pages/<name>.pagespec.ts` with a typed `Spec`
- [ ] Register in `pages/index.ts` PAGE_SPECS map (or use legacy React route)
- [ ] If new components needed:
  - [ ] Add to `catalog/catalog.ts` (Zod schema + description)
  - [ ] Implement in `catalog/components/<file>.tsx` using `El<"name">` type
  - [ ] Register in `catalog/registry.tsx`
- [ ] For factory methods: `client.callSubid({ subid: "factory.<method>", input: {} })`
- [ ] For schema hash integrity: verify blob hash before promoting gallery → catalog
- [ ] Mark `wip: true` on nav entry until real backend is wired

---

## 11. Key File Locations

| What | Path |
|---|---|
| Catalog contract (reference layout; inspect target first) | `/srv/git/3tched.com/src/json-render/catalog/catalog.ts` |
| Component registry | `src/json-render/catalog/registry.tsx` |
| Shell components | `src/json-render/catalog/components/shell.tsx` |
| Dashboard widgets | `src/json-render/catalog/components/dashboard.tsx` |
| Network components | `src/json-render/catalog/components/network.tsx` |
| Primitives | `src/json-render/catalog/components/primitives.tsx` |
| Nav manifest | `src/json-render/navigation/manifest.ts` |
| Page specs index | `src/json-render/pages/index.ts` |
| Shell spec | `src/json-render/shell/shellSpec.ts` |
| Spec builders | `src/json-render/spec-builders.ts` |
| Factory gRPC types | `src/grpc/gen/plugin_methods/factory__*.ts` |
| gRPC client | `src/grpc/client.ts` |
| json_render plugin | `/srv/git/odbus/crates/op-plugins/src/state_plugins/json_render.rs` |
| schema_renderer plugin | `/srv/git/odbus/crates/op-plugins/src/state_plugins/schema_renderer.rs` |
| tched_router plugin | `/srv/git/odbus/crates/op-plugins/src/state_plugins/tched_router.rs` |
| Gallery/catalog routes | `/srv/git/odbus/crates/op-web/src/handlers/ui_model.rs` |
| json-render library | `/srv/git/json-render/packages/core/src/` |
| json-render.dev docs | https://json-render.dev/docs |
