# op-cozo-store tched naming findings

Status: complete static naming audit. All 29 inventory files / 18,952 supplied newline-counted lines fully read sequentially through EOF, including examples and inline/standalone tests. All 34 literal tched substrings leave_as_is; no replacement or crate source edit. Paths below relative to crates/op-cozo-store.

| Occurrence | Path:line | Token | Disposition | Reason / JEV |
| --- | --- | --- | --- | --- |
| 1 | examples/load_crate_graph.rs:3 | 3tchedFS | leave_as_is | Correct authenticated filesystem projection; J1.example |
| 2 | src/bin/inventory/mod.rs:41 | libexec/3tched | leave_as_is | Exact staging path; J1.paths |
| 3 | src/bin/inventory/mod.rs:43 | /usr/local/libexec/op-dbus/threetched | leave_as_is | Correct external executable target path; J1.paths |
| 4 | src/bin/inventory/mod.rs:376 | usr/bin/threetched | leave_as_is | Exact external executable whitelist; J1.paths |
| 5 | src/bin/inventory/mod.rs:377 | threetched | leave_as_is | Correct diagnostic about external executable staging; J1.paths |
| 6 | src/bin/inventory/mod.rs:894 | 3tched-ui-authoring-sources/1 | leave_as_is | Exact qualified format discriminator; J1.formats |
| 7 | src/bin/inventory/mod.rs:909 | 3tched-skill-packaging-changes/1 | leave_as_is | Exact qualified format discriminator; J1.formats |
| 8 | src/bin/inventory/mod.rs:1671 | libexec/3tched/ | leave_as_is | Exact staging selector; J1.paths |
| 9 | src/bin/inventory/mod.rs:1709 | threetched-manifests | leave_as_is | External executable provenance path; J1.paths |
| 10 | src/bin/inventory/mod.rs:1709 | tched-router.json | leave_as_is | Exact manifest filename contract; J1.paths |
| 11 | src/bin/inventory/mod.rs:1710 | usr/bin/threetched | leave_as_is | External executable provenance association; J1.paths |
| 12 | src/bin/inventory/mod.rs:1713 | 3tched-ui.json | leave_as_is | Exact UI manifest path; J1.paths |
| 13 | src/bin/inventory/tests.rs:479 | libexec/3tched | leave_as_is | Exact staging fixture; J1.fixtures |
| 14 | src/bin/inventory/tests.rs:481 | libexec/3tched/emqx-prepare | leave_as_is | Exact helper path fixture; J1.fixtures |
| 15 | src/bin/inventory/tests.rs:482 | libexec-3tched/emqx-prepare | leave_as_is | Compile-time include dependency; J1.fixtures |
| 16 | src/bin/inventory/tests.rs:491 | file:libexec/3tched/emqx-prepare | leave_as_is | Graph ID for exact helper path; J1.fixtures |
| 17 | src/bin/inventory/tests.rs:494 | file:libexec/3tched/emqx-prepare | leave_as_is | Graph ID for exact helper path; J1.fixtures |
| 18 | src/bin/inventory/tests.rs:592 | libexec/3tched | leave_as_is | Exact staging fixture; J1.fixtures |
| 19 | src/bin/inventory/tests.rs:594 | libexec/3tched/opdbus-rundirs-up | leave_as_is | Exact helper path fixture; J1.fixtures |
| 20 | src/bin/inventory/tests.rs:595 | libexec-3tched/opdbus-rundirs-up | leave_as_is | Compile-time include dependency; J1.fixtures |
| 21 | src/bin/migrate-chatbot-root.rs:112 | 3tched-storage | leave_as_is | Qualified exact Incus pool binding; J2.pools |
| 22 | src/external_mcp_sessions.rs:194 | mismatched | leave_as_is | Ordinary English comparison failure; J2.word |
| 23 | src/identity_transactions/clean_provision.rs:183 | 3tchedChatbot | leave_as_is | Exact allowed service instance name; J2.names |
| 24 | src/identity_transactions/clean_provision.rs:183 | 3tchedCtlChatbot | leave_as_is | Exact allowed service instance name; J2.names |
| 25 | src/identity_transactions/native_creation.rs:40 | 3tchedCtlChatbot | leave_as_is | Fixed identity-scoped reservation; J2.names |
| 26 | src/identity_transactions/native_creation.rs:54 | 3tchedCtlChatbot | leave_as_is | Exact reserved-name validation; J2.names |
| 27 | src/identity_transactions/native_creation.rs:164 | 3tchedCtlChatbot | leave_as_is | Reserved-name allocation; J2.names |
| 28 | src/identity_transactions_tests.rs:1938 | 3tchedChatbot | leave_as_is | Fixture matches persistent service binding; J2.names |
| 29 | src/identity_transactions_tests.rs:1999 | 3tchedChatbot | leave_as_is | Fixture deliberately rejects foreign rebinding; J2.names |
| 30 | src/lib.rs:757 | matched | leave_as_is | English comparison-result wording, not branding; J3.matched |
| 31 | src/lib.rs:1664 | matched | leave_as_is | Query-local relation for peer/session match; J3.matched |
| 32 | src/lib.rs:1666 | matched | leave_as_is | Use of same query-local relation; J3.matched |
| 33 | src/lib.rs:2547 (first) | 3tched-storage | leave_as_is | Exact qualified storage_pool in persistence fixture; J3.fixture_pool |
| 34 | src/lib.rs:2547 (second) | 3tched-storage | leave_as_is | Exact qualified root disk pool in same fixture; J3.fixture_pool |

No cross-crate hunk or concurrent native-audit overlap identified. Existing code is not certified by this naming-only review; printed migration helpers were not executed.

## Coverage and limitations

Root, src, examples, src/bin, src/bin/inventory and src/identity_transactions directory listings reconcile to all29 supplied inventory entries. Every entry was readable text; no binary, missing, symlink or unreviewed dependency inventory entry. Compile-time include_str paths in inventory tests are preserved dependencies outside this crate, not a claim to have reviewed their target contents.

Counts: inventory implementation11 + inventory tests8 + example1 + migration helper1 + external MCP ordinary word1 + clean provision2 + native creation3 + identity transaction tests2 + lib5 =34. Thirty qualified brand/path/name/format occurrences and four ordinary-word substrings (mismatched once, matched three times). No proposed cross-crate rename or native-audit overlap.

All naming decisions supported by three successful installed declarative JEV calls/nine answers, recorded in judgments.md. Hashes in coverage.tsv are supplied initial inventory hashes; no current hash or HEAD command was run. This is source-only naming disposition, not a general security/behavior audit or live deployment evidence.

No searches, extraction, shell/process/network execution, Cargo, build, tests, lint, formatting, smoke fixtures, runtime, deployment, seal, grant or credential actions. No commits/staging/reset/stash or recursive delegation. No BEFORE copies required because no source edit was made.

Next: coordinator reconciles coverage, dispositions and result.json; no op-cozo-store naming patch requires building.
