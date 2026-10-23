# Retained draft compatibility

This fork retains upstream jsonschema 0.29.1's `dependencies` extension in
Draft 2019-09 and Draft 2020-12. Both pinned official test corpora contain
`optional/dependencies-compatibility.json` and expect the legacy property and
schema dependencies to remain assertions. The modern standard keywords are
`dependentRequired` and `dependentSchemas`; callers needing portable modern
schemas should use those names.

`additionalItems` is supported only through Draft 2019-09. In Draft 2020-12,
`items` applies after `prefixItems`; `additionalItems` is an unknown annotation
and must not impose the earlier array constraints.

The corpus revision and original input hashes are recorded in
`test-source-provenance.json`. Retaining these compatibility cases does not
claim they were executed during source repair.

Unevaluated-item/property filters reject structurally cyclic annotation
dependencies that stay at one instance location. Compilation returns an error
instead of recursing indefinitely or inventing annotations for a cyclic edge.
JSON Schema Core 2020-12 section 9.4.1 leaves such infinite nesting undefined
and requires validators not to loop. References through child-instance
applicators, including recursive object properties and array items, remain
supported. This guard concerns the annotation-filter graph; it is not a claim
to implement fixed-point semantics for arbitrary reference cycles.

The guard supports at most 256 active filter objects along one in-place
annotation dependency path. Deeper paths return a compilation error, even if
acyclic, so the preflight itself has bounded recursion. This is an annotation
filter limit, not a general JSON nesting or core-reference depth guarantee.
Malformed Draft2019 `$recursiveRef` values also fail here when meta-schema
validation is disabled. The inherited GH-420 mutual object recursion case
advances through child properties and retains its successful compilation
expectation; same-instance cycle cases explicitly expect compilation errors.
Guard diagnostics retain the schema location and reason but use a null error
instance, avoiding a copy of a potentially large or deeply nested schema.

Each annotation-filter preflight also allows at most 4096 schema visits in
total, including repeated references and inline boolean leaves. A shallow
shared graph can otherwise expand exponentially without a path cycle. Crossing
this work limit returns a compilation error, including for a valid acyclic
schema. Traversal does not memoize by object identity: the same referenced
object is revisited under each actual resolver and dynamic scope. This is a
bound on one annotation preflight, not a global compilation-time guarantee.
