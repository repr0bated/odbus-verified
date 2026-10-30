# Declarative JEV receipts

## JEV-0 — scoped door comment judgment

An initial standalone call supplied the `door/mod.rs:19` comment, local socket declaration and owner naming distinction, and asked whether the phrase accurately distinguishes the filesystem client from the executable; whether to retain/correct/mark insufficient; and to score deployment/identity-boundary risk. It returned `supports` (confidence 1.0), `retain` (confidence 0.43; probabilities retain 0.62/correct 0.27/insufficient 0.11), and risk score 1.88/3 (confidence 0.32; probabilities 0:0.08, 1:0.24, 2:0.40, 3:0.28). The low-confidence disposition and score were not treated as a decision. No deployment claim was inferred. The later joint call included all three exact source contexts and offered explicit insufficient/correction outcomes.

## JEV-1 — all three occurrences


Inputs:
1. `door/mod.rs:19`: “Same path the bridge binds and the in-container 3tchedFS client consumes,” with `DEFAULT_LUNGS_SOCKET=/run/opdbus/lungs/lungs.sock`, module role, and owner guidance distinguishing filesystem 3tchedFS from executable threetched.
2. `controller.rs:365`: “Must be re-installed after any delete-all so unmatched host traffic does not depend solely on async packet_in”; `build_flow_mod_normal` installs wildcard `actions=NORMAL` fallback.
3. `rtnetlink.rs:941`: `route selection matched {} routes; specify table, gateway, and interface`; selector gathers routes and requires exactly one before deletion.

Questions and outputs:
- `meaning`: classify literal meanings, separating product reference from incidental English terms. **supported**, confidence 0.94 (probabilities: supported 0.96, contradicted 0.03, insufficient 0.01).
- `comment`: judge whether “in-container 3tchedFS client” accurately identifies the shared-socket consumer without asserting deployment. **retain**, confidence 0.76 (probabilities: retain 0.84, correct 0.10, insufficient 0.06).
- `incidental`: judge local wording for “unmatched” and “matched.” **retain**, confidence 0.92 (probabilities: retain 0.95, correct 0.04, insufficient 0.01).

Question schema explicitly allowed a competing correction or insufficient-evidence outcome. Human disposition: retain all three based on local implementation and owner naming authority. The lower-confidence initial scoped wording judgment (0.43) and risk score (1.88/3, confidence 0.32) were non-dispositive; the broader call supplied actual local code contexts. No desired-answer retries were made.

JEV call transcript returned:
`{'meaning': {'choice': 'supported', 'confidence': 0.94}, 'comment': {'choice': 'retain', 'confidence': 0.76}, 'incidental': {'choice': 'retain', 'confidence': 0.92}}`.
Supplemental reconciliation: coordinator receipt v5 identified `controller_native_tests.rs` and `rovs_proxy_native_tests.rs` as wholly unread in the initial pass. Both were read fully in this continuation (coverage rows identify the supplemental ranges). Neither contains a case-insensitive `tched` occurrence; this is established by full-read source coverage, not a new JEV lexical judgment. Thus JEV-0/JEV-1 remain applicable to the same three recorded occurrences, with no affected disposition. All 28 mandatory preload files were also reread supplementally; see `preload-receipt.md`. Initial receipt gaps are not backdated.
## JEV-2 — reconciled static audit completion

Evidence included the corrected full-read coverage: all 42 inventory files now read, the two coordinator-identified gaps read supplementally through EOF, all 28 mandatory preload paths read supplementally through displayed EOF, three prior occurrences with JEV-0/JEV-1 dispositions unchanged, and no unresolved source fix. Runtime/build/test checks were expressly excluded.

Question: “Does the evidence support marking this crate's static tched audit complete now, or is evidence still missing? Assess source and preload coverage separately from prohibited runtime/build claims.”

Output: **complete**, confidence 0.92 (probabilities: complete 0.95, partial 0.05, insufficient 0). This judges static audit completion only; it does not establish runtime, deployment, or build/test results.