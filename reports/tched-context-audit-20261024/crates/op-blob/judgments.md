# Contextual declarative JEV receipt

**Call:** installed `judge(state, questions)` via eval, successful. One evidence state, four independent choice questions. The failed first syntax attempt did not produce a judgment; the successful result is the recorded receipt. Exact successful answer object:

```json
{"capability":{"type":"choice","choice":"retain_contextual_reference","confidence":0.98,"probabilities":{"retain_contextual_reference":0.99,"correct_in_scope":0.01,"insufficient_evidence":0}},"subid":{"type":"choice","choice":"retain_contextual_reference","confidence":0.98,"probabilities":{"retain_contextual_reference":0.99,"correct_in_scope":0.01,"insufficient_evidence":0}},"plugin_builder":{"type":"choice","choice":"retain_contextual_reference","confidence":0.98,"probabilities":{"retain_contextual_reference":0.99,"correct_in_scope":0.01,"insufficient_evidence":0}},"descriptor_call":{"type":"choice","choice":"retain_contextual_reference","confidence":0.98,"probabilities":{"retain_contextual_reference":0.99,"correct_in_scope":0.01,"insufficient_evidence":0}}}
```

## Submitted evidence

Source: `crates/op-blob/src/descriptor.rs:871-908`, complete test `cli_flag_fields_round_trip_with_exact_json_names_and_existing_numbers`. It declares method `configure`, capability `threetched.read`, subid `obs.threetched.configure@v1`, `PluginSchema::builder("threetched")`, and `descriptor_set_for_plugin("threetched", &schema)`. Assertions verify `--config-dir` → `__config_dir` while preserving JSON name, `-f` → `_f`, stable numbers 1–3, and JSON roundtrip. The test is a fixture: no method dispatch, external receiver, catalog publication, authorization decision, or runtime-availability claim.

Owner/repository context supplied: `3tched-fs` is the filesystem/package identity; external runtime executable/surface is lowercase `threetched`; `tched_router` is a distinct plugin integration. Names must follow source purpose and actual authority, not substring replacement. This audit requires contextual intent review, not a blanket rename.

## Questions and choices

Each question gave the full local test context plus owner naming guidance, described the actual local purpose, asked for one of three outcomes (`retain_contextual_reference`, `correct_in_scope`, `insufficient_evidence`), and explicitly stated no live routing/dispatch is evidenced.

- **J1 / capability / descriptor.rs:877:** “For the exact occurrence at descriptor.rs:877, judge its actual role and contextual accuracy. Is this tched-specific product identity that should be retained, an incorrect/in-scope name needing correction, or insufficient evidence? A method test fixture uses it as a required capability while testing CLI flag descriptor mapping; no real operation dispatch is shown. Choose only from evidence.” Result retain, confidence 0.98.
- **J2 / subid / descriptor.rs:878:** “For the exact occurrence at descriptor.rs:878, judge its actual role and contextual accuracy. Is this tched-specific product identity that should be retained, an incorrect/in-scope name needing correction, or insufficient evidence? It is the subid attached to the CLI-field mapping test's configure declaration, not an invocation. Choose only from evidence.” Result retain, confidence 0.98.
- **J3 / plugin builder / descriptor.rs:886:** “For the exact occurrence at descriptor.rs:886, judge whether the plugin id threetched is appropriate in this test's actual purpose: descriptor field-name conversion for CLI flags, JSON-name and numbering round-trip. Retain, correct in scope, or insufficient evidence. No external router/consumer is shown.” Result retain, confidence 0.98.
- **J4 / descriptor call / descriptor.rs:888:** “For the exact occurrence at descriptor.rs:888, judge whether the plugin id threetched is appropriate when synthesizing the descriptor set for the same configure test fixture. Retain, correct in scope, or insufficient evidence. The preceding schema builder uses the same id; this tests descriptor names, not live dispatch.” Result retain, confidence 0.98.

All answers support retention of their corresponding fixture terms only. They do not establish runtime existence, grants, publication, caller behavior, or deployment. No weak or conflicting answer.