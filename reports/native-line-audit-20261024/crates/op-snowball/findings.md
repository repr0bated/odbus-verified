# op-snowball native audit findings

Task `native-line-audit-20261024/op-snowball`; Codex crate worker;2026-10-24.

## Outcome

All13 inventory files read sequentially through EOF:5,555 observed source/documentation lines. Root and src directory listings reconcile the inventory; no unlisted file, nested instruction, binary, missing file, symlink or dependency entry encountered. Assignment directory checked initially and after full reads; no coordinator follow-up present.

All28 case/compound occurrences of native are listed individually in occurrences.tsv, including both positions at snowball.rs:1780. Distribution: btrfs_delta.rs1; replication.rs13; snowball.rs14. Ten other files have zero occurrences, explicitly recorded in coverage.tsv. No native in assigned filenames. Documentation, runtime symbols, diagnostics and fixtures are included.

**No native-specific defect established; retain all28 occurrences. No crate source changes. No unresolved native/tched overlap or proposed cross-crate patch.**

## Contextual decisions

| Context | Occurrences | Accepted JEV evidence | Decision |
| --- | ---: | --- | --- |
| Btrfs-owned transaction metadata via CLI |1| J1.delta retain confidence0.73 | Preserve legitimate filesystem vocabulary, not an AI backend claim. |
| Btrfs ioctls/metadata/helper references, sender/receiver/diagnostics |13| J1.replication retain0.67; incremental wording J1.incremental_claim retain0.85 | Preserve native_facts and actual ioctl/library/CLI terminology. Sender+receiver success is not remote readiness/activation proof. |
| Triple/prepared-transfer documentation |3| J1.transfer_docs retain0.66 | Matches the Btrfs transport call and caller-owned remote-parent prerequisite. |
| Btrfs-vs-copy classification variable |5| J2.meaning btrfs_count1 | This runtime counter controls real storage-form eligibility; it is not merely decorative and does not select models/providers. |
| Btrfs-vs-copy classification/retention ownership prose |2| J2.wording retain0.97 | Checks identify read-only Btrfs members rather than ordinary copied directories; preserve terminology without implying race/provenance/security proof. |
| Btrfs fixtures and ignored privileged round-trip |4| J1.fixtures retain0.95 | Names match tested intent. Root note is privilege-prerequisite shorthand, not universal UID0 sufficiency or a test result. |

J1.storage_gate retain0.55 was not accepted as conclusive. Re-examined checks and split actual count behavior from wording in J2; both resolved with confidence1/0.97. Full question criteria, actual results, confidence and source references: judgments.md.

## Separate generic observation — report only, not a native fix

`src/snowball.rs:77-96` retain_snapshots ignores `_policy` and `_now`, keeping only highest counter; `src/retention.rs:1-7` says calendar fields are compatibility-only, while public field docs16-23 describe keeping N snapshots and set_retention_policy1102-1125 persists/logs those fields. Existing source/tests3180-3199 intentionally assert send-parent-only behavior. This is a generic policy/documentation issue for separate owner reconciliation, not caused by native terminology. No retention redesign, deletion, field removal or test alteration authorized/performed here. This audit is not a general correctness or deployment approval.

## Evidence and limits

- coverage.tsv:13 inventory-ordered rows; every file has contiguous ranges through observed EOF and supplied initial SHA256.
- occurrences.tsv:28 individual dispositions; line-relative token ordinal disambiguates multiple positions.
- judgments.md:three successful installed declarative judge calls/nine typed answers; one low-confidence answer explicitly superseded; J3 static completion confidence0.64.
- Static source review only. No searches/extraction/arbitrary eval, Cargo/build/check/tests/lint/format/custom smoke/runtime/deploy/seal/grant/credential changes, staging/commit/reset/stash or recursive delegation.
- Initial inventory hashes were not recomputed. Current HEAD unqueried; reads are not an atomic snapshot. No concurrent crate modification was observed in source read receipts, but unchanged-byte identity to inventory is unverified.
- Higher-priority session instructions require own shared HANDOFF claim/handoff; other agent claims/source preserved. No other shared bookkeeping or task-board mutations.
- No source patch means no BEFORE backup needed. No runtime behavior changed; no built/tested/installed/deployed claim.

## Coordinator next action

Aggregate these13 coverage rows,28 occurrence rows and JEV receipts. No native patch from op-snowball needs a build. Any broader retention-policy work requires separate scope; any requested build/runtime checks remain coordinator-owned.

## Self-evaluation

Accuracy4/5: claims bound to read receipts and explicit JEV results; current hashes/live behavior unverified. Completeness5/5 within assigned static scope:13 files/28 occurrences, no pending disposition. Clarity4/5: detailed evidence in separate ledgers rather than one compact report. Actionability5/5: no native patch or blocker; coordinator aggregation paths and generic issue are explicit. Conciseness4/5: mandated ledgers/receipts necessarily repeat context. Overall4.4/5. Highest-impact improvement outside this static assignment: coordinator bind aggregation to an authoritative source revision. Would owner agree: expected for full-read/no-search/static-only requested scope; no runtime assurance implied.
