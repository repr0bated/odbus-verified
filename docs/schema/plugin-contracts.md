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

## Native Platform Method Observations

The fresh bridge's `runtime_platform.rs` uses an explicit `(plugin, method)`
inventory and retained typed native adapters; a schema declaration alone does
not make a method executable. `config`, `endpoint`, `agent_config`, and embedding
configuration share the parent-owned `RuntimeDocuments` handle. Publication
reads the committed document, not the submitted arguments.

Hardware, process, container, and service methods return native observations.
The bridge preserves unrelated parent fields and children, replaces complete
observed collections so deletions survive, and scopes platform observations to
the verified principal and session. Identity-managed native targets remain
behind the identity lifecycle/provenance owner, never generic container control.

A mutation failure with an unproven outcome is reported as unknown without
retrying the effect. A completed container receipt that disagrees with readback
carries the genuine observed state alongside that failure. Missing native
owners are not replaced with synthetic success, empty inventories, or a second
durable store. Integration/build and deployment evidence remain separate from
these source-level contracts.
