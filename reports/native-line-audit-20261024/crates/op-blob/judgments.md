# Declarative JEV evidence — op-blob

Task `native-line-audit-20261024`, 2026-10-24, Codex crate worker. JEV is evidence only, not runtime proof or mutation authority. Every cited source file was already read in full. Initial source hashes and read ranges: [coverage.tsv](coverage.tsv).

## Successful calls and invocation receipt

- Read installed instructions `xd://eval/judge` before use.
- Batch `jdgb-15b31e394e8f5ff8`: 3/3 judged, TypeSafe `typesafe/jev-1.14.0`, $0.0001, 0.2s (completion receipt). Initial invocation mistakenly used `await judge_batch(...)`; Python raised `TypeError: object JudgmentBatch can't be used in 'await' expression` after launching the batch. This was an invocation error, not an unavailable judge. Its completed results were recovered with supported `judge_batch.attach(...).results()`.
- Corrected batch `jdgb-15b31e445d0f5ff9`: 3/3 judged, `typesafe/jev-latest`, 0s (completion receipt). Supported call: `native_batch = judge_batch(native_states, native_questions, intent='Judging op-blob native contexts')`, then `await native_batch.drain()`. Results below agree with the recovered first batch. Full untruncated responses read through `artifact://1:1-1:raw` and `artifact://2:1-2:raw`.
- Separate successful `await judge(state, questions)` call, transcript label **Judging default and schema authority**, returned both authority judgments below.
- No JEV routing extension, model selection, source filtering or reranking used.

## Batch questions (same questions for each evidence state)

**meaning — choice:** “Classify the native wording/key in its exact supplied source context. Does this evidence establish an implicit provider/model/config override or a native-routing admission prerequisite? Judge only supplied evidence, not deployment.”

Criteria:
- `legitimate_nonrouting`: The occurrence is descriptive storage/protocol vocabulary or a test schema key; supplied code does not select or override an LLM route.
- `runtime_override`: The native occurrence or associated executable code selects/overrides a provider/model/config or imposes native routing admission without explicit admitted selection.
- `insufficient`: Evidence cannot distinguish routing override from descriptive/schema vocabulary.

**disposition — choice:** “For the native-specific audit only, is removing or renaming this native occurrence justified by supplied evidence? Preserve legitimate protocol/storage vocabulary and authored contract keys; do not infer runtime behavior from tests.”

Criteria:
- `preserve`: No native-specific override established; removal/rename would be unrelated or change contract vocabulary without justification.
- `fix_independently`: Concrete independent native override defect shown and a scoped fix is justified.
- `defer_overlap`: A concrete native override is shown but fixing it overlaps tched ownership.
- `insufficient`: Need more source to disposition.

## Evidence states

### storage_wording

Sources: `crates/op-blob/README.md:31-48`, `crates/op-blob/src/btrfs.rs:40-145`.

Literal evidence supplied:

```text
- **One source of truth** — every artifact (D-Bus path, gRPC services,
  reflection descriptors, manifest, compliance metadata) derives from
  `PluginSchema`; nothing is hand-authored downstream.
- **Btrfs filesystem in blob** — the outermost deployment artifact is a
  btrfs image containing the sealed blobs (snapshot/send/receive-able,
  mountable read-only so consumers see the surface as native local).
```

Also supplied the exact `seal_blobs_into_btrfs_image(blob_paths: &[PathBuf], image_path: &Path) -> io::Result<BtrfsImageReport>` opening through its empty-input/missing-mkfs rejection. Context explicitly stated the remainder validates manifest plugin IDs, stages caller-supplied blobs, prepares an image with mkfs.btrfs, checks Btrfs magic and returns a report. No provider/model API in the file. This does not establish live deployment.

### wire_wording

Sources: `crates/op-blob/src/descriptor.rs:1-18,308-381`, `crates/op-blob/src/json_schema_proto.rs:16-85`.

Literal evidence supplied:

