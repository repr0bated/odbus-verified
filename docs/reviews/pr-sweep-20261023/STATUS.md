# PR sweep — pr-sweep-20261023

Agent: /root (Codex), with review_application, review_core and review_deployment.
Updated: 2026-10-23 23:10 UTC. Owner request: “take care of all pr”; explicit scope: resolve findings and merge ready changes.

## Outcome and scope

All 37 originally open PRs were reviewed. Eight PRs were merged, one superseded PR was closed, and 28 remain open with unresolved source, dependency or policy findings. This is not a claim that every finding is fixed or that the recovery stack is ready for main.

| PR | Outcome | Durable merge receipt |
| --- | --- | --- |
| #7 | Repaired and merged to main | 8435d5890ba5585c55b55ebb55828a7b66ba0ddf |
| #24 | Repaired and merged into #23 | 3cd5b089ec3c9b580e5c680946d40c8eae8fea7f |
| #25 | Repaired and merged into #23 | cc48abd6e39969d51d915c4e49652afaafb7b901 |
| #29 | Repaired and merged into #28 | 832817740d2362c4896469efb88716cc9a6e4f0e |
| #39 | Repaired and merged into #38 | 6be4671e7c2accf9399afc28316074065f03c4ef |
| #40 | Repaired and merged into #38 | 0984dcf2f7d7a761207c72a6f57585b64376409c |
| #41 | Repaired and merged into #40, then #38 | fd274c6de958d4ba4be06706b0b4f8896d607946 |
| #44 | Merged into #43 | 8d1665aae529aa01ba9c64c1cfc006a4c31b9e52 |
| #6 | Closed as superseded; head already contained in recovery86f03d2 | Not merged to main |

Successors were retargeted to the surviving predecessor branches: #26→#23, #30→#28, #42→#38. Stack merges are source integration, not deployment and not whole-stack admission to main. #10 remains the overlapping original recovery PR; three credential helpers were repaired there in fa2147b.

Six merge-forward publication rounds preserved history without force pushes. Final stack tip b3d156215930bd5000a458c6b2c155704c98bcca matches reviewed integration3c297ad485af2a8700754f84424b885bd7acb9b2 except the three credential-helper fixes intentionally published only on #10. The final cognitive and vendor trees were compared directly against the built integration before their merges.

## Changes and evidence

Published repairs include schema canonicalization and fallible publication, shared verified native-schema capture, limited-role grants, Incus effective-inventory semantics, workflow attribution/history, provider DTO and mutation validation, bounded native execution, CLI bus/signature consistency, default-deny legacy shell behavior, cognitive retry/privacy/index freshness, atomic checkpoint publication and parent-access preflight, relay half-close/budget/graceful socket cleanup, notebook progress/frozen-plan isolation, validator regex/pointer/draft/cycle/traversal corrections, and pinned dev-source closure.

The exact kernel patch input is now tracked: SHA256 a684f8c19b92a73e1dec7f88563413daf0799e2ff3957b1de494de3d953d91a2. It already existed locally but was hidden by the blanket *.patch ignore. The earlier “no local input” inference is superseded; the missing committed input was real. Only its exact checked-ioctl ENOENT semantics are recognized by the userspace recovery decoder. No kernel application/build/boot or running-ABI proof occurred.

Review dispositions were posted on the applicable PR heads. Third-through-sixth follow-up processing included 55 explicit resolutions, six partial/open replies and 26 already-resolved skips, plus the final checkpoint-preflight resolution. Earlier batches have separate receipts in the detailed reports. Partial fixes and lower-layer dependencies remain open. Automated review state is not deterministic test evidence.

## Build ledger and limitations

All Cargo/compiler execution used jeremy UID1000, one Cargo process at a time, the existing shared target, stable1.99.1 with matching RUSTC/RUSTDOC, and x86_64-unknown-linux-gnu. No workspace-wide command was run.

