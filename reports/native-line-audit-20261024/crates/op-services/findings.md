# op-services native audit findings

Task native-line-audit-20261024/op-services; Codex crate worker; 2026-10-24.

## Outcome

Complete static native audit: 15/15 inventory files, 1,825 pre-edit lines read sequentially through EOF. Crate root, proto, src and every source child directory reconciled with inventory; no extra files, nested instructions, binary/unreadable/symlink entries or missing inventory files encountered. Eight native occurrences, all documentation identifiers; zero runtime identifiers. One native-specific finding, resolved with eight token replacements across two documents. No tched/threetched overlap or unresolved native-specific blocker.

## F1 — stale native-suffixed service CLI inventory — corrected

- `SPEC.md:22,71,72` names nonexistent `systemctl-native` source and binary target.
- `compare-op-services.md:35` (two positions), `:47` (one), `:63` (two) claims those source/bin references are Present/Implemented.
- Authority: `Cargo.toml:7-13` declares only `op-services` and `op-svctl`; complete bin directory lists only those two source files. `src/bin/op-svctl.rs:1,17,79` confirms actual service-manager client/usage name. These are not accurate current claims and are not clearly historical-only references: comparison header explicitly says current system state.
- J1 supplied every actual occurrence context, contrary and insufficient outcomes; wording correction 0.97, stale-service-CLI role 1, bounded patch 0.92. See judgments.md and occurrences.tsv.
- Applied only `systemctl-native` → `op-svctl` in all eight positions. Manifest/runtime/ABI/schema/identity/capability behavior unchanged. Documentation paths now match existing target; no compatibility alias introduced.
- BEFORE affected-hunk excerpts: `/var/tmp/native-line-audit-20261024.ILroaQ/crates/op-services/BEFORE/{SPEC.md,compare-op-services.md}.excerpts`. Explicitly excerpts, not whole-file byte copies. Full-file read receipts retained; refresh snapshots39F3/1967 matched originals before minimal patch. Edit receipts1517/26F2 show all six changed lines / eight replacements; line counts unchanged.

## Separate generic observations — not native-specific repairs

Both documentation files retain unrelated stale dinit/systemd, source inventory/dependency/count claims (`systemctl`, dbus modules, sqlx and others) contradicted by current manifest/directory. This patch is not a general documentation refresh and does not validate those remaining claims.

Production `GrpcServer::new` uses `FailClosedVerifier`; auth opens SID1 structurally then returns unavailable pending canonical shared verifier. Every RPC authorizes before manager calls. CLI supplies no SID1, so its source does not demonstrate an admitted successful service operation. No authorization bypass, service activation or deployment availability inferred.

## Verification boundaries

Evidence: complete sequential source reads, directory parity, actual declarative installed JEV result and successful exact hashline edit receipts. No searches/extraction, shell/process execution, Cargo/build/tests/lint/format/custom smoke/runtime/deploy/seal/grant/credential actions, commits/staging/reset/stash or recursive delegation. No behavioral change requiring a runtime smoke; only documentation corrected. Primary coordinator owns any required build after reconciliation. Not built, tested, installed or deployed by this worker. HEAD/current hashes not queried; inventory hashes are initial receipts; reads are not an atomic snapshot.

## Self-evaluation

Accuracy4: exact occurrence/context and authority evidence; runtime/current hash unverified, explicitly limited. Completeness4: all15 files and8 occurrences accounted for; non-atomic reads are the remaining evidence limit. Clarity4: separate coverage, occurrence and JEV ledgers add navigation overhead. Actionability4: minimal applied docs fix plus result.json; coordinator still aggregates independent crates. Conciseness4: occurrence-level evidence repeats J1 by requirement. Average4.0/5. Highest-impact improvement: coordinator reconcile current document hunks before integration; do not construe static audit as runtime proof. Expected user assessment: complete requested crate audit, bounded wording repair only.
