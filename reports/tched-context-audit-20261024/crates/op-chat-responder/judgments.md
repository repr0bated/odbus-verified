# Declarative JEV judgments

Installed declarative `judge(state, questions)` via the eval tool. Four successful calls, 22 contextual choices; no completion/model-routing substitution. The initial calls used exact source context and local callers plus then-available repository naming authority; the follow-up J4 adds complete current registry/plugin producer reads and the coordinator-saved mounted-view/config.get operation receipts. Source/runtime limits were explicit.

## Call 1 — all initial occurrences

Common question, independently keyed to every context O1–O11: “For exact occurrence Oi, choose whether this local tched/threetched reference is accurate to retain, materially misleading and needs scoped correction, or evidence is insufficient. Judge actual context, role, intent, wording accuracy and effect; do not infer live runtime or outside-scope implementation.”

Choices: `retain` = accurately identifies local product/source role; `correct` = materially conflates/misnames role or false tched-specific claim; `insufficient` = evidence does not establish meaning/accuracy.

| ID | Line | Exact context | Choice | Confidence | Probabilities retain/correct/insufficient |
|---|---:|---|---|---:|---|
| O1 | 9 | `ZeroClaw/\`threetched\` is foundation only` | retain | 0.69 | 0.79 / 0.17 / 0.04 |
| O2 | 10 | `\`threetched.config.get\`` | retain | 0.80 | 0.87 / 0.12 / 0.01 |
| O3 | 11 | `\`threetched.stateless-generate\`` | retain | 0.70 | 0.80 / 0.18 / 0.02 |
| O4 | 11 | `or any threetched chat/generate CLI` | retain | 0.66 | 0.77 / 0.21 / 0.02 |
| O5 | 30 | `without threetched chat/generate CLI` | retain | 0.46 | 0.64 / 0.34 / 0.02 |
| O6 | 31 | `\`threetched.stateless-generate\`` | retain | 0.43 | 0.62 / 0.34 / 0.04 |
| O7 | 31 | `\`tched_router.Chat\`` | retain | 0.23 | 0.49 / 0.40 / 0.11 |
| O8 | 152 | the `\"threetched\"` plugin argument to `views.call_response(..., "config.get", ...)` | retain | 0.38 | 0.59 / 0.08 / 0.33 |
| O9 | 204 | `(not threetched generate CLI)` | retain | 0.66 | 0.77 / 0.19 / 0.04 |
| O10 | 219 | `do not use threetched.stateless-generate` | retain | 0.77 | 0.85 / 0.13 / 0.02 |
| O11 | 220 | `or any threetched chat/generate CLI` | retain | 0.74 | 0.83 / 0.16 / 0.01 |

O5–O8 were initially low confidence; J2 supplemented O5 and O7, and the original J2 O8 `insufficient` answer correctly reflected its then-missing contract evidence. J4 re-evaluated O2, O7 and O8 using current producer and projected-operation evidence. Its bounded O8 disposition does not establish grants, successful execution, or read-only semantics.

## Call 2 — closer evidence for low-confidence contexts

Questions/answers (choices: retain / correct / insufficient):

- `cli_meaning` (O5): Is “threetched chat/generate CLI” an accurate local distinction of the external executable's CLI, or misleadingly names another integration? **retain**, confidence 0.52; probabilities 0.68 / 0.23 / 0.09. Still weak; external CLI behavior is not independently established.
- `router_meaning` (O7): Does “not tched_router.Chat” accurately name a distinct prohibited alternative rather than conflate it with the threetched executable? **retain**, confidence 0.67; probabilities 0.78 / 0.19 / 0.03. Supported by source's distinct `tched_router.Chat` token and the owner naming distinction.
- `plugin_id` (O8): What disposition is warranted for `plugin="threetched"` given the mounted call and owner authority, which does not declare this sealed plugin contract? **insufficient**, confidence 0.97; probabilities 0.02 / 0 / 0.98. Retain source unchanged; do not claim the sealed contract is verified.

## Call 3 — filesystem package and imports

Common question, independently keyed to F1–F5: “For exact occurrence Fi, does the local name accurately match repository naming authority and source purpose; is it materially misleading and should be corrected; or is evidence insufficient?” Choices: `retain` = accurate package/directory/module role; `correct` = misnames or conflates roles; `insufficient` = evidence does not establish role/accuracy.

Exact contexts and successful answers:

| ID | Location / exact occurrence | Context | Choice | Confidence | Probabilities retain/correct/insufficient |
|---|---|---|---|---:|---|
| F1 | `Cargo.toml:16`, col 1 `tched-fs` | dependency package name | retain | 0.97 | 0.98 / 0.02 / 0 |
| F2 | `Cargo.toml:16`, col 30 `threetched-fs` | relative source-directory component in path | retain | 0.91 | 0.94 / 0.05 / 0.01 |
| F3 | `src/main.rs:21`, col 5 `tched_fs` | crate import for `invocation::Deadline` | retain | 0.92 | 0.95 / 0.03 / 0.02 |
| F4 | `src/main.rs:22`, col 5 `tched_fs` | crate import for mounted namespace API | retain | 0.94 | 0.96 / 0.02 / 0.02 |
| F5 | `src/main.rs:23`, col 5 `tched_fs` | crate import for `MountIdentity` | retain | 0.92 | 0.95 / 0.02 / 0.03 |

## Call 4 — current producer and projected operation

State: Fresh source reads of `default_registry.rs` (628 lines) and `state_plugins/threetched.rs` (465 lines) show `tched_router` retired and `threetched` registered; the latter generates methods from captured binary commands and assigns `SideEffect::Mutation` / `idempotent=false`. The current caller passes one path string in `arguments` and `--json: true`. Coordinator-saved projected-operation receipts identify `config.get`, require exactly one string argument, allow optional boolean `--json`, and return `{stdout, stderr, exit}`; metadata requires a method-specific capability and declares mutation/non-idempotent. The saved view receipt (2026-10-24T20:11:35Z, generation 216) contains both `tched_router` and `threetched`. Coordinator reports listing `plugins/threetched/data` and `value.json` returned EIO. No method call, grant check, or runtime bypass was performed.

| ID | Exact context | Choice | Confidence | Probabilities |
|---|---|---|---:|---|
| O2 | `main.rs:10`, `threetched.config.get` | retain | 0.94 | retain 0.96 / correct 0.03 / insufficient 0.01 |
| O7 | `main.rs:31`, exclude `tched_router.Chat` for `ChatManager` | retain | 0.97 | retain 0.98 / correct 0.02 / insufficient 0 |
| O8 | `main.rs:152`, `threetched` + `config.get` + caller input | retain_bounded | 0.85 | retain_bounded 0.90 / correct 0.08 / insufficient 0.02 |

J4 retains O8's exact target/method/input shape only. The projected declaration's mutation, non-idempotence and required capability are material constraints; no claim of a grant, successful call, or read-only operation follows.

## Human disposition

No in-scope source correction is supported after J4. O8 moved from J2's insufficient result to retain_bounded (0.85) on the additional source and projected-operation evidence. O5/O6 remain qualified negative constraints; external CLI behavior is not established. No JEV answer establishes a grant or successful invocation.
