# Validation boundaries in this fork

This fork retains the existing legacy `dependencies` extension in Draft
2019-09 and Draft 2020-12. Use `dependentRequired` and `dependentSchemas` for
portable modern schemas. `additionalItems` is supported only through Draft
2019-09; Draft 2020-12 uses `items` after `prefixItems`.

Unevaluated-item/property filters reject cyclic annotation dependencies
that remain at the same instance location. Compilation returns an error
instead of recursing indefinitely or inventing annotations. References
through child-instance applicators, including recursive object properties
and array items, remain supported.

Each annotation-filter preflight supports at most 256 active filter objects
on one dependency path and 4096 schema visits across all branches. Repeated
references and boolean leaves consume the shared visit budget. Exceeding
either limit returns a compilation error, including for a valid acyclic
schema. References are revisited under their actual resolver and dynamic
scope rather than memoized by object identity. These bounds apply to one
annotation preflight, not to all compilation work or JSON nesting.

Malformed Draft 2019-09 `$recursiveRef` values also fail this preflight when
meta-schema validation is disabled. Diagnostics preserve the schema
location and reason, using a null error instance to avoid copying an
arbitrarily large schema.

These repairs were reconciled from the earlier reviewed source at
`b3d156215930bd5000a458c6b2c155704c98bcca`. Historical test corpus and helper
packages were not restored as part of this source reconciliation. Source
regression cases do not establish that tests have been executed.
