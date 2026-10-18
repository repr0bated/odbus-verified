# Versioned UI documentation

Read these copies through the runtime's admitted documentation/file surface.
Paths below are relative to this file. `sources.json` records repository,
tag, resolved commit, original path and SHA-256 for every imported file.
The copies retain upstream licenses. They are a focused offline reference,
not a second application catalog or a substitute for live grants/state.

## Read before using an API

| Technology | Version in the UI lockfile at capture | First reference |
|---|---|---|
| json-render core and Svelte renderer | 0.20.0 | [Svelte renderer](upstream/json-render/svelte-guide.md), then [component types](upstream/json-render/catalog-types.ts) and [registry implementation](upstream/json-render/renderer.ts) |
| Svelte | 5.57.1 | [Runes](upstream/svelte/runes.md), [props](upstream/svelte/props.md), [markup and events](upstream/svelte/markup.md) |
| SvelteKit | 3.0.0; adapter-static 4.0.0 | [Routing and layouts](upstream/kit/routing.md), [static adapter](upstream/kit/adapter-static.md) |
| Vite | 8.2.2 | [Build and base paths](upstream/vite/build.md), [environment variables](upstream/vite/env-and-mode.md) |

These versions were read from `/srv/git/3tched.com/package-lock.json` on
2026-10-11. They describe this capture, not every future installation.

## Read for the task at hand

- Reactive Svelte components: [state](upstream/svelte/state.md),
  [derived values](upstream/svelte/derived.md),
  [rendering child snippets](upstream/svelte/render.md).
- Session-owned state and cleanup: [context](upstream/svelte/context.md),
  [lifecycle](upstream/svelte/lifecycle.md). In particular, clean up listeners,
  pending timers and asynchronous work when the owner is destroyed.
- Navigation and static hosting: [single-page apps](upstream/kit/single-page-apps.md)
  and [shallow routing](upstream/kit/shallow-routing.md). A shallow URL change
  is not proof that a deep link reload works on the deployed static host.
- Asset URLs: [Vite assets](upstream/vite/assets.md), alongside the active
  SvelteKit base/adapter configuration.
- The official [Svelte code writer](upstream/svelte-ai-tools/svelte-code-writer.md)
  describes documentation lookup and code analysis. Command examples are
  tool documentation, not permission to run a command inside a chat turn.
  Prefer a file path for code analysis. Single-quote shell arguments containing
  `$state` or other runes; do not add a literal backslash inside single quotes.

## Differences that matter here

- This project uses Svelte components. json-render's general README and many
  third-party examples start with React; use the Svelte-specific source.
  The Svelte registry accepts component constructors with `BaseComponentProps`.
- The imported json-render guide is upstream documentation. Where it differs
  from the same revision's implementation, consult the bundled types/source
  and [renderer schema](upstream/json-render/schema.ts).
  Its built-in-action list does not establish this app's allowed actions.
- Upstream examples may request sample data or use HTML, new API routes and
  state bindings. The actual application catalog, validator and permission
  boundary govern whether any of those are allowed. Preserve unknown live
  values; do not turn examples into operational telemetry.
- Svelte runes have specific compiler-supported positions, including reactive
  class fields. A blanket claim that all runes are top-level-only is too broad.
  Use the pinned language documentation for the construct being edited.
- SvelteKit 3 config is supplied to `sveltekit()` in Vite. This checkout uses
  `svelte-vite-options.ts`; do not restore a SvelteKit 2 config layout by habit.
  Its shallow routing API also differs from older examples.
- The installed Anthony Fu `vite` skill was authored against Vite 8.3.1;
  this UI locks 8.2.2. Consult the pinned 8.2.2 docs for exact support. Installing
  a skill must not silently upgrade the application's dependencies.
- `VITE_` variables are browser-visible build input. They cannot hold provider
  secrets. Use the current adapter's public routing configuration only.

## Supplemental skill sources

The owner supplied [spences10/svelte-skills-kit](https://github.com/spences10/svelte-skills-kit).
At commit `c4cf7da930117a506f968a73e809f9f23d2952ec` its README identifies
[spences10/skills](https://github.com/spences10/skills) as the canonical
agent-independent source. Its separation of runes, structure and deployment
is useful for skill discovery. Reviewed guidance includes older Vite 7
instructions and the broad rune-position claim addressed above. This bundle
does not install its Claude marketplace or copy those instructions as API
authority.

The owner also requested installation of
[sveltejs/ai-tools](https://github.com/sveltejs/ai-tools)'s `svelte-code-writer`
and [antfu/skills](https://github.com/antfu/skills)'s `vite`. Preserve their
installed source and licenses; use this project's runtime contract when a
general-purpose skill assumes tools or backends unavailable to this caller.

All ten owner-retained skills are standalone runtime entries in the default
bundle. Their unchanged `SKILL.md` contents are also included in the sealed
shared guide for every chat mode. Read their supporting files as needed through
granted tools. Each directory's `SOURCES.json` records the exact upstream
commit, source paths and file hashes. Resolve the skill names below through
the runtime skill catalog; each skill owns its supporting reference paths.

| Reference | Applicability to this application |
|---|---|
| `svelte-code-writer` | Svelte documentation and analysis; tools require the caller's existing grants |
| `vite` | Build configuration; confirm APIs against the pinned application version |
| `core` | Schema/catalog concepts; confirm APIs against the pinned application version |
| `web-design-guidelines` | Accessibility and interface review; fetching the referenced live rules requires an available tool |
| `building-components` | Component design and accessibility adapted to Svelte |
| `ai-sdk` | SDK documentation when relevant; keep the existing authenticated door and provider handling |
| `remotion` | Video composition guidance within the requested task and application stack |
| `remotion-best-practices` | Video and animation guidance within the requested task |
| `ai-elements` | Chat interface patterns adapted to the declared Svelte catalog |
| `streamdown` | Streaming content guidance; keep the application's registered Svelte renderers |

The separate `react` and `react-native` skills and their captured references
were removed at the owner's request. All other requested skills were retained.
Installing or reading a skill does not authorize a new framework, dependency,
provider gateway, credential path or backend. Some current upstream guides
describe unreleased APIs; the application's pinned documentation takes priority.

## Refreshing this capture

Read the current UI lockfile, resolve matching upstream release tags to full
commits, and review changed docs/source before updating the imported files.
Regenerate their provenance hashes and retain licenses. Check catalog,
registry, active profile and runtime navigation independently; framework docs
cannot supply them. Keep the skill entrypoint small and load relevant
references on demand. Staging files, sealing a plugin description and loading
a skill into a chat mode are distinct steps, each requiring its own evidence.
