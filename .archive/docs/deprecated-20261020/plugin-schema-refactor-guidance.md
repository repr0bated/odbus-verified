# Plugin Schema Refactor Guidance

This document preserves the core instructions for continuing the plugin-as-schema refactor in the
control-plane chatbot context. Keep it in sync with the live refactor so the next run can pick up
without re-explaining the architecture.

## Core Rules
- The plugin owns a single canonical JSON-RCP document (`ctl-plane-chatbot` example) that carries
  schema, footprint policy, privacy instructions, tags, and metadata. Everything downstream reads
  through that document.
- `op_plugins::PluginCatalog`/`PluginRegistry` is the runtime catalog/cache. It **does not create new
  schema**; it hydrates from the persisted `op_dbus_model::SqlitePluginCatalog` (a.k.a. the schema
  catalog store) and exposes `SchemaCatalog`/`SchemaRegistry` as a read-only index.
- `SchemaCatalog` is the shared lookup/composition layer for validation, JSON rendering, vectorization,
  and compatibility exports. Rename callers to the catalog terminology, but keep compatibility aliases
  for the short term.
- Registration order: plugin code builds schema → canonical document → persist to catalog store → update
  in-memory catalog → export D-Bus/grpc projections.

## Operational Notes
- When rate-limited (Voyage or Cargo), pause embedding/indexing work and fall back to cached catalog
  copies; resume once the throttle lifts.
- If you are blocked mid-refactor, capture the last command outputs (cargo check/test) so the next
  iteration knows what failed.
- Keep the QA list minimal: `cargo check -p op-plugins`, `op-state(-store)`, `op-grpc-bridge`, `op-dbus-model`.

## To Do Next Time
1. Finish rehydrating startup so the catalog seeds from persisted documents before calling
   `PluginRegistry::register` (makes persisted authority the ground floor).
2. Remove any code still inferring schema by sampling live state (`build_plugin_schema` fallback) once
   all plugins ship explicit `schema()` implementations.
3. Align the `ctl-plane-chatbot` Kiro spec with this architecture (metadata, reasoning episodes, vectors).
