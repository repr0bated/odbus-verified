# Declarative JEV receipts

## J1 — exact_schema.rs native contexts

Actual installed `judge()` call via functions.eval (JavaScript), task native-line-audit-20261024/op-state-store. The complete submitted state is in the tool transcript and output artifact `artifact://0`; no model routing requested.

Evidence supplied: `src/exact_schema.rs:16-20,193-199,247-265` error contract and upstream validation boundary; actual native fixture contexts at `518-575,604-633,737-761`, including complete JSON documents, `.field("native", ...)` declarations, encoded lookup, valid/invalid state payloads and public-state pointers. Non-native depth loop summarized; no native occurrence omitted. All source read sequentially through 763 EOF before judgment. Context row mapping: occurrences.tsv (17 positions).

Questions and explicit contrary outcomes:

- `diagnostics`: Is “native validator diagnostics” accurate wording given the upstream jsonschema boundary? `retain` = library-original diagnostics intentionally not exposed; `fix` = wording misrepresents behavior; `insufficient` = evidence cannot decide.
- `fixtures`: Evaluate every native key/path at lines 542,546,551,556,557,558,564,566,569,573,615,617,631,741,760,761 for semantic, wording or routing correction. `retain` = consistently bound arbitrary exact-schema fixture fields/JSON pointers, no provider selection/product assertion/parallel authority; `fix` = at least one context inaccurate or overriding authority; `insufficient` = contexts cannot decide.

Actual returned answers:

```json
{"diagnostics":{"type":"choice","choice":"retain","confidence":0.94,"probabilities":{"retain":0.96,"fix":0.03,"insufficient":0.01}},"fixtures":{"type":"choice","choice":"retain","confidence":0.92,"probabilities":{"retain":0.95,"fix":0.05,"insufficient":0}}}
```

Decision: retain all 17 occurrences. Library diagnostic terminology is supported by fixed-code errors and `.is_valid`; fixture names consistently identify exact-schema boundaries. This is static contextual review, not a claim those fixtures ran or passed.

## J2 — remaining terminology and lexical contexts

Actual installed `judge()` call via functions.eval (JavaScript). Complete submitted source state and explicit choice questions are in this session's tool-call transcript (`evidenceJ2`, `answerJ2` in the eval kernel); no search, execution or model routing.

Evidence supplied:

- `schema_validator.rs:192-269`: “Expand propertyDependencies to if/then for validators that don't support it natively”, both documented JSON shapes, complete helper flow removing `propertyDependencies`, appending `allOf` if/then branches with required discriminator, and recursively transforming nested objects. “Natively” describes direct keyword support, not readOnly authorization.
- `state_schema_shape.rs:21-32,119-209,459-483`: finite tagged-intersection producer contract, normalization behavior and complete fixture. Exact occurrence: `"device":{"$ref":"#/$defs/Base","type":"object","properties":{"type":{"type":"string","const":"nic"}},"required":["type"],"description":"native tagged device"}`. Original base defines required string parent. Fixture preserves original input, checks derived tag/ref state and compares validators across valid/invalid/open-property examples. Annotation is fixture-local, not binary or deployment provenance.
- `schema_projection.rs:78-103`: “Treat secrecy / privacy as a conservative annotation across every applicable schema branch. JSON Schema `allOf` combines constraints, so selecting only the first property declaration must not discard a later `x-secret: true` or `x-opdbus-private: true`. Applying the same rule to alternatives avoids exposing a value when any accepted shape redacts it.” Actual function checks the direct property then any branch of allOf/anyOf/oneOf recursively. “alternatives” contains the letters native but is ordinary branch vocabulary; included conservatively as lexical-only coverage.

Questions with explicit contrary outcomes:

- `natively`: Does line192 need contextual correction? retain = accurate built-in/direct keyword support, no readOnly-enforcement/model-routing claim; fix = wording contradicts translation; insufficient = cannot decide.
- `tagged_fixture`: Does line462 need correction? retain = descriptive original-schema fixture annotation without authority/selection claim; fix = misleading product/behavior assertion or override; insufficient = intended meaning not established.
- `lexical_alternatives`: Evaluate substring context, including local accuracy, not routing only. retain = ordinary schema branches and conservative any-branch redaction support sentence; fix = native-specific semantic defect; insufficient = cannot decide.

Actual returned answers:

```json
{"natively":{"type":"choice","choice":"retain","confidence":0.95,"probabilities":{"retain":0.97,"fix":0.03,"insufficient":0}},"tagged_fixture":{"type":"choice","choice":"retain","confidence":0.98,"probabilities":{"retain":0.99,"fix":0.01,"insufficient":0}},"lexical_alternatives":{"type":"choice","choice":"retain","confidence":0.92,"probabilities":{"retain":0.95,"fix":0.03,"insufficient":0.02}}}
```

Decision: retain all three. No native-specific correction or overlap established. No runtime/test claim.

## V1 — compact-summary claim check (not accepted)

Actual `functions.jev_verify`, model `jev-latest`, supplied the completed coverage summary, 20 occurrence context summaries, actual J1/J2 results and separate generic issues. Claim: “The reviewed native contexts in op-state-store need no native-specific wording or behavior change; preserving them is consistent with their displayed intent. This is a static contextual conclusion, not test or deployment proof.”

Returned `verdict=insufficient — low confidence, review`, `verbatim=0.04` (no quote requested), `misleading=1.1/3`, `confidence=0.43`, `tokens(in/out)=908/81`. This low-confidence summary-only check is not accepted as a disposition and does not supersede source-context judgments. Supplemented immediately with J3, not silently discarded or retried unchanged.

## J3 — full-context supplemental disposition

Actual installed `judge()` call via functions.eval, state included original complete `evidenceJ1` and `evidenceJ2` source-context objects plus prior actual answers and V1's limitation. No new source extraction or model routing. Question explicitly says not to rely only on prior answers; evaluate every native word/key including diagnostic terminology, fixture keys/pointers, tagged-device annotation, built-in keyword-support wording and lexical alternatives. Explicitly excludes coverage certification, runtime, test and general-quality claims.

`native_disposition` choice outcomes: retain_contexts = all submitted words/fixture keys preserve local source intent with no native-specific evidenced defect (unrelated behavior unapproved); change_needed = at least one contextual native-specific wording/behavior defect; insufficient = material native-specific question unresolved.

Actual returned:

```json
{"native_disposition":{"type":"choice","choice":"retain_contexts","confidence":0.83,"probabilities":{"retain_contexts":0.89,"change_needed":0.07,"insufficient":0.04}}}
```

Decision: no native-specific patch. J3 supplements every occurrence's original J1/J2 mapping, resolving V1's weak compact-summary check. Total actual installed judge calls3 / contextual answers6; plus one non-decisive jev_verify.
