# Declarative JEV receipts — op-introspection

Task: native-line-audit-20261024/op-introspection. Date: 2026-10-24. Auditor: Codex crate worker.

Installed facility: read `xd://eval/judge`; invoked `await judge(state, questions)` once through JS eval, transcript label **Judging native Cozo contexts**. Three successful typed choice answers. No model override/routing, source filtering, extraction, process code or delegation. Full tool-emitted state/questions/answers receipt: `artifact://0` (session-local); this durable report retains source references, actual wording, question rubrics and answers. The full sequential coverage is the auditor's observed evidence, not a JEV-certified absence proof.

## Shared evidence state

All ten inventory files fully read sequentially through EOF, 1,704 lines; root/src directory listings reconcile. Occurrence ledger contains exactly the two observed native uses:

- `crates/op-introspection/SPEC.md:88`: `The derived index uses the shared Cozo store and native full-text search.` Following context: `A configured index path opens a sibling <path>.cozo directory; any existing SQLite file remains untouched. Rebuild the derived index from D-Bus to populate the new store. :memory: remains available for transient indexes and tests.` Markdown quoting in this sentence is normalized only for readability; exact source is in the read receipt.
- `crates/op-introspection/src/indexer.rs:3-5`: `//! Search uses Cozo's native full-text index over service, path, interface,` / `//! member, description, and signature. Query syntax follows Cozo FTS (including` / `//! terms, phrases, Boolean operators, and prefix \`*\`); invalid syntax is an error.`

Exact source supplied to JEV from indexer.rs:63-65,110-143,285-305,337-363 included the `CozoGraphShuttle` field, memory/persistent constructor branch appending `.cozo`, all three relation/FTS schema strings, existing-relation error handling, concatenation of all six documented index fields, prefix normalization, both typed/untyped FTS query strings, parameter bindings, `run_query` and `FtsHit` decode. The decisive exact fragments:

```rust
"::fts create dbus_index_items:search { extractor: search_text, tokenizer: Simple, filters: [Lowercase] }",
```

```rust
let search_text = [
    record.service.as_str(), record.object_path.as_str(),
    record.interface.as_str(), record.item_name.as_str(),
    record.description.as_str(), record.signature.as_str(),
].join(" ");
```

```rust
"?[service, object_path, interface, item_type, item_name, description, signature, score] := ~dbus_index_items:search{service, object_path, interface, item_type, item_name, description, signature | query: $query, k: $k, bind_score: score, filter: item_type == $kind}",
"?[service, object_path, interface, item_type, item_name, description, signature, score] := ~dbus_index_items:search{service, object_path, interface, item_type, item_name, description, signature | query: $query, k: $k, bind_score: score}",
```

Generic evidence supplied separately in the same state: SPEC.md:19-30,69-81 lists ten Rust files including mod.rs/hierarchical.rs/cpu_features.rs; comparison:11-21,35-40,76 claims these present and lists rusqlite. Current root/src listing contains seven Rust files, manifest depends on op-cozo-store rather than rusqlite. lib.rs:8 says FTS5; parser.rs:12-19 ignores `_xml` and returns empty interfaces/children, while scanner implements XML parsing.

State limit explicitly supplied: static source-only; no certification of dependency internals, runtime deployment, tests, external caller identity enforcement or global text absence; unrelated generic defects not authorized for repair.

## J1

Question: Evaluate BOTH actual native wording occurrences for intended meaning and semantic accuracy against exact Cozo FTS implementation. Does either need a native-specific correction? Distinguish a built-in database capability from an AI provider/model selection mode.

Choices:
- `preserve_accurate_fts`: Both native uses accurately denote Cozo built-in full-text search using ::fts create and ~relation:search. No native-specific wording fix.
- `change_needed`: At least one native use misstates shown behavior or denotes an unauthorized implicit selection requiring correction.
- `insufficient`: Evidence does not determine intended meaning or semantic accuracy.

Actual answer: `preserve_accurate_fts`; confidence **0.88**. Probabilities: preserve_accurate_fts **0.92**, change_needed **0.07**, insufficient **0.01**.

Decision: preserve both occurrences. Relation creation, extracted search_text fields and actual Cozo FTS queries substantiate the descriptive meaning. No source patch.

## J2

Question: Does the shown data flow itself implement an implicit provider/model/config override or duplicate plugin schema authority merely because it says native? Select only conclusions grounded in the shown evidence.

Choices:
- `no_shown_override`: The native text describes a Cozo search index; queries are text retrieval, no provider/model override or independent plugin definition is demonstrated.
- `override_shown`: Evidence demonstrates native mode overriding authoritative provider/model/config choice or duplicate plugin definition.
- `insufficient`: Cannot classify even this shown data flow.

Actual answer: `no_shown_override`; confidence **1**. Probabilities: no_shown_override **1**, override_shown **0**, insufficient **0**.

Decision: no native routing/schema-authority defect demonstrated in assigned crate. Does not certify external callers or deployed admission.

## J3

Question: Classify generic inventory/FTS5/parser discrepancies separately from the two native FTS descriptions. Should these unrelated issues be changed under this narrow native audit?

Choices:
- `report_generic_only`: Stale inventory/FTS5 wording and parser empty output are independently observable generic issues, not a reason to rename accurate native Cozo FTS or expand this repair.
- `native_fix_required`: Those discrepancies show either native description is inaccurate and requires a native-specific fix.
- `insufficient`: Evidence cannot distinguish generic discrepancies from native-specific issues.

Actual answer: `report_generic_only`; confidence **1**. Probabilities: report_generic_only **1**, native_fix_required **0**, insufficient **0**.

Decision: retain separate generic source-only observations in findings.md; no unrelated edits.