| Source | Scoped build | Result |
| --- | --- | --- |
| Initial repairs | op-blob library, opblob binary, jsonschema library, op-chat-responder | Passed |
| 66ceefa | op-cozo-store, op-plugins, op-grpc-bridge, op-cognitive, threetched-fs | Passed, 18m44s |
| 9b98f0b | op-cognitive, threetched-fs, op-web, op-llm, op-mcp, op-deployment, op-network | Passed, 16m34s |
| 9b98f0b | op-state-store carrier with jsonschema resolve-file/resolve-async | Passed after locked dependency fetch |
| 7eab333 | Combined source | Failed on borrowed annotation lifetime; fixed in fdff630 |
| 7eab333 | Excluded op-uds-relay binary | Passed |
| fdff630 | op-cozo-store, op-plugins, op-grpc-bridge, op-cognitive, op-dbus-cli, op-web, op-agents | Passed, 16m09s |
| 02e3f4b | Combined source | Failed on Qdrant schema-map representation; fixed via existing exact-schema annotation in2cc653d |
| 2cc653d | Eight selected packages | Failed101: host global OOM killed bridge rustc after13m02s |
| 0465337 | op-cognitive including binaries | Passed, 2m21s |
| 0465337 | op-state-store library + jsonschema resolve-file/resolve-async | Passed, 6.71s |
| 0465337 | Excluded op-uds-relay binary | Passed, 0.46s |
| 3c297ad | Final op-cognitive including binaries | Passed, 8.31s, 23:07:40UTC |

[Build receipts](build-receipts/) preserve exact source, command, identity, environment and result. Earlier failed configuration/cache attempts remain recorded. Logs remain under /var/tmp/odbus-pr-sweep-20261023/.

The bridge failure is confirmed by /var/log/kern.log:17434–17435: global_oom killed rustc PID17597 UID1000 at2026-10-23T19:02:27.840390-04:00, matching the receipt. Swap was exhausted. A concurrent rust-analyzer used about6.6GiB; no unrelated process or host limit was changed. dmesg denied EPERM; no escalation or alternate-identity bypass was attempted. The failed combined build remains failed; smaller successes do not turn it into a pass.

Capture binary/manifest hashes are in every recent receipt. The verified pair's branding patch hash d950152b857ee1068d1919de1318beb8705f91d1113b804ff06f6458ccd3c0eb differs from this stack's pin6e35f93c3064a547e01735ea61a5f52302df40a92447dcca520f6b31cacc9491. Compilation is not release provenance.

Tests, Clippy, formatting checks, /verify and smoke fixtures were not run under the owner's explicit manual/on-request rule. Added source cases were not executed; test-target compilation is not claimed. Edited shell syntax received bounded sh -n/bash -n checks. Nothing was installed, sealed, deployed or restarted. Existing replica text/retired vector points were not scrubbed; no live data or permission changes occurred.

## Remaining PRs

The following is a concise blocker index, not a replacement for every inline finding. [Remaining review links](remaining-review-links.json) identifies the unresolved paths and exact GitHub discussions; findings can change after this snapshot.