```rust
//! Typed gRPC reflection descriptors synthesized from `PluginSchema.methods`
//! (REQ-6.1): per-method `FileDescriptorProto`s with real field types derived
//! from `MethodDecl.args` / `MethodDecl.returns` JSON Schemas. Typed fields
//! retain their native types; dynamic JSON uses protobuf's well-known types.
```

Also supplied the exact `json_schema_type_to_proto(node)` classification call and scalar match arm mapping `string`, `int64`, `uint64`, `double`, `bool` to descriptor constants, with `Value` fallback. Marked the excerpt noncontiguous, omitting other match arms. Full source shows only schema-driven transport mapping, not model/provider/account selection.

### fixture_key

Source: `crates/op-blob/tests/exact_field_contract.rs:8-97`.

Literal evidence supplied includes:

```rust
    let mut schema = PluginSchema::builder("exact_fixture")
        .version("2.0.0")
        .field(
            "native",
            FieldSchema {
                field_type: FieldType::exact_json_schema(document.clone()).unwrap(),
                required: true,
                description: "Internal source contract".into(),
                default: None,
                example: None,
                constraints: vec![],
                read_only: true,
                read_only_when: None,
            },
        )
        .build();
```

Noncontiguous literal assertion/input evidence also supplied:

```rust
    assert_eq!(
        export["properties"]["native"]["properties"]["jobs"]["additionalProperties"]["$ref"],
        "#/properties/native/$defs/Row"
    );
    let input = json!({"native":{"jobs":{"stable_key":{"revision":u64::MAX,"label":null,"children":{"stable_grandchild":false}}}}});
    // The exact field is internal, regardless of the method output fixture.
    assert_eq!(
        schema
            .project_public_state(&simd_json::serde::to_owned_value(input).unwrap())
            .unwrap(),
        None
    );
```

Context supplied: complete test seals exact schema, rebases refs, validates input, roundtrips sealed protobuf descriptors, and expects no public state. No routing calls or model/provider keys. Test was read, not run.

## Batch results (both successful batches)

| State | meaning | Confidence | Probabilities: legitimate / override / insufficient | disposition | Confidence | Probabilities: preserve / fix / defer / insufficient |
| --- | --- | ---: | --- | --- | ---: | --- |
| storage_wording | legitimate_nonrouting | 1.00 | 1 / 0 / 0 | preserve | 0.93 | 0.95 / 0.01 / 0 / 0.04 |
| wire_wording | legitimate_nonrouting | 1.00 | 1 / 0 / 0 | preserve | 0.81 | 0.86 / 0.05 / 0.01 / 0.08 |
| fixture_key | legitimate_nonrouting | 1.00 | 1 / 0 / 0 | preserve | 0.92 | 0.94 / 0.01 / 0 / 0.05 |

Decision: preserve all three contexts. Renaming is not the owner's requested removal of implicit routing/config overrides.

## Default and full-schema authority judgment

Evidence: exact complete bodies of `apply_state_defaults` at `adapter.rs:267-291` and `blobify_plugin_schema_with_identity` at `lib.rs:79-100`, including their doc comments. No `native` occurrence was claimed here. Important mutations supplied verbatim:

```rust
        field.default = Some(value.clone());
```

Only fields already present in the supplied schema are visited. The helper takes `schema: &mut PluginSchema` and `state: &JVal`; it does not mutate `state`.

```rust
    schema.name = blob::canonical_plugin_id(plugin_id);
    let canonical_value = blob::canonicalize_json_object_keys(
        serde_json::to_value(&schema).expect("op-state-store PluginSchema serializes"),
    );
    // Explicitly sort object keys because serde_json's `preserve_order`
    // feature may be enabled elsewhere in the workspace.
    let canonical_json =
        serde_json::to_string(&canonical_value).expect("canonical value serializes");
    let working: PluginSchema = serde_json::from_value(canonical_value)
        .expect("op-blob PluginSchema view accepts op-state-store schema");
    blob::blobify_canonical(&working, canonical_json, identity)
```

