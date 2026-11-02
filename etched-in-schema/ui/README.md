# ui

> **The UI is in the 3tchedFS dev cycle.** Screens come from sealed plugin
> schemas, components from the catalog sealed in the `json_render` blob, and
> every action calls a sealed method (human: the door with their own
> credential; identity: its mount's call file) and shows the receipt. A new
> method's blob gives it UI without new code. See `../docs/DEV-CYCLE.md`.

The web UI is built **from scratch** for the hackathon (no code shared with the
3tched.com app). Only these references are kept, in `reference/`:

| File | What it is |
|---|---|
| `reference/element-map.json` | Design-free map of the 46 screens of the retired operation dashboard: headings, fields (type/label/placeholder), tabs, table columns and visible text. No styling, class names or layout. |
| `reference/menu.json` | The dashboard's menu: id, label, section, route. |
| `reference/docs/json-render-core.README.md` | `@json-render/core` 0.20.0 — specs, catalogs, validation (+ LICENSE). |
| `reference/docs/json-render-svelte.README.md` | `@json-render/svelte` 0.20.0 — the Svelte renderer that turns generated specs into UI (+ LICENSE). |

Stack (versions in use on the platform): Svelte 5.57, SvelteKit 3, Vite 8,
`@json-render/core` + `@json-render/svelte` 0.20. Svelte, SvelteKit and Vite
documentation: the skills listed in `AGENTS.md` (`svelte`, `svelte-code-writer`,
`sveltekit`, `json-render`, `core`, `devtools-svelte`) and the official docs at
svelte.dev and vite.dev.

Rules: Svelte + json-render only; every visible element is declared in the
catalog; the element map is a content reference, not a design to copy.

## The 3tched.com UI repository (pre-existing, not hackathon code)

`/srv/git/3tched.com` (github.com/repr0bated/3tched.com) is 3tched's own web
UI: SvelteKit + json-render, served by op-web, talking only to the door. It
predates the hackathon. The hackathon UI does **not** share code with it; the
only things taken from it are the references in `reference/` (the design-free
element map and menu of the retired operation dashboard, and the json-render
docs). Do not import from, build in, or publish from that repository for the
hackathon. Its own documentation and references stay there for the platform.
