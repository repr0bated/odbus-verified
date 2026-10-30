# Declarative JEV receipts — op-snowball

Task: native-line-audit-20261024/op-snowball. Agent: Codex crate worker. Date:2026-10-24.
Installed facility instructions read at `xd://eval/judge`; successful `await judge(state, questions)` calls through JavaScript eval. No routing extension/model selection, custom endpoint, filesystem extraction, shell or network code. Source context was supplied manually from complete sequential reads. Decisions below are static semantic assessments, never runtime verification or mutation authorization.

## J1 — Judging all native contexts

Tool receipt: successful `functions.eval`, title `Judging all native contexts`; session variables `nativeEvidence`, `nativeQuestions`, `nativeAnswers`. This call supplies every one of28 occurrences with actual context; grouped questions do not stand in for unread source.

### Submitted evidence references

- A: `src/btrfs_delta.rs:1-12` full module documentation, including exact `//! Native btrfs delta primitives: generation numbers, \`find-new\` deltas, and`; implementation35-55,65-112,148-171 described with exact commands, transid advancement, parser fields and error propagation. Native denotes Btrfs-owned metadata/CLI primitives, not an in-process reinvention. Reactive-trigger prose outside the native term is not certified.
- B: `src/replication.rs:8,40,84,86,88,121,139,305,328,332,372,376,390` each exact native line supplied; helper84-99 returns inode/FSID/root/UUID/provenance/name/readonly from Btrfs ioctls. Validation101-142, symlink-safe descriptor walk160-191, receiver command214-217, SSH230-255, bounded diagnostics258-277, transfer304-426 were included in contextual excerpts/explicit implementation descriptions. `btrfs-uapi` direct dependency supplied. All thirteen exact native contexts are preserved in occurrences.tsv.
- C: `src/snowball.rs:34-40,673-733,735-853`; all three exact native docs supplied with transfer loop, triple checks/pins, error behavior and dump-vector wrapper behavior. Caller remains responsible for remote parent's existence; sender/receiver success does not establish activation or remote admission. Dump wrapper does snapshot/prune; lower prepared transfer does not.
- D: `src/snowball.rs:1586-1621,1742-1784,2319-2326`; exact complete_snapshot_set logic supplied including both native positions on1780. Distinguishes Btrfs subvolume form from historical copies, counts successful readonly verification, rejects mixed sets, and gates retention. Counter affects storage eligibility, not model/provider selection. Generic max-counter-only retention noted separately, not hidden.
- E: `src/snowball.rs:2752-2777,2889-2904,2907-2957`, all four exact native fixture/ignore-message contexts supplied along with operations/assertions and fixture destruction scope2431-2465. Tests not run. Root prerequisite explicitly framed as privilege shorthand, not a claim UID0 suffices universally.

Full submitted state and question objects are in the tool transcript at the title above. This report records the question strings and exact typed results below; source references use crate-relative `src/` paths unless otherwise noted.

### Questions, criteria, answers

`delta` — Evaluate the single Native in section A at btrfs_delta.rs:1 with exact implementation context. Is the intended filesystem-owned CLI/transaction-metadata meaning accurate enough to retain? Judge wording/semantic correctness, not merely routing scope. Do not certify the generic reactive-trigger claims.
Criteria: retain=Accurate Btrfs-owned primitive vocabulary even though accessed via CLI; no native-specific correction established. fix=Native wrongly attributes implementation or metadata ownership and needs a concrete wording correction. insufficient=Evidence does not settle that native-specific wording.
Result: retain; confidence0.73; probabilities retain0.82/fix0.17/insufficient0.01.
Decision: retain line1; CLI uses filesystem-owned transaction metadata and received UUID, no unsupported implementation-ownership claim established.

`replication` — Evaluate ALL thirteen native occurrences in B, including the helper definition and references, ioctl and sender/receiver/error/name docs. Does evidence support retaining each as Btrfs kernel/CLI/library vocabulary, with no implicit provider/config override? Evaluate actual error preservation and sender success context, not just scope.
Criteria: retain=All thirteen uses accurately denote shown Btrfs operations/metadata; no native-specific correction established. fix=At least one use has a contextual native-specific defect; identify uncertainty rather than granting blanket approval. insufficient=Evidence insufficient for retaining all thirteen.
Result: retain; confidence0.67; probabilities retain0.78/fix0.21/insufficient0.01.
Decision: retain13. ioctl helper, sender, receiver and diagnostics match the intended Btrfs meaning; bounded diagnostic truncation is explicitly disclosed.

`incremental_claim` — Evaluate specifically replication.rs:139 native send ioctl checks actual incremental relationship and receive checks its remote copy, with B evidence. Is this appropriate filesystem responsibility wording, without claiming same-parent_uuid validation or client-side remote attestation? You may use established Btrfs protocol semantics; distinguish absent remote preflight.
Criteria: retain=Appropriate Btrfs send/receive protocol responsibility wording; does not claim preflight attestation. fix=Incorrect responsibility/relationship claim needs native-specific correction. insufficient=Need actual dependency/kernel contract to settle claim.
Result: retain; confidence0.85; probabilities retain0.90/fix0.04/insufficient0.06.
Decision: retain139; neither sibling-parent prohibition nor verified remote preflight is claimed. No actual send or receive was executed.

`transfer_docs` — Evaluate all three native occurrences in C at snowball.rs:34,677,736 against actual wrappers and B sender/receiver. Distinguish complete report from activation, and remote-parent caller responsibility from a verified remote check.
Criteria: retain=All three accurately describe Btrfs triples/prepared-transfer implementation and stated caller obligations; no native-specific defect. fix=One or more native-specific transfer claim inaccurate; correction required. insufficient=Evidence insufficient.
Result: retain; confidence0.66; probabilities retain0.77/fix0.22/insufficient0.01.
Decision: retain3; these reference the actual prepared Btrfs transport and triple/caller contract. No activation/readiness assertion.

