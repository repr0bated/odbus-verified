# Declarative JEV judgments

All calls used `judge(state, questions)` on already-read evidence; no model routing was performed.

## J1 — Cargo author attribution

**Evidence supplied:** Exact `crates/op-xray-daemon/Cargo.toml` line 6: `authors = ["3tched Contributors"]`; package is `op-xray-daemon`, a D-Bus Xray lifecycle service. Supplied repository `AGENTS.md` naming distinction says filesystem is `3tched-fs` (package `tched-fs`, plugin `tched_fs`) and external executable/surface is `threetched`, but gives no author-organization authority. Network operations documentation states Xray is not required/active as an inner tunnel; this does not establish metadata ownership. Audit is not a blanket rename and requires specific correction evidence.

**Questions and answers:**
1. Meaning: choose among generic repository contributors, specific product author, wrong component label, insufficient. Result `repo_contributors`, confidence 0.63, probabilities: repo_contributors 0.72; product_author 0.04; wrong_component 0.01; insufficient 0.23.
2. Wording: choose whether exact phrase is accurate, correction evidenced, or insufficient. Result `insufficient`, confidence 0.94, probabilities: accurate 0.04; correct 0; insufficient 0.96.
3. Disposition: retain, correct, or insufficient. Result `retain`, confidence 0.43, probabilities: retain 0.62; correct 0.02; insufficient 0.36.

**Decision:** Retain provisionally. JEV weakly favors generic contributor attribution but cannot establish exact author identity; no grounded replacement exists. No speculative edit.

## J2 — coverage and necessary change

**Evidence supplied:** The complete 29-file inventory and report coverage ledger; each listed file was read in full sequential raw ranges through listed EOF. One substring is recorded above and judged in J1. No source/runtime checks; assignment prohibits build/test and runtime/deployment actions. A prior J2 evidence summary mistakenly called the inventory's proto count 17; it is 20. The complete 29-row ledger was used, so this was a prose count error, not an unread-file gap. Supplemental J3 reassessed coverage with the correct count.

**Questions and answers:**
1. Coverage: `complete`, confidence 0.94 (complete probability 0.96; partial 0.03; contradiction 0.01).
2. Tched-specific correction: `no_correction`, confidence 0.95 (no_correction 0.97; correction 0; insufficient 0.03).
3. Does static review establish runtime/catalog/deployment? Boolean 0.02 (does not establish).

**Decision:** Static audit complete; no crate source correction warranted. Runtime/catalog/deployment remain unverified.
## J3 — corrected inventory type count

**Evidence supplied:** Corrected exact inventory composition: 29 files total, including 20 `.proto` files; exact 29-row full-read coverage ledger; J1 disposition of the sole occurrence. The prior J2 summary incorrectly stated 17 `.proto` files, though its 29-file count and full-ledger evidence were correct.

**Questions and answers:**
1. Coverage with correction: `complete`, confidence 0.92 (complete probability 0.95; partial 0.05; contradicted 0).
2. Source change: `no_change`, confidence 0.85 (no_change 0.90; change 0.02; unresolved 0.08).
3. Does this establish runtime/deployment? Boolean 0.02 (does not establish).

**Decision:** The count-summary typo does not invalidate complete coverage; the exact ledger establishes all 29 files were reviewed. No source edit is supported; runtime/deployment remain unverified.