Question `default_behavior`: “Does apply_state_defaults itself override a live caller-selected provider/model/config, or only project defaults into a schema? Evaluate exact implementation, not the name overwrite. No native occurrence is claimed in this code.”

Choices: `schema_metadata_only` (only supplied schema default metadata), `implicit_runtime_override` (function itself writes live routing/config without request), `insufficient` (mutation target unclear).

Answer: **schema_metadata_only**, confidence **1.00**, probabilities **1 / 0 / 0**. Decision: no independent native fix.

Question `sealing_authority`: “Does the shown workspace sealing entry create independent hard-coded model/provider/schema authority, or seal the supplied full PluginSchema and derive a reduced view? Identity/capability values must not be inferred from generated transport metadata.”

Choices: `supplied_schema_authority` (same supplied full schema, derived view, identity forwarded), `independent_authority` (hard-coded independent contract or capability grant), `insufficient`.

Answer: **supplied_schema_authority**, confidence **1.00**, probabilities **1 / 0 / 0**. Decision: preserve full-schema producer boundary; transport synthesis is not independent model selection or authorization.

## Limits

Judgments evaluate already-read source evidence, not runtime, caller admission beyond this crate, unreviewed dependencies, deployment or test execution. No confidence value is treated as a runtime pass.

## Final bounded claim verification

Successful `jev_verify` with model `jev-latest` checked the conclusion that the
supplied occurrences describe mounted storage, protobuf type fidelity and an
internal test field, not provider/model selection or a native-routing admission
gate; therefore renaming them is not justified by this evidence. Evidence cited
README:41-43, descriptor:1-4,328-381 and exact_field_contract:15-30,51-60,90-96.
Result: **supports**, confidence **0.95**, misleading **0.1/3**, tokens in/out
**857/78**. No verbatim quotation was claimed (`quote` was empty); the tool's
`verbatim=0.13` is not used as a quotation assertion or runtime verification.

## Occurrence reconciliation

Owner clarification read in full from updated `per-crate.md` (19:15Z clarification)
and `reconcile-occurrences.md`. This section supersedes the earlier **routing-only
sufficiency** assessment; it does not retract the original full-file reads.
No `coordinator-followup.md` was present in the complete assignment listing.

### Accounting and evidence scope

Manual sequential review accounts for **18 case-insensitive native substrings on
17 lines in 5 files**: 7 literal `native` tokens and 11 incidental substrings in
`alternatives`. This conservative accounting includes non-word substrings rather
than silently excluding them. Every occurrence has its own row N01–N18 in
`occurrences.tsv`. `line#position` is the left-to-right native-substring ordinal
on that line, not a fabricated byte column. The two tokens at descriptor.rs:224
are separate rows.

Prior full reads of all 19 files remain evidence. Sequential rereads: README1–78,
adapter1–240/241–440/441–612, descriptor1–235/236–470/471–705/706–928,
json_schema_proto1–112, exact_field_contract1–97, all through EOF. The remaining
14 files have zero occurrences under their retained complete read coverage.
No source search, extraction, filtering or arbitrary eval used.

### Actual installed JEV batch

`judge_batch(occurrence_states, occurrence_questions,
intent='Evaluating every op-blob native occurrence')` returned
**jdgb-15b3279b8f2b2faa**. Completion: **7/7**, TypeSafe
**typesafe/jev-1.14.0**, **$0.0003**, **0.2s**. Full untruncated returned
judgments read from `artifact://3:2-2:raw`. All seven states and questions were
hand-authored from source already read, not generated by a search or extraction.

**context_accuracy** — exact question:

> Evaluate EVERY listed native-substring occurrence in its actual supplied context
> and intended meaning. Is its naming/wording and any native-related behavioral
> implication accurate and appropriate? Check semantic/doc accuracy, not merely
> absence of LLM routing. Incidental native inside alternatives still must be
> evaluated. Keep unrelated generic defects outside this decision. Consider
> documented limits and distinguish a source/test design from runtime proof.

