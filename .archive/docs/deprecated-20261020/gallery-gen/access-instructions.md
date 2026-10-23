# Plugin Schema Access Instructions

This document explains how to read and interpret the sealed plugin blob catalog. It is provided to inference models during gallery generation.

## What a PluginSchema Is

Every plugin in the OP-DBUS control plane publishes a `PluginSchema` — a JSON object describing its state fields, methods, and identity bindings. The schema IS the plugin's contract. There is no separate API documentation.

The sealed blob catalog at `/dev/shm/opdbus/plugin-blobs/` contains one binary blob per plugin. Each blob has sections:
- **Section 1**: The canonical `PluginSchema` JSON (this is what you read)
- **Section 3**: Protobuf reflection descriptors (for wire encoding only — do not use for schema discovery)

Read schemas via:
- `op_blob::catalog::read_plugin_schema_shm(plugin_id)` on-host
- `GET /api/ui-model/plugin-schema/:plugin_id` over HTTP
- `PluginService.GetSchema(plugin_id)` over gRPC

## Schema Structure

```json
{
  "name": "plugin_name",
  "version": "1.0.0",
  "description": "What this plugin does",
  "fields": {
    "field_name": {
      "field_type": { "object": { "properties": {...} } } | { "array": {...} } | "string" | "number" | "boolean",
      "description": "Human-readable explanation",
      "default": <value>,
      "required": true | false,
      "read_only": true | false,
      "constraints": { "min": 0, "max": 100, "pattern": "..." },
      "subid": "obs.category.plugin.field@v1"
    }
  },
  "methods": {
    "method_name": {
      "description": "What calling this method does",
      "args": { <JSON Schema of input struct> },
      "returns": { <JSON Schema of output struct> },
      "side_effect": "read" | "mutation",
      "idempotent": true | false,
      "required_capability": "plugin.read" | "plugin.write",
      "subid": "mut.category.plugin.method@v1"
    }
  },
  "subids": {
    "sch.category.plugin.schema@v1": "Schema-level identity",
    "obs.category.plugin.field@v1": "Field-level identity"
  },
  "mutation_index": 42,
  "guarantees": ["atomic", "consistent"]
}
```

## Field Types and Their Meaning

| Type | Renders as | Notes |
|------|------------|-------|
| `string` | Text input | Use for identifiers, names, paths |
| `number` | Numeric input or slider | Check `min`/`max` constraints for range |
| `integer` | Whole number input | Often used for ports, counts |
| `boolean` | Toggle or checkbox | On/off states |
| `array` | Table or list | `items` describes each row's schema |
| `object` | Nested section or card | `properties` contains nested fields |
| `enum` | Dropdown | `enumValues` lists valid options |
| `oneOf` | Discriminated union | Multiple possible shapes — render as switcher |

## Constraints

- `min`, `max` → Numeric bounds. Use for validation and slider ranges.
- `pattern` → Regex validation for strings.
- `minLength`, `maxLength` → String length bounds.
- `required` → Array of field names that must be present.
- `readOnly` → Field is display-only, not editable.
- `default` → Initial value if not specified.

## Method Side Effects

| Side effect | Meaning | UI treatment |
|-------------|---------|--------------|
| `read` | Safe query, no state change | Refresh button, auto-poll |
| `mutation` | Changes state | Confirmation dialog, audit trail |

Methods with `mutation` side effect MUST show accountability fields (`actor_id`, `capability_id`) if the plugin has any `mut.*` subids.

## Subids (OSCAL Identity)

Every schema element carries an OSCAL subid for audit traceability:

```
<category>.<component-type>.<subject>.<verb>@vN
```

Categories:
- `src` — Source/provenance
- `prj` — Project context
- `sch` — Schema definition
- `mut` — Mutation/action
- `obs` — Observation/telemetry
- `evt` — Event record
- `exp` — Export/exposure

Use subids to:
- Group related fields across plugins
- Find fields by audit category
- Ensure generated UI elements are traceable

## Binding Live Data

In a json-render spec, `bind` props reference live plugin state:

```json
{ "type": "label", "props": { "bind": "/status" } }
```

The path is a JSON pointer relative to the plugin's current state. When the spec renders, the interpreter resolves the pointer against live data.

For method results, bind against the response:

```json
{ "type": "table", "props": { "bind": "/peers", "columns": [...] } }
```

## Plugin Categories

Each plugin has an `x-oscal-category` indicating its domain:

- `software` — Application/service plugins
- `network` — Networking (OVS, WireGuard, Xray)
- `system` — System state (procfs, services)
- `security` — Security and identity
- `compliance` — OSCAL, audit trails
- `observability` — Logging, metrics

Use categories to scope generation ("only network plugins", "cross security and compliance").
