# Findings — op-grpc-bridge

## F1 — Mixed native profile inventory rejected before target-family selection

**Evidence:** `src/tched_catalog_binding.rs` originally required every non-excluded candidate to use `target.family`, returning an error on the first unrelated family. The same file's existing `catalog_binding_distinguishes_platforms_and_path_boundaries` fixture passes both `custom.zen` (wrong endpoint) and `openai.go` (matching target family/endpoint), and expects `openai.go`. The guard therefore prevented the behavior asserted by the test.

**Decision/change:** skip unrelated-family candidates; continue enforcing the target family for candidates eligible to bind. This is a narrow correction to match the explicit mixed-family fixture while retaining fail-closed exact provider-family selection. Fresh JEV J1 reported the contradiction with confidence 0.91, but had low confidence on intent and proposed change (0.09 / 0.48); human disposition relies on the explicit regression-test expected behavior.

**Verification:** no Cargo/test/build/smoke command run, per assignment. Existing test is not claimed as executed. Coordinator must include the crate in its authorized build. Runtime and deployment behavior remain unverified.

**Other candidate concerns:** `$MODEL.n` resolver recognizes tokens in both `value` and `values` collections in `collect_model_tokens`, but `rewrite_model_tokens` visibly rewrites only a string `value` field. This possible mismatch is not adjudicated: the complete admitted schema, caller contract, tests and producer semantics were not read. No change made.

**Provider/model authority:** unresolved; see status. No claim that role labels, model registry ordering, native config, or local selection is authoritative beyond the directly read code.
