# op-runit-systemctl native audit

Task: native-line-audit-20261024/op-runit-systemctl
Auditor: Codex crate worker
Date: 2026-10-24
Status: complete — static native audit only

Five of five inventory files read completely through EOF (1,129 lines). Root/src/src/bin directory listings reconcile all inventory entries. Exactly one native occurrence: src/main.rs16 occurrence1, describing built-in sv restart rather than emulated stop+start. J1 contextual wording judgment retains it (confidence0.89); selection not_selection (confidence1); static completion boundary0.8. Missing-live sv up branch explicitly included in judgment evidence. No native-specific source edit, blocker or tched overlap. Generic stale s6 documentation/CLI labels recorded separately, unchanged.

Reports: coverage.tsv (five complete rows and initial hashes), occurrences.tsv (one resolved row), judgments.md (actual installed declarative JEV evidence/questions/three answers), findings.md (decision, separate generic observations and limitations). Assignment result.json records coordinator handoff. Mandatory preload complete. Shared administrative HANDOFF.md bookkeeping only as required by higher-priority session rules; preserve other claims.

No searches, arbitrary eval execution, Cargo/build/check/tests/lint/format/custom smoke/runtime/deploy/seal/grant/credential/git mutations or recursive delegation performed. No crate source changed. HEAD/current hashes unqueried; inventory hashes are initial receipts. No built/tested/installed/deployed claim. Next: coordinator aggregate reports; no native patch from this crate requires a build.
