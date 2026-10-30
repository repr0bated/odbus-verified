# Contextual declarative JEV receipts — op-identity

## Tool and method

Read `xd://eval/judge` instructions before calls. Used `judge(state, questions)` through `functions.eval`, solely for declarative evidence assessment. J2 is the initial occurrence-level call; J4 is the latest contextual reassessment after coordinator evidence. No model-routing or runtime claims.
## J2 — initial occurrence-level assessment (one call, seven independent choices; superseded for affected contexts by J4)


### State supplied

Exact source/caller evidence from the current complete sequential source reads:

- `src/bin/op-decoy-issuer.rs:11-27`: the decoy is the sole incoming WireGuard termination point and issues signed OIA1; `/identity/self` yields the JSON shape consumed by the 3tched UI, which fetches over the human tunnel and forwards the assertion unchanged over WARP to :8090. Lines 21-27 state the issuer's WG binding as the identity boundary.
- `src/bin/op-decoy-issuer.rs:369-379`: `pickup_json` documentation names `3tched.com` `door-config.ts` as the parser of op-web's same-origin pickup response; implementation returns door, `identity.kind=oia1`, assertion, and expiry.
- `src/bin/op-decoy-issuer.rs:481-491`: `cors_origin_is_exact_match_only` configures `https://3tched.com`, submits it in an Origin header, expects the exact value, and rejects `https://3tched.com.evil`. Each of lines 483-486 is a separately judged occurrence in the same exact-origin test.
- `src/session.rs:111-123`: one comment says “SQL is obsolete in the 3tched architecture”; adjacent lines scope the compatibility constructor to in-memory DashMaps and an identity vault under `/etc/ghostbridge`. Current crate session projection is authoritative per-session `identity_sled`; `session_projection.rs:100-113` states clients forward MutationEngine-authored SID1 without rebuilding identity. No source evidence supplies a replacement noun for the broad architecture phrase.
- Owner AGENTS distinguishes the `3tched-fs` filesystem, external `threetched` executable/surface, and UI checkout `/srv/git/3tched.com`. These distinctions do not by themselves imply the UI's proper name should be rewritten or identify an intended replacement for the broad session comment.

Each question included alternatives for retaining accurate wording, correcting a demonstrated error, or insufficient evidence. The six issuer contexts were individually represented rather than extrapolated from a generic scope question.

### Exact questions and observed output

Call result (choices, confidence, and probabilities as returned):

| Question key / occurrence | Question posed | Choice | Confidence | Probabilities (retain / correct / insufficient) |
|---|---|---|---:|---|
| `issuer_line14` (`op-decoy-issuer.rs:14`, chars 74-78) | Does this UI-shape phrase accurately identify the consumer, require a specific correction, or remain uncertain? | retain | 0.73 | 0.82 / 0.06 / 0.12 |
| `issuer_line369` (`op-decoy-issuer.rs:369`, chars 44-48) | Does the named parser reference accurately describe the consumer contract, require a correction, or remain uncertain? | retain | 0.86 | 0.91 / 0.03 / 0.06 |
| `issuer_test483` (`op-decoy-issuer.rs:483`, chars 30-34) | Is the URL origin an accurate exact-CORS fixture, a wrong fixture, or insufficiently evidenced? | retain | 0.98 | 0.99 / 0.01 / 0.00 |
| `issuer_test484` (`op-decoy-issuer.rs:484`, chars 79-83) | Is the request Origin literal accurate for the exact-match test, wrong, or insufficiently evidenced? | retain | 0.95 | 0.97 / 0.02 / 0.01 |
| `issuer_test485` (`op-decoy-issuer.rs:485`, chars 65-69) | Is the expected allowed-origin literal consistent with the exact-origin contract, wrong, or insufficiently evidenced? | retain | 0.95 | 0.97 / 0.02 / 0.01 |
| `issuer_test486` (`op-decoy-issuer.rs:486`, chars 69-73) | Is the evil suffix URL a correct negative spoof test, contradictory, or insufficiently evidenced? | retain | 0.97 | 0.98 / 0.01 / 0.01 |
| `session_line115` (`session.rs:115`, chars 36-40) | Is the broad architecture phrase defensible, specifically incorrect on available evidence, or insufficient to establish meaning/change? | insufficient | 0.33 | 0.44 / 0.01 / 0.55 |

Session wording judgment is explicitly low confidence and close between retain and insufficient. Human disposition is insufficient: exact current intended meaning/replacement is not established; do not invent a rename or tched-specific correction. It remains a documented uncertainty rather than a claim of proven accuracy.

## Earlier preliminary receipt — superseded, not used for final disposition

A prior preliminary call used incomplete occurrence coverage and incorrectly referenced stale line numbers (386 and 432). It judged issuer client reference `correct_client_ui_reference` (0.76; probability 0.84), issuer change `retain` (0.83; probability 0.89), and guessed vault replacement `no_replacement` (0.80; probability 0.87). Those were not applied to the exhaustive ledger; the vault string is not a `tched` occurrence. Retained here for audit history only. The final decisions above use J2's fresh exact source contexts and corrected coordinates.

## Disposition

Superseded by J4 for `session.rs:115`: the initial disposition was insufficient and retained provisionally, but the exact local implementation and owner rule support the bounded comment correction recorded above. Six issuer occurrences and four CORS fixtures retain their earlier context-specific dispositions. No JEV result establishes live UI deployment or runtime state.
## J3 — completed-receipt check