Choices:
- `accurate_preserve`: All listed occurrences are accurate/coherent vocabulary or
  fixture/identifier names in context. No native-specific wording/behavior change
  is justified.
- `needs_native_change`: At least one listed occurrence has misleading native
  wording or native-specific behavior requiring a scoped change; a no-routing
  observation alone does not cure it.
- `insufficient_evidence`: Missing context prevents disposition of one or more
  listed occurrences.

**intent** — exact question:

> What is the intent of the listed occurrences in the supplied code/docs? Evaluate
> actual effect, not the substring alone.

Choices: `local_filesystem` (ordinary filesystem after explicitly mounting, not
existing deployment), `typed_protocol` (schema-derived protobuf with bounded/dynamic
limits), `schema_union_alternatives` (local branch-array helper; incidental spelling),
`exact_schema_fixture` (coherent internal key/pointer, not production model-route
setting), `native_policy_or_mislabel` (native policy or misleading semantic/product
label established), `insufficient_evidence`.

### Source states actually supplied

**O01_storage — N01:** README:26–28,41–43,57 plus btrfs:15–19,135–145.
The literal native sentence and mount command above were supplied again, together
with the actual Btrfs report's read-only mount hint and catalog-open path. Meaning
declared: ordinary locally mounted files, not an LLM route or existing deployment.
The question was explicitly limited to the native-local wording, not unrelated
historical deployment/standalone statements.

**O02_adapter_field — N02/N03:** exact source `adapter.rs:126–144` supplied:

```rust
    // `anyOf`/`oneOf`: Option<T> collapses to T; multi-branch unions become a
    // real discriminated `OneOf` so all variants render.
    if let Some(alternatives) = node
        .get("anyOf")
        .or_else(|| node.get("oneOf"))
        .and_then(JVal::as_array)
    {
        let non_null: Vec<&JVal> = alternatives
            .iter()
            .filter(|a| type_str(a) != Some("null"))
            .collect();
        match non_null.as_slice() {
            [] => {}
            [single] => return field_type(single, defs),
            many => {
                return FieldType::OneOf(many.iter().map(|a| field_type(a, defs)).collect());
            }
        }
    }
```

Meaning: branch-array binding and iterator source. No blanket certification of
surrounding union/constraint semantics requested.

**O03_adapter_constraint — N04/N05:** exact complete `constraint_node` body
and its two-line comment at adapter:221–238 supplied:

```rust
// The finite producer preflight admits only one non-null optional branch
// when a shared FieldSchema must carry its constraints.
fn constraint_node<'a>(node: &'a JVal, defs: Option<&'a Map<String, JVal>>) -> &'a JVal {
    let resolved = resolve(node, defs);
    if let Some(alternatives) = resolved
        .get("anyOf")
        .or_else(|| resolved.get("oneOf"))
        .and_then(JVal::as_array)
    {
        let mut concrete = alternatives
            .iter()
            .filter(|node| type_str(node) != Some("null"));
        if let (Some(single), None) = (concrete.next(), concrete.next()) {
            return constraint_node(single, defs);
        }
    }
    resolved
}
```

Meaning: candidates for the single non-null optional branch's constraints.

**O04_descriptor_wording — N06:** exact descriptor opening1–4, fallback
documentation191–193, bounded/ref fallback315–328, scalar mapping367–381 and
mapper precision comment4–5 supplied. Native wording interpreted as schema-to-wire
typing, not original Rust ABI. Context explicitly included string-enum→string,
int→64-bit mapping, bounded/recursive fallback and dynamic Value numeric limits.
Unlike the earlier judgment, this asked whether those limits make the wording
misleading, not simply whether it routes models.

**O05_descriptor_resolve — N07–N10:** complete `MappingContext::resolve`
implementation at descriptor:208–233 supplied, including cycle/depth/reference logic.
The complete union arm actually evaluated was:

```rust
            } else if let Some(alternatives) = node
                .get("anyOf")
                .or_else(|| node.get("oneOf"))
                .and_then(JVal::as_array)
                .filter(|alternatives| alternatives.len() == 1)
            {
                node = &alternatives[0];
            } else {
                return Some(node);
            }
```

