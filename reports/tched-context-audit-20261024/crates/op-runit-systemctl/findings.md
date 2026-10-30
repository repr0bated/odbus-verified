# Findings — op-runit-systemctl tched context audit

All five inventory entries were read sequentially through EOF: 1,129 listed lines. One case-insensitive `tched` substring occurs in `README.md:101`, “3tched Architecture.” J1 classified meaning as generic architecture attribution (0.97); wording/action were low confidence (0.59/0.25). Coordinator follow-up found one omitted mandatory-preload interval: `.agents/skills/json-render/SKILL.md:301-377`. This was read fully in a supplemental pass, not backdated. It adds json-render UI/plugin reference mentions but does not define the README phrase or establish this crate's live state. Fresh J2 kept generic attribution as the best-supported meaning (0.77), found wording accuracy unresolved (retain confidence 0.07), and found no concrete correction supported (0.61).

Human disposition: wording accuracy remains insufficient; no specific tched correction is established, so leave it unchanged provisionally rather than replace it with an unrelated component name. No source files changed.

All five source-file receipts are complete; coordinator-read-gaps.json reports no missing-line or column-limited source files, and coordinator-followup.md explicitly says no source reread is needed. The README contains unrelated legacy s6 instructions, while current crate implementation is runit-oriented; that inconsistency is outside this tched-specific task and was not changed.

No Cargo/build/check/test/Clippy/fmt/smoke/runtime/deployment operations were run; the assignment explicitly forbids them. Source review does not establish deployed/live state.
