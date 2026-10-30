# How to Create and Register a Plugin

This standalone guide lists the steps to implement a new state plugin, provide its schema, and register it
with the canonical plugin catalog. It does not assume any refactor stage; it simply documents the
authorized workflow you must follow every time you add a plugin.

## 1. Implement the `StatePlugin` trait

1. Create a Rust module under `crates/op-plugins/src/state_plugins/` (or reuse another crate if appropriate).
2. Implement `op_state::plugin::StatePlugin` for your type. At minimum provide:
   - `fn metadata(&self) -> PluginMetadata` with descriptions, dependencies, and optional D-Bus service names.
   - `async fn query_current_state(&self)` and `async fn apply_state(&self, diff: &StateDiff)`.
3. Define `fn schema(&self) -> Option<PluginSchema>` that returns the canonical schema for this plugin:
   - Use `PluginSchema::builder(name)` and describe every field (types, defaults, descriptions).
   - Mark `semantic_index`/`privacy_index` paths, tags, immutability rules, and sensitivities that the
     vectorization/JSON rendering pipelines will consume.
   - Return `Some(schema)`; there is no fallback inference.

> Tip: If the schema is static, you can define a helper `pub fn schema() -> PluginSchema` and call it from `StatePlugin::schema`.

## 2. Provide lifecycle/context helpers (optional)

If your plugin needs:

- BTRFS storage -> use `PluginRegistry::create_plugin_subvolume` or similar.
- NUMA affinity -> store assigned node in `PluginContext` when initialized.
- Custom tooling/events -> expose metadata or register tools like the chatbot tooling path.

Always keep these helpers consistent with the plugin schema; do not add runtime fields outside the schema contract.

## 3. Register the plugin

1. Add the plugin name to `DefaultPluginRegistry` (`crates/op-plugins/src/default_registry.rs`) so it is auto-loaded at
   startup, or register it manually (e.g., `plugin_catalog.register(Arc::new(MyPlugin::new())).await?`).
2. During registration, `PluginCatalog::register` will:
   - Build your schema via `StatePlugin::schema()` (or built-in compatibility entry if the schema already exists in
     `SchemaCatalog`).
   - Persist the canonical `PluginCatalogDocument` to `op_dbus_model::SqlitePluginCatalog`.
   - Index the schema into the shared `SchemaCatalog` so validation, JSON rendering, and vectorization read the same
     fields.
   - Export D-Bus/gRPC projections derived from that catalog entry.
3. Ensure the schema document contains metadata such as `service_name`, `storage_path`, and `source`.

## 4. Include the catalog in the workflow

1. The canonical document produced by `PluginCatalog::register` is persisted to `op_dbus_model::SqlitePluginCatalog`.
2. At startup the catalog store hydrates the shared `SchemaCatalog` via `plugin_catalog.hydrate_catalog_from_store()`, and each registration updates that same catalog entry.
3. Consumers such as validators, vector workers, and renderers resolve schema/footprint data through `SchemaCatalog::get_copies(plugin_name)` so they never invent a parallel schema.

## 5. Keep downstream flows schema-driven

- Any worker that builds embeddings must call `SchemaCatalog::get_copies(plugin_name)` and render text according to the schema’s `semantic_index`/`privacy_index` instead of hardcoding fields.
- JSON renderers, vector payload writers, and compatibility adapters read the schema via the catalog.
- If you need a new vector collection, configure it to accept only fields approved in the plugin schema.

## 6. Document the plugin

- Add a short description in `docs/plugins/plugin-catalog.md` and link to a spec if needed.
- If the plugin needs a dedicated spec (e.g., the chatbot conversation plugin), create one under `.kiro/specs/` describing the schema/footprint/flow.

## Verification checklist

- `cargo check -p op-plugins`
- `cargo check -p op-state` (if your plugin interacts with state manager)
- Ensure the schema catalog contains the new entry (`PluginCatalog::register` logs the persistence step). 
