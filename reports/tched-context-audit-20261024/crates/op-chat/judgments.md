# Fresh contextual JEV receipts

JEV instructions read from `xd://eval/judge`. Both calls used declarative `judge_batch` with the installed `typesafe/jev-1.14.0`; no model override. Inputs were based on already-read source excerpts and included local context, project naming boundaries, alternative interpretations, and an insufficient-evidence choice. JEV is classification evidence, not runtime proof or edit authorization.

## Receipt `jdgb-15b3324ef776d1a0` — four contexts

Question: classify each exact `tched` occurrence using the excerpt and project identity distinctions: canonical/correct project naming, project-specific stale/conflated naming, incidental, or insufficient evidence; distinguish source declarations from deployed/runtime proof.

- `system_prompt.rs:31`, opening phrase `3tched AI infrastructure platform`: insufficient, confidence .48; probabilities canonical .17, stale .21, incidental .01, insufficient .61. No external authority in supplied context resolves exact branding.
- `system_prompt.rs:35`, `mail-3tched`: project-specific-stale/unsupported, confidence .48; probabilities canonical 0, stale .61, incidental 0, insufficient .39. Confidence is weak; current container state not established.
- `system_prompt.rs:249`, `/run/3tched-lungs.socket`: project-specific-stale/unsupported as current prompt claim, confidence .77; probabilities canonical 0, stale .83, incidental 0, insufficient .17. Not runtime proof.
- `python_testing_patterns.md:50`, `test_patched`: incidental, confidence .97; probabilities canonical 0, stale 0, incidental .99, insufficient .01. Ordinary Python mocking terminology.

## Receipt `jdgb-15b332776bf6d1a1` — fourteen individually contextualized occurrences

Question: classify exact substring in context as incidental ordinary word, correctly used project-specific design term, inaccurate/unsupported project-specific naming, or insufficient evidence. Inputs for each occurrence included exact surrounding proto/source text and, where relevant, the root project's distinctions among 3tchedFS, threetched and tched_router; source declarations do not prove runtime/deployment. Four proto/generated field comments were each supplied with their actual schema contexts; the three agent identifiers and memory-loop comment each had their local operation/comment context; system prompt inputs separately supplied branding/container/socket context.

- `proto/chat.proto:7` threetched: correct .76; P incidental .02, correct .82, issue .08, insufficient .08.
- `proto/chat.proto:8` threetched: correct .75; P .02, .81, .09, .08.
- `proto/chat.proto:45` threetched: correct .32; P .05, .49, .13, .33. Weak confidence.
- `proto/chat.proto:46` threetched: correct .23; P .03, .42, .18, .37. Weak confidence.
- `op_chat.chat.rs:13` threetched: correct .53; P .03, .65, .08, .24.
- `op_chat.chat.rs:16` threetched: correct .56; P .03, .67, .09, .21.
- `agent_tools.rs:183` matched: incidental .55; P .66, .26, .02, .06.
- `agent_tools.rs:184` matched: correct/project-specific .25; P .19, .44, .06, .31. Human disposition uses the actual ordinary identifier context; low JEV confidence retained.
- `agent_tools.rs:192` matched: correct/project-specific .47; P .09, .60, .03, .28. Human disposition uses ordinary set-membership context; low confidence retained.
- `memory_loop.rs:204` matched: correct/project-specific .55; P .19, .66, .06, .09. Human disposition uses ordinary English filtering description.
- `python_testing_patterns.md:50` `test_patched`: incidental .99; P incidental .99, correct 0, issue 0, insufficient .01. Included in this call.
- `system_prompt.rs:31` phrase `3tched AI infrastructure platform`: correct .20; P .04, .40, .30, .26. This weak answer conflicts with the dedicated high-insufficient contextual result above; human disposition remains conservative/insufficient, no guessed rewrite.
- `system_prompt.rs:35` `mail-3tched`: issue .51; P .04, .29, .63, .04. Dedicated call also marked this weakly stale/unsupported (.48); no current container receipt or replacement name supplied.
- `system_prompt.rs:249` `3tched-lungs`: issue .32; P .01, .26, .49, .24. Dedicated call marked unsupported/stale .77; no current runtime evidence or replacement path supplied.

Some wording-only choices used JEV's labels `correct` despite low confidence; probabilities are ordered as listed in the receipt question: incidental, correct, issue, insufficient. Human dispositions were determined from the reviewed source and owner naming constraints, not the low-confidence label alone.

## Source-level conflict and limitation

`chat_service.rs:5-11` repeats the threetched routing design statement, while lines 103-116 explicitly call actual provider/model pipeline wiring TODO and emit a stub echo. This is a design/implementation distinction; the schema annotations were retained as declarations. The audit did not run runtime or deployment checks. No JEV outcome proves current service behavior, container existence, socket availability, or deployment status.

## Coordinator follow-up reassessment and newly logged occurrence

The assigned inventory contains 15 pre-edit case-insensitive `tched` substring occurrences, not 14: the original system-prompt Incus table at line 256 has a second `mail-3tched` occurrence in addition to the capability-list occurrence at line 35. The line-256 occurrence was missing from the initial occurrence ledger. It is now recorded separately in `occurrences.tsv`; its current row is line 255 after the scoped prompt edit.

Receipt `jdgb-15b334559151389c` is the fresh three-context reassessment:

- `assistant_instance`: claim was that grouping `assistant` with Incus instances is inaccurate given owner guidance that assistant is the control-plane chatbot, not a container. Answer `correct`, confidence .74; probabilities retain .10, correct .83, insufficient .07. This supports removing the grouping, not inventing an instance name.
- `socket`: question concerned retaining `/run/3tched-lungs.socket` as a source-declared path given exact owner guidance and `dispatch.rs:307-350`, while distinguishing live availability. Answer `retain`, confidence .27; probabilities retain .51, correct .17, insufficient .32. Weak JEV result; human disposition relies on the corroborating owner/source declaration and makes no runtime claim.
- `branding`: question concerned retaining the supplied product phrase without treating it as runtime proof. Answer `retain`, confidence .64; probabilities retain .76, correct .06, insufficient .18. No contradictory project authority was found in the allowed evidence.

Receipt `jdgb-15b334be8411389f` judges the table row's exact declared identifier and current-state wording:

- `mail-name`: `insufficient`, confidence .55; probabilities supported 0, stale/conflated .34, incidental 0, insufficient .66. Keep the source-declared name but do not claim it is a current deployed instance.
- `mail-state`: changing `ERROR` / “needs repair” to `UNVERIFIED` while retaining the name is `yes`, confidence .97; probabilities yes .98, no .01, insufficient .01. The prompt now requires identity-bound declared discovery for current state.

The same two judgments were returned by duplicate receipt `jdgb-15b334bb7f51389e` after the first batch call had already started; the duplicate results match exactly. The receipt listed above remains the direct successful batch result. These judgments do not authorize operations and do not prove runtime state.

## Supplemental preload and source evidence

The initial mandatory skill preload was incomplete. All 28 mandatory paths were read completely as supplemental reads after coordinator follow-up; they are not backdated. Exact path/range receipts are in `supplemental-reads.md`. `coordinator-read-gaps.json` reports 54/54 inventory files with metadata and no missing-line or column-limited files. Related filesystem-source context confirms the socket pathname is explicitly supplied by `dispatch.rs` and the owner guidance; it does not establish service readiness.