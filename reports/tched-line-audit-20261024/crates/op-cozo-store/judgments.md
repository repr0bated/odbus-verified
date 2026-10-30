# Declarative JEV receipts

Task: tched-line-audit-20261024/op-cozo-store. Installed interface read at xd://eval/judge. Actual calls use functions.eval JavaScript `await judge(state, questions)`; no model routing or external client. Confidence is returned confidence, not the chosen probability. Evidence is manually transcribed contextual snippets from sequential reads, not search/extraction. Full source remains in read receipts.

## J1 — first source clusters

Successful eval: `Judging first naming clusters` (2026-10-24).
Policy: filesystem projection = 3tchedFS/3tched; external executable = threetched; preserve exact published paths/plugin IDs/package names/format tags without a migration.
Each question used choice criteria leave_as_is / replace_with_threetched / replace_with_3tched / insufficient_evidence.

| Question | Source and contextual evidence | Answer | Confidence | Decision |
| --- | --- | --- | --- | --- |
| example: disposition of qualified 3tchedFS | examples/load_crate_graph.rs:1-8: offline example; live imports use `cozo.replace_system_graph through authenticated D-Bus/3tchedFS` | leave_as_is | 0.96 | Qualified filesystem projection branding is correct. |
| paths: disposition of RELEASE_ROOTS, selectors, provenance paths and diagnostic | src/bin/inventory/mod.rs:38-44 directory ownership boundaries: helpers maps to `libexec/3tched`; `/usr/local/libexec/op-dbus/threetched` maps to `usr/bin`. :373-378 usr/bin permits only `usr/bin/threetched`, diagnostic `threetched target contains unrelated usr/bin entries`. :1670-1672 scans `libexec/3tched/`. :1707-1715 associates `usr/share/op-dbus/threetched-manifests/tched-router.json` with `usr/bin/threetched` and `usr/share/op-dbus/ui-manifests/3tched-ui.json` with dashboard assets. | leave_as_is | 0.99 | Qualified names or exact staging/provenance path contracts; no migration in scope. |
| formats: disposition of two captured-source discriminators | src/bin/inventory/mod.rs:872-895 recognizes captured-source structure, unknown/mixed stays config; matches `3tched-ui-authoring-sources/1`; :909 matches `3tched-skill-packaging-changes/1`; :934 unknown format returns false. | leave_as_is | 1 | Exact already-qualified input contracts. |
| fixtures: eight exact test path substrings | src/bin/inventory/tests.rs:476-495 staged helper resolution uses `libexec/3tched`, `libexec/3tched/emqx-prepare`, include_str `../../../../../deploy/runit/libexec-3tched/emqx-prepare`, and matching edge/node IDs. :586-597 catalog transition uses `libexec/3tched`, `libexec/3tched/opdbus-rundirs-up`, include_str `../../../../../deploy/runit/libexec-3tched/opdbus-rundirs-up`. | leave_as_is | 0.89 | Exact implementation path pins and compile-time external dependencies must stay aligned. |

Returned probabilities: example {leave_as_is:.97, replace_with_threetched:.02, replace_with_3tched:.01, insufficient_evidence:0}; paths {.99,.01,0,0}; formats {1,0,0,0}; fixtures {.92,.04,.03,.01}, same label order. No low-confidence result.

## J2 — persistent names and ordinary substring

Successful eval: `Judging persistent names and substring` (2026-10-24).
All questions used leave_as_is / replace_with_threetched / replace_with_3tched / insufficient_evidence, with explicit contrary external-brand and filesystem-brand criteria.

| Question | Context supplied | Answer | Confidence | Decision |
| --- | --- | --- | --- | --- |
| pools: disposition of pool_name | src/bin/migrate-chatbot-root.rs:98-114 printed migration JSON: device /dev/sdb2, native_root /var/lib/incus/containers/…/rootfs, :112 `"pool_name": "3tched-storage"`, :113 pool_source from incus profile show | leave_as_is | 1 | Qualified exact pool name; changing a printed binding is not a storage migration. |
| names: disposition of service instance/reservation names | identity_transactions/clean_provision.rs:177-191 validates device cozo_id, attached, Stopped container and :183 exact `3tchedChatbot` or `3tchedCtlChatbot` or numbered native name. native_creation.rs:39-43 reserves `3tchedCtlChatbot` only for identity owner and fixed logical UUID; :52-56 validates reserved record.number/name; :160-168 assigns same reserved name. identity_transactions_tests.rs:1931-1941 same service fixture at :1938; :1998-2010 replaces `3tchedChatbot` with foreign to reject rebind. | leave_as_is | 1 | Already-qualified persistent/reserved names, not executable branding; no cross-crate/persistent migration authorized. |
| word: disposition of mismatched | external_mcp_sessions.rs:189-195 reads registered human principal; principal_id inequality or revoked_at !=0 returns `"external MCP principal is revoked or mismatched"` at :194 | leave_as_is | 0.91 | Ordinary English comparison-failure word, not a product name. |

Returned probabilities in same label order: pools {1,0,0,0}; names {1,0,0,0}; word {.93,.02,0,.05}.

## J3 — final source occurrences

Successful eval: `Judging final source occurrences` (2026-10-24).
Same four contrary choice outcomes and naming policy.

| Question | Exact source context supplied | Answer | Confidence | Decision |
| --- | --- | --- | --- | --- |
| matched: three ordinary substrings | src/lib.rs:754-758 empty query result returns PolicyVerdict allow true, reason `"no deny rule matched"` at757. :1652-1668 conditional peer-session deletion must preserve a newer session mapping; :1664 `matched[peer_pubkey] := *wg_peer_sessions[peer_pubkey, session_id],` binds peer/key and session equality; :1666 `?[peer_pubkey] := matched[peer_pubkey]` feeds deletion. | leave_as_is | 0.8 | English matched and query-local matching relation are not branding. |
| fixture_pool: both pool literals | src/lib.rs:2545-2548 test identity_sled_reopen_keeps_overlay_sized_instance_json: complete overlay literal at2547 contains `"storage_pool":"3tched-storage"` and root disk `"pool":"3tched-storage"`; :2551-2566 stores/reopens document, checks exact preservation and devices count. | leave_as_is | 0.85 | Qualified pool name in persistence fixture; no storage migration. |

Returned probabilities: matched {leave_as_is:.85, replace_with_threetched:.02, replace_with_3tched:.08, insufficient_evidence:.05}; fixture_pool {.89,0,.08,.03}, same label order.

All three calls succeeded: 9 source-context choices; confidence0.8–1. No judge failures, invented endpoints or routing calls. JEV classifies already-read source context; full coverage/absence is reader-observed, not excerpt-based JEV certification.
