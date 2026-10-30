# Findings — completed static tched audit

Task `tched-context-audit-20261024`, crate `op-chat`. All 54 assigned inventory entries were read sequentially through EOF; coordinator reconciliation reports no missing-line or column-limited files. The pre-edit source contained 15 case-insensitive `tched` substring occurrences. All 15 are logged in `occurrences.tsv`; one occurrence in the capability-list phrase was removed by the scoped wording correction, leaving 14 in current source.

## Contextual dispositions

- `proto/chat.proto` declares threetched provider/model routing and annotates request fields as threetched-selected; generated Rust comments mirror the schema. These are design/contract statements, not evidence of running behavior. `chat_service.rs:103-116` describes routing as TODO and returns a stub echo. Retained as source-level declarations only; no runtime claim.
- Three `matched` identifiers in `agent_tools.rs`, one `matched` in `memory_loop.rs`, and `test_patched` in the generic Python mocking example are ordinary incidental substrings. JEV confidence varied for individual `matched` cases; source context establishes ordinary collection/test terminology. Retained.
- The supplied prompt branding `3tched AI infrastructure platform` is retained as source wording. The supplemental JEV result favored retaining it (.64), and no allowed project authority contradicted that exact phrase. This is not deployment evidence.
- The prompt previously grouped `assistant` with Incus containers and listed `assistant RUNNING`. Owner guidance identifies assistant as the control-plane chatbot, not an Incus container. JEV supported correcting the grouping (.74). The capability now directs instance name/state discovery through identity-bound declared operations and explicitly distinguishes the assistant chatbot. The false assistant row was removed.
- The remaining `mail-3tched` inventory row previously asserted `ERROR` and “needs repair.” No runtime check is authorized, and source inventory does not prove present state. JEV found the identifier's current status insufficiently established (.55) and supported marking its state unverified (.97). The prompt retains the declared name but now says `UNVERIFIED` and directs current state discovery through identity-bound declared operations. This does not assert that the named instance currently exists.
- The exact shared socket pathname `/run/3tched-lungs.socket`, backed by `/run/opdbus/lungs/lungs.sock`, is declared in root owner guidance and explicitly used by `crates/threetched-fs/src/dispatch.rs:307-350`. Supplemental `main.rs`, `mounted.rs`, and `dispatch.rs` context does not contradict that architectural path. The path is retained; runtime socket availability was not checked. JEV's retain answer was weak (.27), so the disposition rests on the direct owner/source evidence, not JEV.

## Scoped source change

Only `crates/op-chat/src/system_prompt.rs` changed:

- Replaced the false parenthetical grouping of `assistant` and `mail-3tched` under Incus orchestration with identity-bound declared discovery guidance and the correct chatbot distinction.
- Removed the assistant-as-container table row; retained the mail name with state `UNVERIFIED` and identity-bound discovery guidance.

No container name, socket, endpoint, capability, or replacement architecture was invented. The file was fully read before the edit (558 lines); the current file is 557 lines after replacing two table rows with one. The changed source excerpt was read back after the edit. No test/build/runtime execution was performed because the user explicitly prohibited those actions. This is static source verification only; generated prompt behavior and live deployment remain unverified.

## Coordinator follow-up and preload correction

The initial mandatory skill preload was incomplete. All 28 paths in `mandatory-preload.txt` were read completely as supplemental follow-up, not backdated. The exact path/range record is `supplemental-reads.md`. The coordinator's source-read reconciliation reports no assigned-source display gaps. The additional filesystem reads establish the declared socket wiring only, not live status. Fresh JEV receipts and reassessments are preserved in `judgments.md`; JEV is not treated as permission or runtime proof.
