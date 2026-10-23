---
name: ui-authoring
description: Compose catalog-based interfaces or edit the 3tched SvelteKit, Svelte, Vite and json-render UI. Read runtime grants, versioned documentation, the implemented catalog and navigation before working. Applies in every chat mode.
author: 3tched
version: 0.2.0
category: ui
---

# UI authoring

Use Svelte, SvelteKit, Vite and the json-render Svelte renderer. A request to
draw an interface can be fulfilled by composing existing registered elements;
a request to change component implementation requires project file access.
Keep those tasks distinct when checking grants. This skill does not select a
chat mode, grant permissions, install tools, or authorize deployment.

This stack and the preflight below apply to every user and every chat mode.
Use only Svelte/SvelteKit, Vite and json-render's Svelte renderer for application
UI code. All retained skill entrypoints are loaded as public guidance across
modes. Apply their relevant advice within this application's stack.
Do not add React/JSX/Next.js components,
another renderer, or framework dependencies; use only guidance compatible with
this stack. Each mode and caller retains its existing capabilities.

The shared guide includes `svelte-code-writer`, `vite`, `core`, `remotion`,
`remotion-best-practices`, `web-design-guidelines`, `building-components`,
`ai-sdk`, `ai-elements` and `streamdown`. Their supporting references stay in
the runtime skill bundle and are read as needed through granted tools.
Loading these instructions does not install the libraries described in them.
Project versions, this application contract and the caller's grants govern
their use; upstream examples do not select a new framework or provider gateway.

## Before anything else

1. **List your granted capabilities.** Read the current caller's effective
   grants through its supported runtime surface. A declared capability, a
   menu entry, or a method's required capability is not a grant. For project
   edits, verify read and write access to this project; if either is missing,
   stop and name the missing grant from the declaration or denial. If the
   grant cannot be discovered, report that uncertainty instead of inventing
   its name. Do not simulate file edits. Spec composition requires its own
   permitted rendering surface, not a grant to modify project source.
2. **Read the documentation from the runtime docs store.** The admitted
   `json_render` schema's `ui_authoring` field carries this skill and pinned
   upstream references. When the chat adapter attaches selected references
   with their schema hash and source provenance, read those supplied contents;
   they are runtime documentation, not evidence that a file tool is available.
   For a tool-enabled session reading the filesystem bundle, start with the
   [documentation index](references/index.md). Read the pinned references
   for json-render, Svelte, SvelteKit and Vite. Use their APIs and syntax,
   checking versions against the project's lockfile or admitted build
   metadata. Do not rely on memory. If a required reference is unavailable or
   mismatched, stop the dependent work and identify it. Reference files in
   this bundle are documentation; they do not create a docs service or tool.
3. **Read the current catalog and registry in full.** Also read the active
   rendering profile and validator. Use only declared element and action
   names with actual registered implementations allowed by that profile.
   For new elements or actions, declare and register them first under the
   required project grants, then validate before generating a spec using
   them. A broad reference catalog alone is insufficient. If the runtime
   cannot expose the current registry, report that missing surface.
4. **Read navigation data from the runtime.** Use the same authoritative
   data source that populates the menus, including section/item ordering,
   routes, availability, WIP and placeholder flags. Do not reconstruct it
   from remembered menus or documentation examples. If that source is
   absent or denied, report it before changing navigation.

## Compose an interface

- Everything visible belongs to a declared, implemented spec element.
  Framework examples explain APIs; their example component names are not
  declarations in this application's catalog.
- Use the current profile's spec format and exact property schemas. With a
  flat element tree, every child ID must resolve and the root must exist.
  Library support for actions, bindings or state does not mean a static
  canvas profile permits them. Do not bypass validation with HTML or code.
- Read actual data through the caller's declared operations. Documentation,
  discovery instructions and topology are not measurements. Show unknown,
  empty, denied and unavailable states truthfully. Label requested sample
  data as illustrative; never invent healthy services or completed actions.
- In the chat canvas, each accepted code block is its own temporary catalog
  object, scoped to the conversation and removed when that chat ends. Use
  the existing lifecycle; a response does not promote itself to the gallery
  or permanent catalog. Check that the deployed runtime implements this
  lifecycle before claiming it did so.

## Change implementation

- Read each file fully before changing it. Make reviewable patches; do not
  use sed/awk substitutions. Preserve other agents' changes.
- Use the installed `svelte-code-writer` skill for Svelte code work and the
  `vite` skill for build configuration when available. Their documentation
  and analysis tools still require the caller's existing execution grants.
  If those tools are absent, do not invent results or install another MCP
  transport. See the index for pinned offline references and version limits.
- Declare props/actions in the catalog, implement Svelte components and
  action handlers, then register them. Update the active render profile and
  its existing export path together when necessary. Keep plugin schemas in
  their owning plugin; do not create a second schema authority.
- Preserve keyboard access, focus, accessible labels and current-route
  indicators. For a hover menubar, include click and keyboard operation,
  one open menu, delayed pointer exit, Escape/focus-leave closing, selectable
  WIP entries and disabled placeholders. Use runtime navigation data.
- Use the existing authenticated adapter and door for backend operations.
  A component, URL, skill or prompt never adds a capability. Keep provider
  credentials, session credentials and tokens out of specs, model-visible
  state, prompts and browser build variables.

## Report evidence

Report what was proposed, edited, validated and deployed separately. Use
actual tool results and operation receipts for completed actions, followed
by authoritative read-back where required. Preserve failures and cancellation;
never automatically replay an unknown-outcome operation. Respect the owner's
current build/test timing, and report deferred checks rather than a pass.

Runtime availability must be verified for each user mode. Installing this
skill for a development agent or staging the runtime bundle does not prove
that a provider-only chat has loaded it or gained file/operation tools.