Both separate line224 tokens were explicitly listed in the state. Meaning: branch
array, closure parameter, length-check receiver, and sole branch.

**O06_mapper_union — N11–N13:** exact opening of `classify` through the
union arm at json_schema_proto:20–35 supplied:

```rust
fn classify(schema: &Value, depth: usize) -> (String, bool) {
    if depth >= 64 {
        return ("google.protobuf.Value".into(), false);
    }
    if let Some(alternatives) = schema
        .get("anyOf")
        .or_else(|| schema.get("oneOf"))
        .and_then(Value::as_array)
    {
        // Value preserves null and heterogeneous unions without inventing a
        // default scalar, or discarding one branch of the plugin's contract.
        if alternatives.len() != 1 {
            return ("google.protobuf.Value".into(), false);
        }
        return classify(&alternatives[0], depth + 1);
    }
```

Meaning: union array, count, and sole-branch reference. Context explicitly described
the remaining scalar/array/message mapping and documented precision boundary.

**O07_fixture — N14–N18:** exact declaration10–30, sealing/decoding44–65
and private-projection comment/assertion90–96 supplied as noncontiguous excerpts.
All five native lines were explicitly included, not represented by one sampled key:

```rust
            "native",
        serde_json::to_value(&decoded.fields["native"].field_type).unwrap()["json_schema"],
        export["properties"]["native"]["properties"]["jobs"]["additionalProperties"]["$ref"],
        "#/properties/native/$defs/Row"
    let input = json!({"native":{"jobs":{"stable_key":{"revision":u64::MAX,"label":null,"children":{"stable_grandchild":false}}}}});
```

These are separate original lines18,51,57,58,60, not one contiguous expression.
Meaning: declaration, decoded lookup, export lookup, rebased pointer, instance key.
Method insertion/wire-roundtrip middle was described as omitted, not fabricated.
Test was explicitly **not run**; no deployed schema or passing assertion inferred.

### Complete batch outcomes

Accuracy probabilities are in order preserve/change/insufficient. Intent probability
is the selected class; residual classes are stated below when nonzero.

| State | Rows | context_accuracy | Confidence | Probabilities | intent | Confidence | Selected probability |
| --- | --- | --- | ---: | --- | --- | ---: | ---: |
| O01_storage | N01 | accurate_preserve | 0.27 | 0.51 / 0.47 / 0.02 | local_filesystem | 1.00 | 1.00 |
| O02_adapter_field | N02–N03 | accurate_preserve | 0.53 | 0.69 / 0.28 / 0.03 | schema_union_alternatives | 1.00 | 1.00 |
| O03_adapter_constraint | N04–N05 | accurate_preserve | 0.69 | 0.79 / 0.17 / 0.04 | schema_union_alternatives | 1.00 | 1.00 |
| O04_descriptor_wording | N06 | accurate_preserve | 0.31 | 0.54 / 0.43 / 0.03 | typed_protocol | 0.98 | 0.98 |
| O05_descriptor_resolve | N07–N10 | accurate_preserve | 0.74 | 0.83 / 0.14 / 0.03 | schema_union_alternatives | 1.00 | 1.00 |
| O06_mapper_union | N11–N13 | accurate_preserve | 0.51 | 0.67 / 0.25 / 0.08 | schema_union_alternatives | 0.90 | 0.92 |
| O07_fixture | N14–N18 | accurate_preserve | 0.55 | 0.70 / 0.15 / 0.15 | exact_schema_fixture | 1.00 | 1.00 |

O04 residual intent: native_policy_or_mislabel0.02; all other labels0.
O06 residual intent: typed_protocol0.08; all other labels0.
All unlisted intent probabilities for other states are0.

### Low-confidence follow-through: substantive verification, not routing-only

