# Status — complete static audit with scoped prompt correction

Task `tched-context-audit-20261024`, crate `op-chat`.

- All 54 assigned inventory entries were read sequentially through EOF. `coordinator-read-gaps.json` reports no missing-line or column-limited files. Coverage records the original pre-edit line counts; `system_prompt.rs` was 558 lines at read time and is now 557 after the targeted prompt correction.
- The original source contains 15 case-insensitive `tched` substring occurrences. All 15 are in `occurrences.tsv`; one occurrence was removed with the false assistant/container grouping, leaving 14 in the edited source. The original ledger had omitted the second `mail-3tched` occurrence in the Incus table; it was added and judged separately during follow-up.
- Initial mandatory skill preload was incomplete. All 28 mandatory paths were read fully in the supplemental follow-up, without backdating; receipts are in `supplemental-reads.md`.
- Fresh JEV receipts: initial `jdgb-15b3324ef776d1a0`, `jdgb-15b332776bf6d1a1`; follow-up `jdgb-15b334559151389c`, `jdgb-15b334be8411389f`. Duplicate completed receipt `jdgb-15b334bb7f51389e` returned the same mail-name/state judgments as `jdgb-15b334be8411389f`. Results and confidence are detailed in `judgments.md`.
- Source change: `crates/op-chat/src/system_prompt.rs`. The assistant is explicitly identified as the control-plane chatbot, not an Incus instance; the prompt directs instance discovery through identity-bound declared operations. The remaining declared `mail-3tched` name is retained with state `UNVERIFIED`. No replacement name was guessed.
- `/run/3tched-lungs.socket` remains because root owner guidance and filesystem dispatch source declare that path. The source path is not a claim that the socket is live or available.
- Verification was limited to source review and reading back the changed prompt excerpt. No searches, Cargo/build, tests, runtime, deployment, or delegation actions were run, as explicitly prohibited by the user. No behavioral or deployment status is claimed.

Remaining limitation: current `mail-3tched` existence/state, socket availability, and deployed prompt behavior are unverified without prohibited runtime evidence.
