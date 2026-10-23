# Plugin Contract Model

## Contract Envelope

Each plugin object is modeled with a uniform envelope:

- `schema_version`
- `plugin`
- `object_type`
- `object_id`
- `stub`
- `immutable`
- `tunable`
- `observed`
- `meta`
- `semantic_index`
- `privacy_index`

Primary implementation: `crates/op-plugins/src/state_plugins/schema_contract.rs`

## Section Roles

- `stub`: source identity and discovery metadata.
- `immutable`: identity fields and immutable creation metadata.
- `tunable`: desired/operator-controlled state.
- `observed`: runtime observation and drift markers.
- `meta`: dependency graph, recovery inclusion/priority, sensitivity tags.
- `semantic_index`: what data is vectorized for semantic search.
- `privacy_index`: redaction rules (`secret_paths`, `pii_paths`, action behavior).

## Uniformity Rules Applied

- Absolute JSON pointer paths in semantic/privacy include/exclude lists.
- Dependency targets must be known plugin schemas.
- Recovery priority bounded.
- Plugin contracts aligned to plugin state shapes (example: `dinit.services` instead of `dinit.units`).

## Field-Quality Hardening

Contract schemas were tightened where previously underspecified:

- Explicit `required` keys for critical nested structures.
- `additionalProperties` controls for shape predictability.
- Typed nested arrays/objects for common plugin tunables.
- Privacy and semantic path coverage for sensitive fields.

## Materialization Behavior

On mutation apply:

- If payload is contract-like, missing envelope sections are filled automatically.
- If payload is non-contract, plugin schema template is generated and merged.

This enforces schema/plugin coupling at runtime.
