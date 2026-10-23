# Spec grammar

This file used to restate json-render's output contract by hand. The contract
itself now travels with the catalog export
(`schemas/json-render/catalog.prompt.md`), which is what the model is given; see
`json-render-catalog.md` in this directory for the export and how to regenerate
it. What is documented here is only what this repo adds on top.

## What the model emits

A JSONL stream of RFC 6902 patch operations, one JSON object per line, applied to
an empty document until it becomes a spec. `add`, `replace` and `remove` are the
operations in use; state patches follow the elements that read them so a renderer
can fill in progressively.

```jsonl
{"op":"add","path":"/root","value":"main"}
{"op":"add","path":"/elements/main","value":{"type":"card","props":{"title":"Mesh","tone":"ok"},"children":["peers"]}}
{"op":"add","path":"/elements/peers","value":{"type":"statCard","props":{"label":"Peers","value":{"$state":"/netmaker/peers"},"sub":null,"variant":"ok"}}}
{"op":"add","path":"/state/netmaker/peers","value":7}
```

`op_gallery_gen::spec_stream::assemble` turns that stream into the stored spec. It
tolerates prose and code fences around the JSONL (models drift; a turn is too
expensive to discard over a sentence) and refuses anything that would invent
content the model did not describe: an unsupported operation, a gap in an array,
a path that is not a JSON Pointer.

## What admission checks

`op_gallery_gen::validator::SpecValidator` has two layers, and the split matters:

- **Grammar**, always on: `root` exists and resolves, `elements` is an object,
  every child reference exists, no cycles, plus the dedup signature.
- **Vocabulary**, only with a catalog attached (`SpecValidator::with_catalog`):
  component names, declared props and their types, required props, undeclared
  props, children on leaf components, `visible` conditions, and JSON Pointer
  syntax for `$state`/`$bindState`.

A validator without a catalog cannot reject a component name — it has no
vocabulary to reject it against — so every admission path attaches one and fails
closed when the artifact is missing or its digest does not match.

Props holding a directive (`{"$state": …}`, `{"$cond": …}`, `{"$template": …}`)
are checked for pointer shape but never for type: a directive's resolved type is a
property of runtime state, not of the spec. Two thirds of the live shell spec's
elements bind at least one prop, so type-checking them would reject valid work.

## Error codes

| Code | Layer | Meaning |
|---|---|---|
| `E_INVALID_SPEC` | grammar | not a JSON object |
| `E_MISSING_ROOT` | grammar | no `root`, or it names an element that does not exist |
| `E_MISSING_ELEMENTS` | grammar | no `elements` object |
| `E_MISSING_TYPE` | grammar | element without a `type` |
| `E_DANGLING_REF` | grammar | child id with no element |
| `E_CYCLE` | grammar | element tree is not a tree |
| `E_UNKNOWN_COMPONENT` | catalog | component the catalog does not declare |
| `E_UNKNOWN_PROP` | catalog | prop the component does not declare |
| `E_PROP_REQUIRED` | catalog | declared-required prop absent |
| `E_PROP_SCHEMA` | catalog | prop value fails the catalog's schema |
| `E_PROPS_NOT_OBJECT` | catalog | `props` is not an object |
| `E_CHILDREN_NOT_ALLOWED` | catalog | children on a component with no slots |
| `E_VISIBLE_SCHEMA` | catalog | `visible` is not a valid visibility condition |
| `E_BIND_PATH` | catalog | `$state`/`$bindState` value is not a JSON Pointer |
