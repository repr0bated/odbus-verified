# Plugin schemas from structs (schemars)

**Standard for new state plugins:** define the config as ordinary `serde` +
`schemars::JsonSchema` structs and *derive* the `PluginSchema`. Do **not**
hand-build `FieldSchema`/`PluginSchema::builder` maps — those drift from the
struct they're supposed to mirror.

The struct is the single source of truth. `port` bounds, field descriptions
(from doc comments), `required` flags, patterns and immutable paths are all read
off the type.

## Recipe (new plugin)

```rust
use op_state_store::PluginSchema;

/// Doc comment on the struct → schema description isn't used; pass it explicitly.
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
// optional: declare immutable paths at the struct level
#[schemars(extend("x-immutable-paths" = ["sockets"]))]
pub struct MyPluginState {
    /// Doc comment → this field's `description`.
    pub path: String,
    #[schemars(range(min = 1, max = 65535))]   // → Constraint::Min/Max
    pub port: u16,
    #[serde(default)]                            // → not in `required`
    pub label: String,
}

pub fn my_plugin_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(MyPluginState))
        .expect("schemars schema serializes to JSON");
    crate::state_plugins::schemars_adapter::plugin_schema_from_json(
        "my_plugin", "1.0.0", "what this plugin manages", &root,
    )
}
```

Then return `Some(my_plugin_schema())` from the plugin's `schema()` method and
register it in `plugin_schema_defs.rs`.

## What the adapter maps

| schemars / JSON Schema | `op_state_store` |
|---|---|
| `string` / `integer` / `number` / `boolean` | `FieldType::String/Integer/Float/Boolean` |
| `array` (`items`) | `FieldType::Array` (recursed) |
| `object` (`properties`, via `$ref`/`$defs`) | `FieldType::Object` (recursed) |
| `enum` | `FieldType::Enum` |
| `minimum` / `maximum` / `pattern` | `Constraint::Min` / `Max` / `Pattern` |
| field doc comment / `description` | `FieldSchema.description` |
| `required: []` | `FieldSchema.required` |
| `default` / `examples[0]` | `FieldSchema.default` / `.example` |
| `readOnly` | `FieldSchema.read_only` |
| `#[schemars(extend("x-immutable-paths" = [...]))]` | `PluginSchema.immutable_paths` |

Reference implementation: `unix_socket` (`unix_socket_schema_derived`), guarded
by `derived_schema_matches_hand_rolled` and `all_subids_are_valid` tests. All
converted plugins follow the same two-test guard.

## Migrating an existing plugin

Conversion is **mechanical only when the struct already mirrors the schema**
(like `unix_socket`). The common case is a struct with opaque `Value` fields
(e.g. `cron`'s `jobs: Value`) while the hand-rolled schema spells out the detail.
There, migration means **fully typing the struct first** (replace `Value` with
real nested types), which also touches the plugin's runtime read/write code. Do
that deliberately, per plugin, with this recipe:

1. **Add `JsonSchema` to the (fully typed) struct(s).** Use `#[derive(schemars::JsonSchema)]`.
2. **Add OSCAL subids.** Put `#[schemars(extend("x-oscal-subid" = "..."))]` on the root struct and on every field. Use the AGENTS.md §4a taxonomy (`src|prj|sch|mut|obs|evt|exp`).
3. **Mark optional fields.** Add `#[serde(default)]` to fields that are not required.
4. **Declare constraints and read-only fields.** Use `#[schemars(range(min = ..., max = ...))]`, `#[schemars(pattern = "...")]`, `#[schemars(extend("readOnly" = true))]`, etc.
5. **Declare immutable paths at the struct level.** Use `#[schemars(extend("x-immutable-paths" = [...]))]` when the plugin has immutable paths.
6. **Add `*_schema_derived()` using the adapter.**
   ```rust
   pub fn my_plugin_schema_derived() -> PluginSchema {
       let root = serde_json::to_value(schemars::schema_for!(MyPluginState)).unwrap();
       super::schemars_adapter::plugin_schema_from_json("my_plugin", "1.0.0", DESC, &root)
   }
   ```
7. **Keep the old hand-rolled schema as a `#[cfg(test)]` golden reference.** Name it `my_plugin_schema_golden()`.
8. **Add two tests:**
   - `derived_schema_matches_hand_rolled` — use `schemars_adapter::schema_diffs` to compare the derived schema against the golden reference. The test must assert the diff is empty.
   - `all_subids_are_valid` — collect every subid from the derived schema and run `common::oscal::validate_subid` on each one.
9. **Once green, point `schema()` and `plugin_schema_defs.rs` at the derived version.**

Plugins that already follow this recipe include:
- Phase 1/2: `unix_socket`, `oscal_subid_registry`, `cron`
- Phase 3: `zeroclaw`, `antigravity`, `antigravity_chat`
- Phase 4 (mechanical): `adc`, `gcloud_adc`, `agent_config`, `mcp`, `compact_mcp`, `cognitive_mcp`, `keypair`, `endpoint`, `net`, `hardware`, `software`, `sessdecl`, `config`, `ctl_plane_chatbot`
- Phase 5 (no-struct authored): `lxc`, `procfs`, `notebooklm`, `web_ui`, `wgcf`, `xray`, `workflows_plugin`, `schema_renderer`

Remaining plugins still use their original hand-rolled schemas and should be migrated only when typing their structs is independently worthwhile.

**Do not** attempt a bulk sweep — it's a runtime-touching refactor, not a
find-and-replace.

## Shared LLM projection (`common/llm_projection.rs`)

New LLM-related plugins should reuse the shared projection types instead of
redeclaring provider/model/tool schemas. The module at
`crates/op-plugins/src/state_plugins/common/llm_projection.rs` defines:

- `Provider`, `ModelRoute`, `Router`, `LlmTool`, `ConfigSchema`, `UiSurface`,
  `StructuredOutput`, and the composite `LlmProjection`.

Each struct derives `schemars::JsonSchema` and carries OSCAL subids, so the
D-Bus projection is generated directly from the type. Embed `LlmProjection` in a
plugin state struct and call `plugin_schema_from_json` on the root, or use the
`schema_helpers::golden_from_state_and_schema` helper when the plugin needs to
overlay `schema_from_state` defaults onto the schemars-derived base.

Plugins currently using it: `zeroclaw`, `antigravity`, `antigravity_chat`.

## OSCAL subid gate

Every converted plugin is guarded by an `all_subids_are_valid` test that runs
`common::oscal::validate_subid` against every `x-oscal-subid` declared on the
root and fields. The canonical regex and category rules live in
`crates/op-plugins/src/state_plugins/common/oscal.rs`. CI runs these tests, so
new schemars-derived schemas must carry valid subids from the AGENTS.md §4a
taxonomy or the test suite will fail.
