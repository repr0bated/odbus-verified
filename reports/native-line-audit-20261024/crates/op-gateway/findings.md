# op-gateway native audit findings

Task `native-line-audit-20261024/op-gateway`; Codex crate worker; 2026-10-24.

## Outcome — static audit complete, source repair implemented

All nine original inventory files fully read: 2,116 supplied lines through EOF. Prior reads retained; reconciliation reread the entire original storage file before mutation and the security document, then accounted for all changed/added lines through edit receipts and contextual reads. No invented new full-file coverage. Root/src listings matched the inventory.

All original20 native occurrences dispositioned. Current source contains eight and updated documentation three: **11 current occurrences**, all evaluated by actual installed declarative JEV. Ledger retains all20 original records with before/current mapping and adds four new occurrences. Thirteen original occurrences removed; seven retained/corrected; one new rejection diagnostic and three new documentation occurrences. Each current line has one native occurrence. No needed native-specific correction remains unresolved in this crate. No model/provider override or tched overlap found.

## F1 — unverified native encryption path: implemented fail-closed correction

Before: `src/encrypted_storage.rs:123-184` accepted a fresh native-backend path on `btrfs subvolume create -e` subprocess success without resulting-path encryption attestation. Existing storage already failed closed. Public config/default advertised experimental support. Statistics contained unverified native labels, although their construction was unreachable after the existing capacity error.

After: `src/encrypted_storage.rs:124-143` rejects `use_native_encryption=true` as the first initialization statement, before directory creation, subprocess execution or master-key loading/generation. The unsupported native implementation and sole-use permissions helper are deleted. Explicit false still follows the unchanged mapper-name, mapper-presence, LUKS1/LUKS2 type, exact mountpoint and block-device identity verification path. No new backend, grant bypass, identity change or provisioning logic.

The public serialized field remains an actual backend request, not an alias or silent fallback. Its default remains true and therefore deliberately fails closed. The default `WireGuardAuthManager::new()` also fails closed; no new public configuration setter was introduced. This trades default usability for preserving explicit backend intent: callers must explicitly select false and arrange the verified mapper through the authorized storage owner. No field removal or external consumer migration is needed for this bounded correction.

Module/config/setup wording now describes supported verified LUKS storage and rejected legacy requests. Native statistics branches/label are removed; surviving classification follows successful LUKS verification at initialization. The existing unavailable-capacity error is unchanged; successful statistics output is NOT claimed.

Existing native regression fixture now covers both absent and already-existing storage paths, asserting rejection, unchanged base existence and no master.key. The exact-error-wording assertion was deleted, not re-pinned. Fixture updated, **not executed**. Existing explicit-false absent-LUKS-mapper fixture is retained. `SECURITY-MODEL.md:42-56` documents the actual request/default/provisioning contract.

### Earlier blocker superseded

The original report incorrectly treated public-field removal as necessary to resolve F1. Coordinator followup proposed preserving the field and rejecting the unsupported request instead. JEV R1 supports bounded_repair1; R2 truthful_explicit_rejection0.93. This smaller same-crate repair removes the hypothetical external-caller migration dependency. Historical N4 still describes only the abandoned field-deleting proposal; no external caller existence was ever asserted.

## Occurrence judgments and evidence

N1-N4 preserve actual-context assessment of every original occurrence. R3 assessed every current source occurrence at24,26,81,127,128,577,589,591: accurate_rejection_vocabulary0.92. R4 assessed all documentation occurrences45,47,49: accurate_crate_contract1. R5 supports only static_complete_only0.68, not runtime success. All questions, contrary/insufficient choices, evidence and returned probabilities are in judgments.md.

Three successful installed declarative judge calls, nine typed questions total; no routing extension/model selection. Original separate bounded-claim verification remains historical evidence, not evidence that the later patch ran.

Exact BEFORE copies under assignment directory:
- `encrypted_storage.rs.BEFORE`:22647bytes, snapshotEB66, matching pre-edit source receipt.
- `SECURITY-MODEL.md.BEFORE`:1213bytes, snapshotD499, matching pre-edit documentation receipt.

Applied source receipts: storage1A92/616lines and security99A0/56lines. Changed files only `src/encrypted_storage.rs` and `SECURITY-MODEL.md` inside this crate.

## Outside scope

SPEC.md:46-47 and compare-op-gateway.md:64 retain historical sqlx/SQLite documentation inconsistent with current manifest. No unrelated cleanup. No statement about installed btrfs/kernel encryption support or deployed exposure; the repair removes this crate's unattested capability, not a claim about every kernel.

## Verification and limitations

Static evidence: full sequential source/doc/fixture reads, exact BEFORE receipts, surgical edit receipts, explicit occurrence accounting, actual contextual JEV results. **Not built, tested, linted, formatted, smoke-run, installed or deployed.** Owner explicitly prohibited Cargo/test/check/fmt/runtime operations and custom smoke gates. No searches, arbitrary eval execution, shell/process commands, Git mutation or recursion. No live filesystem/mount/storage/grant/credential changes.

Next: coordinator reconcile source patch and final ledgers; perform only separately owner-authorized build/verification. No native source repair or external caller migration remains pending under this assignment.

## Self-evaluation

Accuracy4: source-versus-runtime boundary explicit; supported kernel/CLI/runtime behavior unverified.
Completeness4: all20 original/all11 current occurrences covered and F1 implemented; execution deliberately outside assignment.
Clarity4: superseded blocker and default failure tradeoff explicit; initial/current ledger necessarily carries two positions.
Actionability4: exact two changed files, BEFORE receipts and coordinator next action provided; build/test proof absent by authorization.
Conciseness4: full contextual JEV receipts repeat source to make individual judgments inspectable.
Overall4.0. Improvement: coordinator may obtain runtime proof only when owner authorizes it; do not turn static completion into a test/deployment claim.