After status, coverage, findings, occurrences, J2 judgment, and both result receipts were written, a separate declarative question supplied their current contents/state: “Are the requested static audit/report steps complete now, is a specific required step pending, or is evidence insufficient? Distinguish the explicit unresolved wording interpretation from unfinished work.”

Result: `complete`, confidence 0.79; probabilities `complete` 0.86, `pending` 0.11, `insufficient` 0.03. This applies only to completion of the assigned static review and reports. It does not resolve the insufficient wording meaning at `session.rs:115`, establish deployment/runtime, or substitute for prohibited build/test execution.

### J2 exact `instructions` inputs

- `issuer_line14`: “Does the exact `3tched` UI-shape phrase at issuer line 14 accurately identify the consumer, require a specific correction, or remain uncertain?”
- `issuer_line369`: “Does the exact named `3tched.com` parser reference at issuer line 369 accurately describe the consumer contract, require a correction, or remain uncertain?”
- `issuer_test483`: “Disposition the URL origin at issuer test line 483: accurate test fixture for exact CORS behavior, correction, or insufficient evidence?”
- `issuer_test484`: “Disposition Origin header at issuer test line 484 in exact-match CORS test: accurate fixture, correction, or insufficient evidence?”
- `issuer_test485`: “Disposition expected allowed-origin value at issuer test line 485: accurate test expectation, correction, or insufficient evidence?”
- `issuer_test486`: “Disposition evil suffix URL at issuer test line 486: accurate negative origin test case, correction, or insufficient evidence?”
- `session_line115`: “Disposition generic phrase `3tched architecture` at session.rs line 115, considering this compatibility API and current projection authority: retain accurate enough, specifically correct, or insufficient to establish intended meaning. Do not guess product renaming.”

## J4 — coordinator-evidence reassessment

After reading `coordinator-followup.md`, all consumer source, and the exact local session implementation, submitted one fresh contextual call with three separate choices. State included `session.rs:84-129` (in-memory `DashMap<String, Session>`, separate `IdentityVault`, the current architecture-wide SQL comment and constructor); authoritative repository guidance that MutationEngine owns canonical state; `session_projection.rs:1-5,100-113` establishing the per-session projection and MutationEngine-authored SID1; issuer producer docs/implementation at `op-decoy-issuer.rs:11-27,369-379`; and the complete current UI consumer `src/chat/door-config.ts` at `/srv/git/3tched.com`, including its header/identity contract, `fetchIssuedIdentity`, `resolveIdentity`, and `fetchIdentityDocument`. Consumer code parses `identity`, `door`, and `expires_at` for issuer pickup. The state explicitly excluded runtime/deployment inference and distinguished producer payload shape from the consumer parser contract.

Question instructions and returned results:

| Key | Question | Choice | Confidence | Probabilities |
|---|---|---|---:|---|
| `session_scope` | “Given the exact local SessionManager fields/constructor and MutationEngine durable-state ownership, is `SQL is obsolete in the 3tched architecture` appropriately scoped, does it need a specific bounded comment correction, or is evidence insufficient? Assess the proposed replacement `This compatibility session manager keeps sessions in memory; MutationEngine owns durable session state.` for factual scope and risk of conflating the separate user vault.” | correct | 0.98 | retain 0 / correct 0.99 / insufficient 0.01 |
| `issuer_line14` | “Does the named UI/parser claim in issuer module lines14-19 match the independently read door-config.ts consumer source behavior, does it overclaim, or is evidence insufficient? Restrict claim to checked-in source, not live receiver/deployment.” | accurate_source_contract | 0.97 | accurate_source_contract 0.98 / overclaim 0.01 / insufficient 0.01 |
| `issuer_line369` | “Does issuer pickup_json comment line369 correctly describe door-config.ts parsing the producer shape from the op-web same-origin endpoint, or conflate producer and parser behavior / remain uncertain? Distinguish the parser shared input handling from the actual payload source; no deployment inference.” | accurate_shape_contract | 0.83 | accurate_shape_contract 0.89 / overclaim 0.05 / insufficient 0.06 |

Human disposition: correct only the overbroad `session.rs:115` comment to scope the statement to this compatibility manager's in-memory sessions and identify MutationEngine as durable session-state owner. The separate local user vault and behavior remain unchanged. Retain issuer docs: checked-in consumer independently supports their source-level shape/parser claim, but no live receiver/deployment claim is made. J4 supersedes J2's low-confidence `session_line115` insufficient disposition and adds direct consumer evidence for the issuer doc entries; J2's CORS fixture judgments remain applicable.

## J5 — final receipt check after coordinator follow-up

State supplied: final 20/20 source coverage, zero coordinator display gaps, seven baseline occurrences (one corrected, six remaining), supplemental full reads of all 28 mandatory skills without backdating, source-level UI parser evidence, all updated report/result receipts, and prohibited checks/actions not run.

Questions/results:

| Key | Question | Result | Confidence/probability |
|---|---|---|---|
| `final_completion` | “Does evidence establish completion of all allowed follow-up deliverables, is a required step pending, or evidence insufficient? Prohibited validation is intentionally not run; assess completeness only.” | `complete` | confidence 0.23; probabilities complete 0.49 / pending 0.42 / insufficient 0.09 |
| `honest_receipts` | “Do final receipts accurately state initial skill full-read receipts were not established, supplemental reads were completed without backdating, and live runtime/deployment remain unverified?” | true | probability 0.94 |

J5's completion answer is low confidence and not used as standalone proof. It does not identify a concrete pending step; the enumerated allowed follow-up deliverables and receipts are present. Record the uncertainty rather than upgrading the answer to certainty.