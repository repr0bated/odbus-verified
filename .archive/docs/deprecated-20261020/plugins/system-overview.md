# Plugin System Overview

This document is the official reference for the plugin creation/registration lifecycle, the schema catalog, and the control plane workflows that consume plugins. It synthesizes the refactor, the snowball mutation spec, and the chatbot-vector spec into a single authoritative guide.

## 1. Plugin-as-schema Rule
- Every plugin implements `op_state::plugin::StatePlugin` and provides `fn schema(&self) -> Option<PluginSchema>`.
- The schema defines all authoritative fields, privacy tags, semantic tags, read-only paths, and metadata.
- No runtime consumer invents a schema; everything resolves through the schema catalog.

## 2. Canonical Workflow
1. Plugin instantiation (per `DefaultPluginRegistry` or manual registration).
2. `PluginCatalog::register` builds the canonical `PluginSchema` via `StatePlugin::schema()` or compatibility entries, persists the `PluginCatalogDocument` in `op_dbus_model::SqlitePluginCatalog`, and indexes the shared `SchemaCatalog`.
3. Startup hydrates the in-memory catalog from persisted documents via `plugin_catalog.hydrate_catalog_from_store()` before registering new plugins.
4. Consumers access schema copies through `SchemaCatalog::get_copies(plugin_name)` for validation, rendering, footprint projection, vectorization, and compatibility exports.

## 3. Mutation-footprint Integration
- The `mutation_footprint` plugin (see `.kiro/specs/snowball-mutation-footprint`) owns the audit schema fields, persistence logic, snowball writes, and optional vectors.
- Mutation producers send events to this plugin, which uses the catalog to resolve hashes/chain linkage, writes to `StreamingSnowball`, and emits tracing spans.

## 4. Control-plane chatbot vectors
- The `ctl-plane-chatbot` schema (see `.kiro/specs/ctl-plane-chatbot-reasoning-vectorization`) defines reasoning episode fields and privacy/semantic tags.
- The vector worker renders embedding text according to that schema, calls Voyage, and upserts Qdrant payloads derived from the schema-led public footprint.

## 5. Documentation & Verification
- Use `docs/plugins/create-and-register.md` for step-by-step creation instructions.
- Update `.kiro/specs/...` when adding a new plugin to describe schema/footprint requirements.
- Run `cargo check -p op-plugins`, `cargo check -p op-state`, and any affected crate after schema changes.

When the above steps are followed, the plugin system becomes a coherent, catalog-first architecture that powers state, blocks, and embeddings consistently.
