# Jev strategy review (jev-1.14.0, 2026-10-28)

Jev answered atomic questions about the strategy state in `review/jev_strategy.py`; the findings below are decided in code from fixed thresholds (yes ≥ 0.7, no ≤ 0.3).

- Scope fit: 1.98 on 0 (fits) … 2 (clearly over-scoped), probabilities {'0': 0.0, '1': 0.01, '2': 0.99}.
- Overall judge appeal: 1.39 on 0 (familiar) … 2 (novel, clearly demonstrated), probabilities {'0': 0.05, '1': 0.51, '2': 0.44} — split between 'interesting' and 'stand-out'.

## Findings (decided in code)
- (unsettled) Rewrite the thesis in plain words for non-specialists. — `judge.thesis_clear` = 0.32
- **Cut the concepts shown in the demo to the three that carry the thesis.** — `judge.concept_overload` = 0.97
- **Most of the golden path is unproven: prove it end to end before adding scope.** — `feas.path_mostly_proven` = 0.12
- **The golden path depends on open issues: fix those first.** — `feas.path_depends_on_open_issue` = 0.94
- **Get Meta's model wired in first; it is a hard requirement.** — `feas.meta_blocker` = 0.95
- **De-risk tool calling: test it early with Meta's model or move tool calls into code.** — `feas.tool_calling_risk` = 0.97
- **Stabilise the platform: commit often, fix the activation health check, make the UI part of a release.** — `feas.platform_unstable` = 0.97
- **Record a full fallback run as soon as the golden path first works.** — `feas.fallback_ready` = 0.02
- **Do not demo with root: use a dedicated low-privilege identity or restrict the demo to pre-tested operations.** — `sec.demo_overprivileged` = 0.98
- **Prove on-chain anchoring early; it is the verification claim.** — `sec.anchoring_proven` = 0.16
- **Name a first customer (even a design partner) for the pitch and YC.** — `biz.first_customer_named` = 0.03
- **State a revenue model for YC.** — `biz.revenue_stated` = 0.03
- **Lead with verifiability and compliance; present the blockchain as the neutral witness, not the product.** — `biz.web3_may_deter_enterprise` = 0.85

## All answers
| id | question | value | band |
|---|---|---|---|
| `judge.thesis_clear` | Could a non-specialist understand `thesis` after reading it once? | 0.32 | unsettled |
| `judge.meta_essential` | Does a step in `golden_path` require Meta's model to work? | 0.77 | yes |
| `judge.proof_checkable` | Does `golden_path` let a judge check evidence themselves rather than take the presenter's word? | 0.94 | yes |
| `judge.concept_overload` | Does `pitch_concepts` contain more ideas than a judge can follow in a three-minute demo? | 0.97 | yes |
| `judge.memorable_moment` | Does `golden_path` contain a moment a judge would likely remember the next day? | 0.75 | yes |
| `judge.overall_appeal` | How compelling is this project to hackathon judges, based on `thesis` and `golden_path`? | 1.39 | level 1.39 |
| `feas.path_mostly_proven` | Are most steps in `golden_path` already listed in `proven`? | 0.12 | no |
| `feas.path_depends_on_open_issue` | Does any step in `golden_path` depend on an item in `open_issues`? | 0.94 | yes |
| `feas.meta_blocker` | Is wiring Meta's model into the system still unresolved according to `open_issues`? | 0.95 | yes |
| `feas.tool_calling_risk` | Do `open_issues` show a risk that the model will not make the tool calls the demo needs? | 0.97 | yes |
| `feas.platform_unstable` | Do `open_issues` describe an unstable platform for a live demo? | 0.97 | yes |
| `feas.fallback_ready` | Does `fallback` describe a fallback that is ready to use today? | 0.02 | no |
| `feas.scope_fit` | How well does the work in `planned` fit ten days for one person with an AI coding agent? | 1.98 | level 1.98 |
| `sec.demo_overprivileged` | Does `demo_identity` hold more authority than a public demo needs? | 0.98 | yes |
| `sec.dedicated_identity_planned` | Is a separate low-privilege identity for the demo listed in `planned`? | 0.99 | yes |
| `sec.receipts_proven` | Is producing a receipt for each operation listed in `proven`? | 0.62 | unsettled |
| `sec.anchoring_proven` | Is on-chain anchoring listed in `proven`? | 0.16 | no |
| `sec.egress_isolated` | Does `proven` state that the identity cannot reach the backend over the network? | 0.97 | yes |
| `biz.first_customer_named` | Does `business.first_customer` name a specific customer? | 0.03 | no |
| `biz.revenue_stated` | Does `business.revenue_model` describe how the business makes money? | 0.03 | no |
| `biz.single_product` | Do the items in `business.parts` describe one product rather than several separate businesses? | 0.94 | yes |
| `biz.wedge_specific` | Does `business.wedge` describe a specific kind of customer with a specific need? | 0.97 | yes |
| `biz.web3_may_deter_enterprise` | Would describing the product with a Web3 trust layer make some small-business or enterprise buyers less interested? | 0.85 | yes |
