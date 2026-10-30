# Native contextual audit — op-dbus-cli

Task: native-line-audit-20261024. Date: 2026-10-24. Auditor: Codex crate worker.

## Outcome

Complete static source review: four inventory files, 1,492 lines, every line read sequentially through EOF. No `native` occurrence or independent native-specific provider/model/config override found. No crate source edit; no tched/threetched overlap. Directory listings of crate root and src agree with inventory; no unlisted file, nested instruction file, binary, missing entry or symlink was shown.

Coverage and supplied initial hashes: [coverage.tsv](coverage.tsv). Substantive source decisions and actual successful declarative JEV results: [judgments.md](judgments.md).

## Clarified occurrence reconciliation — 2026-10-24

The clarified scope covers every case-insensitive `native` substring, including
compound identifiers, comments, help text and embedded fixtures/scripts, not
only model routing. Reconciliation of the retained complete four-file reads
finds **zero occurrences across all these contexts**. No reading was pending;
no new source-read coverage is claimed. Current crate directory entries agree
with the original four-file inventory. The complete assignment-directory
listing contains no `coordinator-followup.md`.

[occurrences.tsv](occurrences.tsv) records a zero-occurrence disposition for
each inventory file, linked to [coverage.tsv](coverage.tsv). These are absence
records, not invented occurrences. No native wording, behavior or intended
meaning needs correction; no unresolved native change or overlap remains.

The three earlier successful JEV judgments remain valid evidence for their
specific supplied source contexts. They are not presented as per-occurrence
semantic judgments or proof that a model inspected every source line. With no
actual native occurrence, no supplemental occurrence-specific JEV call is
needed or fabricated. The no-occurrence conclusion rests on complete human
sequential coverage, as the clarified assignment permits. Generic observations
below remain unchanged and outside native-specific ownership.


## Context and preserved authority

- `Cargo.toml:1–25`: dynamic D-Bus CLI manifest; no provider/model dependency or selection configuration.
- `src/identity.rs:1–53`: delegates configured-session selection to existing op-identity resolver; rejects resolver errors or missing genesis locally. Identity fields are displayed, not minted or written here. Constant SHM path is display provenance, not a second selection store.
- `src/introspect.rs:18–32,120–218`: transport/service/path defaults and discovery of live D-Bus declarations. `DBUS_SESSION_BUS_ADDRESS` selects the transport; it does not replace model/provider configuration. No independently authored plugin method schema is installed. Parser/conversion code at 231–705 projects XML descriptions and D-Bus type signatures; existing tests at 708–732 were read, not run.
- `src/main.rs:51–80,276–467,469–559`: dispatches explicit plugin/method/property requests using discovered interface declarations. No automatic provider/model request insertion, native routing gate or startup profile materialization. String/tuple marshalling is protocol implementation, not model selection.
- `src/main.rs:561–678`: identity display, help and completion generation do not select a model/provider. Embedded completion shell text was fully read, not executed.

## Generic observations — outside native-only repair ownership

1. **Argument marshalling limitation**, `src/main.rs:346–438`: validation checks only a minimum argument count; non-string signatures are not converted, and >=3 strings serialize as `Vec<&str>` (array), despite the tuple comment. A positional multi-string D-Bus method therefore receives a different signature. JEV: generic_out_of_scope, confidence1. Source-only finding; no runtime reproduction. Do not expand native override removal into a dynamic D-Bus serializer rewrite.
2. **Discovery and invocation connection mismatch**, `src/introspect.rs:120–129` versus `src/main.rs:287–290,525–528`: discovery has an explicit `/run/opdbus/session-bus.sock` fallback, while invocation/property reads call `Connection::session()`. When the session address environment variable is absent, these paths do not visibly share the same explicit fallback. External zbus behavior was not read; this is a static consistency risk, not a reproduced failure or native override. Proposed separate repair: use the existing adapter connection policy consistently, subject to caller/admission review.
3. **Identity display is not authenticated dispatch proof**, `src/identity.rs:24–42`, `src/main.rs:276–438,561–575`: CliIdentity is used only by the display command. No registration or SID1 injection is established here. Server-side sender admission was not audited; no authorization bypass is claimed. JEV: display_only, confidence0.99. Do not bypass receiver checks or mint credentials to make this CLI work.

No generic observation is an in-scope independent native fix. No proposed tched hunk or cross-crate change.

## Verification and limitations

Performed: complete sequential source reads; directory/inventory reconciliation; three successful declarative source judgments in one JEV call. JEV is evidence, not runtime proof.

Not performed or claimed: searches; Cargo/build/check/test/lint/format; custom smoke fixtures; runtime/identity-bound operation calls; deploy/seal/persist; service/config/grant/credential changes; Git staging/commit/reset/stash. No behavioral code changes require a worker smoke run. Coordinator owns build reconciliation. Supplied hashes are pre-audit receipts, not current hashes; HEAD was not independently queried. No live/deployed correctness conclusion.

## Self-evaluation

Accuracy4/5: source claims cite full-file reads and JEV; dependency/server admission unverified and explicitly limited. Completeness5/5: all four assigned inventory files read through EOF and every finding dispositioned. Clarity4/5: generic observations separated from native result; protocol detail remains necessary. Actionability4/5: coordinator can reconcile exact reports; generic repairs require separate authorization. Conciseness4/5: judgment receipt intentionally retains exact choices/probabilities. Overall4.2/5. Highest-impact improvement belongs to separately authorized runtime/receiver verification, not this static audit. Expected owner assessment: full native audit, not a runtime CLI certification.

## Coordinator handoff

No op-dbus-cli source patch needs building from this worker. Reconcile coverage and judgments with other crate receipts. Keep generic CLI defects separate from native-specific cleanup and preserve all concurrent work.
