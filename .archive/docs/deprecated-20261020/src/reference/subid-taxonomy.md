# Subid Taxonomy

Every D-Bus object, plugin, schema, mutation, event, and tool carries a `uuid`
and a `subid`:

```text
<category>.<component-type>.<subject>.<verb>[.<facet>][@vN]
```

There are exactly seven categories: `src`, `prj`, `sch`, `mut`, `obs`, `evt`,
and `exp`. Subids are immutable per subject; material meaning changes require a
new subject with a `@vN` suffix.

TBD: complete category and component-type reference.