`storage_gate` — Evaluate all seven native occurrences in D, including both uses on1780. Is native accurate Btrfs-vs-copy classification and retention-ownership vocabulary? Do NOT call runtime variable descriptive-only: it affects storage eligibility. Distinguish implementation flaws unrelated to this term from native-specific defects; do not certify race-free paths.
Criteria: retain=All seven accurately denote shown Btrfs classification/ownership; gate affects storage not model/provider choice; preserve term and logic. fix=One or more native-specific classification/wording uses inaccurate and need change. insufficient=Cannot settle native meaning/correctness from supplied checks.
Result: retain; confidence0.55; probabilities retain0.70/fix0.28/insufficient0.02.
Decision: NOT accepted as conclusive (<0.6). Re-examined full checks; success requires Btrfs filesystem magic, subvolume-root inode and readonly for each of3. Split count behavior from wording in J2. No source changed on this low-confidence result.

`fixtures` — Evaluate four native occurrences in E including fixture identifiers and ignore message. Is Btrfs kernel/CLI fixture intent accurately described? Assess root-prerequisite wording independently; do not claim test pass or authorization merely from test naming.
Criteria: retain=All four legitimate Btrfs fixture uses; root note is conventional privilege-prerequisite shorthand, not universal sufficiency proof; no native-specific correction. fix=Native fixture/privilege wording materially inaccurate and should be corrected. insufficient=Cannot settle this wording/intent without additional evidence.
Result: retain; confidence0.95; probabilities retain0.97/fix0.03/insufficient0.
Decision: retain4; fixture identifiers describe Btrfs snapshot behavior. Ignore reason is not a deployment-permissions claim. No test executed/passed claim.

## J2 — Resolving low confidence storage judgment

Tool receipt: successful `functions.eval`, title `Resolving low confidence storage judgment`; session variables `storageEvidence`, `storageQuestions`, `storageAnswers`. Supersedes J1.storage_gate for disposition. Already-read exact complete_snapshot_set function1750-1784 supplied again, complete_btrfs_snapshot_set1742-1748, checks1586-1621 and retention guard2319-2326. Explicit limits: no provenance/remote existence/identity/race/general-retention approval implied. No new source extraction or runtime action.

`meaning` — What do all five native variable references (1765,1776,1780 twice,1783) mean in this exact function? Evaluate actual behavior, not cosmetic categorization.
Criteria: btrfs_count=Counts successfully verified read-only Btrfs members and enforces either zero copied form or all three Btrfs form; no model/provider selector. routing=Selects or overrides an AI model/provider/configuration. insufficient=Cannot determine meaning from this evidence.
Result: btrfs_count; confidence1; probabilities btrfs_count1/routing0/insufficient0.
Decision: retain five runtime references unchanged. Variable governs real storage representation eligibility; no model/provider gate.

`wording` — Evaluate native wording at1750 and2320 strictly in its Btrfs-vs-ordinary-copy sense, with the code checks shown. Does retaining those words accurately describe the storage representation and retention gate? Do not grant snapshot-provenance, permissions, race safety, or general retention approval.
Criteria: retain=Both native uses accurately mean Btrfs subvolume form; no native-specific wording correction established. fix=At least one native use wrongly labels non-Btrfs form or ownership admitted by this gate. insufficient=Shown code does not settle those two native wording claims.
Result: retain; confidence0.97; probabilities retain0.98/fix0.01/insufficient0.01.
Decision: retain1750 and2320. Btrfs-vs-copy meaning matches checks; generic lifecycle/retention concerns stay separate.

## J3 — Judging static audit completion evidence

Tool receipt: successful `functions.eval`, title `Judging static audit completion evidence`; session variables `completionEvidence`, `completionAnswers`. Supplied all13 filenames/exact contiguous EOF ranges,5555 observed lines, all28 native line positions, individual grouping/results, low-confidence supersession, no source patches, no coordinator follow-up, directory reconciliation, existing reports and all static-only/hash/HEAD limitations. This is an evidence-sufficiency judgment, not an independent traversal or runtime proof.

`static_disposition` — Does this evidence support marking the assigned STATIC native audit complete once final status/result receipts are written, rather than claiming general correctness, runtime success or revision-atomic proof? Treat coverage records as evidence of performed reads, not independently proven source hashes. Identify unresolved native dispositions if any; do not introduce unrelated tests or fixes as scope.
Criteria: complete_static=Every inventoried file and all28 observed native contexts accounted for, accepted contextual decisions, no unresolved native fix/overlap; explicitly limited static completion supported. partial=Unfinished assigned coverage or native-specific disposition remains apparent. insufficient=Evidence lacks enough coverage/judgment detail to decide.
Result: complete_static; confidence0.64; probabilities complete_static0.76/partial0.23/insufficient0.01.
Decision: complete the assigned static audit with explicit limitations; do not convert it into source-revision, build/test/live or broad quality approval. Three successful declarative calls/nine typed answers in total; eight accepted answers and one superseded low-confidence answer.

## Judgment boundaries

28 ledger rows map to actual submitted contexts and accepted JEV answers. No occurrences inferred from names alone; all files/docs/fixtures read. Low-confidence J1.storage_gate retained as a receipt, not hidden or represented as authoritative. All results are declarative semantic evidence, not compilation/test/runtime/deployment proof. Current HEAD and hashes unqueried; supplied hashes remain pre-audit inventory data.
