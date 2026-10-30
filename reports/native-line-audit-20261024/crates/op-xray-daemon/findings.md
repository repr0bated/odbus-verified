# Findings — op-xray-daemon native audit

## Native-specific findings

No native-specific defect found. All 29 inventory files were read sequentially through EOF. One case-insensitive occurrence was encountered: `proto/VENDORED.md:42`, “native account/user preflight”. It is accurate Xray protocol/account terminology and describes the HandlerService preflight that prevents reused credentials from overwriting a user's credential-map entry. The relevant implementation (`crates/op-grpc-bridge/src/xray_methods.rs:447-473`) confirms the behavior; this file is cross-crate evidence only and was not edited. The occurrence is retained in `occurrences.tsv` with three actual contextual JEV calls and their confidence.

No source edits. No cross-crate change proposal or overlap. Generic/non-native concerns, including other documentation statements, were not expanded into this assignment.

## Verification and limitations

- Static coverage: 29/29 inventory files, all recorded as contiguous `1-EOF` reads in `coverage.tsv`; nested directory listings reconcile the provided inventory.
- Occurrence accounting: one occurrence, individually judged and recorded.
- JEV: three successful declarative judge calls, nine typed outcomes; low-confidence outcomes and follow-up implementation context are preserved in `judgments.md`.
- No Cargo/build/test/lint/format/smoke/runtime/deployment/operational actions, per assignment instructions. No claim about installed or live runtime state.
- Initial inventory hashes are recorded, not recomputed. Reads were not an atomic snapshot.

## Coordinator follow-up

Closed the two exact display gaps identified by the coordinator:
`src/commander_client.rs:1-8` and `src/main.rs:1-39`. Both were read completely;
neither contains a native occurrence. No new JEV call was applicable. The
29-file coverage count and single-occurrence disposition remain unchanged.