| PR | Remaining source/dependency/policy work |
| --- | --- |
| [#10](https://github.com/repr0bated/odbus-verified/pull/10) | Keep open pending reconciliation of archived/generated material, the reviewed stack and current release/identity policy. Do not merge the duplicate recovery payload over repaired branches. |
| [#11](https://github.com/repr0bated/odbus-verified/pull/11) | Authored protobuf-key restoration is in later bridge layers; integration remains required. Publication I/O is not a filesystem transaction. |
| [#12](https://github.com/repr0bated/odbus-verified/pull/12) | Remaining blockers include protected-argument audit projection and Snowball transfer/retention concurrency. Later layers do not establish independent readiness of this layer. |
| [#13](https://github.com/repr0bated/odbus-verified/pull/13) | The migration helper still needs reconciliation with scoped store ownership. History visits O(history) metadata; no performance claim is made. |
| [#14](https://github.com/repr0bated/odbus-verified/pull/14) | Observer source_identity is excluded from native writes but is not yet computed. Other plugin/state authority findings and later-layer dispatch dependencies remain open. |
| [#15](https://github.com/repr0bated/odbus-verified/pull/15) | Remaining source/authority findings and later-layer service dependencies are tracked in the inline review. No runtime provider state has been changed. |
| [#16](https://github.com/repr0bated/odbus-verified/pull/16) | Native execution is bounded, but direct subprocess control-plane policy remains unresolved. Retained skill execution validates its configured owner. Later-layer runner/source dependencies remain. |
| [#17](https://github.com/repr0bated/odbus-verified/pull/17) | StartWorkflow's sealed input constraints and other plugin-layer dependency findings remain unresolved. |
| [#18](https://github.com/repr0bated/odbus-verified/pull/18) | Direct orchestration admission and UI proxy routing findings remain unresolved. Dependent source closure is separate from runtime admission proof. |
| [#19](https://github.com/repr0bated/odbus-verified/pull/19) | Unsafe grants-reader and credential-replacement concurrency findings remain, together with current provisioning/storage policy reconciliation. |
| [#20](https://github.com/repr0bated/odbus-verified/pull/20) | Same-ID schema drift and detached-work/permit lifetime findings remain unresolved. |
| [#21](https://github.com/repr0bated/odbus-verified/pull/21) | The cognitive proto inventory depends on the relocated input in later PR #25. Combined build closure does not make this intermediate layer independently complete. |
| [#22](https://github.com/repr0bated/odbus-verified/pull/22) | Mutable file/directory inode incarnation remains unresolved. Identity-local stdio MCP needs explicit reconciliation with current public-door policy; it is not evidence of a second public listener. |
| [#23](https://github.com/repr0bated/odbus-verified/pull/23) | The admitted ProcessPendingTurn receiver, authenticated MQTT path and bounded delivery contract remain incomplete. Host runit layout authority is also unresolved. |
| [#26](https://github.com/repr0bated/odbus-verified/pull/26) | The cognitive D-Bus adapter still needs admitted-sender bootstrap and canonical tool-list metadata handling. No sender or capability checks were bypassed. |
| [#27](https://github.com/repr0bated/odbus-verified/pull/27) | The checked kernel lifetime ABI now has its exact source input in later PR #37; live ABI provisioning remains unresolved. Dormant door transport and release readiness contracts remain open. CLI bus reuse does not establish sender bootstrap. |
| [#28](https://github.com/repr0bated/odbus-verified/pull/28) | Legacy task/SSE fixture and retirement findings remain. Relay compilation does not establish release membership, deployment or an idle-timeout policy. Static service configuration belongs to later layers. |
| [#30](https://github.com/repr0bated/odbus-verified/pull/30) | Provider registry/key-installer staging, guest mount session/catalog binding, duplicate 8090 audit ownership and later-layer closure remain unresolved. Historical network suggestions are not deployment authority. |
| [#31](https://github.com/repr0bated/odbus-verified/pull/31) | Storage/GPT lifecycle, boot marker and lungs-socket readiness contracts still need reconciliation. No host service, package or storage operation was performed. |
| [#32](https://github.com/repr0bated/odbus-verified/pull/32) | Root capture/journal writes beneath user-controlled paths, fresh journal ownership, OVS reseeding and retired OpenCode contracts remain unresolved. |
| [#33](https://github.com/repr0bated/odbus-verified/pull/33) | Model-route provisioning and old FUSE/cutover naming or retirement still require reconciliation. No live service action or Python synchronization ran. |
| [#34](https://github.com/repr0bated/odbus-verified/pull/34) | Current retired-transport policy conflicts with these ownership/socket assumptions. Keep open pending explicit source reconciliation; no transport was reactivated. |
| [#35](https://github.com/repr0bated/odbus-verified/pull/35) | Interface/rule ownership, rollback boundaries and listener/egress admission findings remain unresolved against current network policy. No live network changes were made. |
| [#36](https://github.com/repr0bated/odbus-verified/pull/36) | Credential pairing, XHTTP/REALITY validation, rule ownership, resolver inputs and retirement decisions remain unresolved. Fixtures were not executed and no network state was changed. |
| [#37](https://github.com/repr0bated/odbus-verified/pull/37) | Golden concurrency, artifact/source and catalog verification, destination transitions and superseded activation contracts remain unresolved. Current deploy.sh/direct-overlay policy remains authoritative. Tracked kernel source does not prove a running compatible ABI. |
| [#38](https://github.com/repr0bated/odbus-verified/pull/38) | WebSocket SID1 scope, mounted tool names, registry reuse and stateless prompt handling remain open. The available capture binary/manifest pair has a different branding-patch hash from this stack's source pin; successful capture compilation is not release provenance. |
| [#42](https://github.com/repr0bated/odbus-verified/pull/42) | Historical principal-group/grant writes, lifecycle cleanup, PAM/EFI inputs, binary/mount naming and fixture dependencies remain unresolved against current owner policy. |
| [#43](https://github.com/repr0bated/odbus-verified/pull/43) | Privileged path containment and managed-chunk collection across every retained plan/receipt/journal reference remain unresolved. Installer/source-export and grant-policy findings also remain open; no data was deleted. |

The owner clarification about Artix /etc/runit/sv instructions versus the read-only observed Devuan /etc/sv host remains unanswered. No path migration was guessed. Current dirty recovery source also contains newer deployment/identity work; it was not swept into the review stack. The remaining PRs require coherent reconciliation with those current contracts, not automatic restoration of historical behavior.

## Shared memory and next action

- GitHub is authoritative for published heads, discussions and merge state. Retrieve/update through the GitHub connector. /root coordinates publication.
- Original checkout remains at86f03d2dede738e2d780ab789dd5800b58678479 with pre-existing dirty work preserved. Only new task reports were intentionally written there. Do not reset, stage or overwrite its unrelated files.
- Final integration: fix/pr-sweep-third-integration-20261023 at3c297ad in b942. Clean build worktree1c42 also at3c297ad; compiler slot released. Agent worktrees8b31,29f8,5ce9 retain their reviewed commits.
- NotebookLM context was attempted through the mounted identity. notebook_list completed; scoped query outcome_unknown operation b982e0d2-9b18-4c18-a9da-aee404db685f was not retried or bypassed. Present facts were checked against source/GitHub/runtime receipts.
- ECC runtime was unavailable; these repository reports are the inspectable shared task record. Reports are historical evidence, not runtime authority. No secrets are stored here.
- Source-review detail: [core](core-review.md), [application](application-review.md), [deployment](deployment-review.md). Later dated corrections in each report supersede earlier pending conclusions.

Next action: reconcile the remaining per-PR findings with current identity/deployment/retirement authority, beginning with the unresolved host-layout decision and source dependencies. Preserve the three failed-build distinctions and capture provenance limitation. Any bridge rebuild needs a resource plan based on the recorded OOM evidence, not an unchanged automatic retry. Continue merging only source-ready deltas with their applicable build evidence. Live deployment remains outside this task.

## Self-evaluation

| Axis | Score | Evidence / improvement |
| --- | --- | --- |
| Accuracy | 4/5 | Exact heads/merge/build receipts recorded; earlier hidden-patch search error explicitly corrected. Keep source and deployment claims separate. |
| Completeness | 2/5 | All37 PRs reviewed, but28 remain open with substantive unresolved findings. The whole recovery stack is not complete. |
| Clarity | 4/5 | Per-PR blocker index and build ledger are current; detailed agent reports retain lengthy historical updates. Start with this status page. |
| Actionability | 4/5 | Fixes pushed, ready layers merged, exact unresolved discussion links retained; authority decisions and a resource-safe bridge build remain. |
| Conciseness | 3/5 | Large inherited stack required repeated rounds and lengthy audit artifacts. Consolidated status avoids requiring readers to reconstruct the chronology. |

Overall3.4/5. Highest-impact improvements are resolving the remaining authority/source conflicts, reducing integration memory pressure without disturbing unrelated work, and running owner-requested verification only if explicitly assigned. A reader should see substantial completed PR maintenance and the remaining work, not an “all clear.”