Batch accuracy below0.6 was **not** accepted as a conclusion. Re-examined supplied
contexts: read-only mount + std::fs catalog reading; typed mapping and explicit
fallback/numeric limits; all five coherent fixture uses; branch arrays and local
references. Four successful installed `jev_verify` calls refined the concrete
semantic claims below. Each used `model=jev-latest`, an empty quote and actual
source evidence; no runtime inference. The original uncertainty is retained above.

**V01_storage — N01.** Claim:

> In README.md:43, 'native local' accurately describes reading the image's files
> through an ordinary local read-only mount. It is a mountability/design statement,
> not a claim that the mount is already deployed or that plugin operations are
> provided by this image alone. The native wording itself needs no change.

Evidence supplied: README26–28,41–43,57; btrfs15–19,66–89,135–145;
catalog143–147 reading blob files using `std::fs::read(path)`. Distinguishes
preparation, explicit activation/mount and tmpfs runtime consumption.
Result **supports**, confidence **0.92**, misleading **0.4/3**, verbatim0.12,
tokens865/78. Decision: preserve conditional local-filesystem wording.

**V02_protocol — N06.** Claim:

> The descriptor.rs:4 wording 'retain their native types' is appropriate shorthand
> for schema-derived protobuf typing instead of JSON text, within the explicit
> dynamic/bounded-fallback limits in this module. It does not promise original Rust
> ABI types or lossless preservation of arbitrary dynamic JSON integers, and no
> native-specific wording change is required.

Evidence supplied: descriptor1–4,191–193,315–328,343–381; complete mapper branch
behavior24–85 and precision comment4–5; sealed descriptor contract tests156–182
and249–258 as **unexecuted source intent**. Typed scalar/nested/map handling and
Value fallback boundaries were specifically considered.
Result **supports**, confidence **0.89**, misleading **0.5/3**, verbatim0.18,
tokens895/78. Decision: preserve contextual vocabulary, without claiming arbitrary
lossless typing or runtime proof.

**V03_fixture — N14–N18.** Claim:

> All five native occurrences in exact_field_contract.rs form one coherent,
> accurately named internal exact-source-schema fixture: declaration at18, decoded
> field lookup51, export property57, rebased reference target58, and JSON input
> key60. They do not claim deployed configuration or test success. No native-specific
> rename/change is needed.

Evidence supplied: full test flow with exact positions10–30,31–43,44–60,61–89,
90–96. Schema's `description` is `Internal source contract`, `read_only=true`,
the same field keys align across schema/export/reference/input, and public
projection is asserted None. Explicitly not executed.
Result **supports**, confidence **0.70**, misleading **0.5/3**, verbatim0.07,
tokens866/78. Decision: preserve all five coherent fixture positions.

**V04_union_names — N02/N03/N11–N13.** Claim:

> The two alternatives occurrences in adapter.rs:128,133 and the three in
> json_schema_proto.rs:24,31,34 are accurately named local JSON Schema branch
> arrays. The native substring is incidental English spelling, not an intended
> native feature/policy. Their naming needs no change; this does not certify
> unrelated union-conversion semantics.

Evidence supplied: literal branch-array arms from adapter126–144 and
json_schema_proto20–35, including binding, null filtering, length checks and
single-branch references. Explicitly limits claim to identifier semantics,
not generalized lossless schema enforcement.
Result **supports**, confidence **0.76**, misleading **0.3/3**, verbatim0.06,
tokens884/78. Decision: preserve these five accurate identifier uses.

`verbatim` values are not used as quotation checks: all four calls used an empty
quote. No low-confidence result is promoted into runtime acceptance. O03/O05
already had accuracy confidence0.69/0.74 and exact complete function bodies.

### Final decision

All18 positions evaluated in actual source context, including docs/comments,
fixture keys/pointers and every incidental `alternatives` token. Preserve all18.
No native-specific required change or overlapping hunk remains. Previous
representative/routing-only judgments alone were insufficient; these semantic
judgments plus `occurrences.tsv` close that gap. No source edits, tests/checks,
builds, runtime actions, deployment or shared handoff mutation in this continuation.
