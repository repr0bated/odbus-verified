# Committed OpenCode continuation checkpoint

Continue in `/home/jeremy/odbus-opencode`, branch
`wip/opencode-handoff-2026-09-15`. This checkout preserves the unfinished work
from the previous `/srv/git/odbus` session. It is a WIP checkpoint, not a claim
that all plugin methods or the fresh repository meet completion criteria.

At 2026-09-15 04:48 UTC the original checkout was stashed and switched to
`main`. Stash `ac185fbf9aed578eaee1b3b6be2d0a71a6ab2233`, based on
`1b82c594acd8d17a7e1b64b1b6ba52a21e5d9e97`, was recovered in this separate
worktree without dropping the stash or changing the original checkout.
The historical source paths below refer to this recovered checkout for future
work. Do not work from the older `main` source at `/srv/git/odbus` by mistake.

Read this checkout's AGENTS.md and its four mandatory skills. Continue using
WISHLIST.md as the task board. The source below remains subject to the full
verification and all-method completion criteria; 493 passing plugin library
tests do not establish exhaustive coverage.

Pending patches and their notes are preserved under
`docs/src/architecture/verification/all-methods/pending-patches/` using their
original basenames. They have NOT been applied or validated; review and rebase
as described below. Historical and latest selected logs are committed under
`docs/src/architecture/verification/`; native binaries and private runtime
outputs remain outside Git. The fresh local repository at
`/srv/git/odbus-verified` remains the earlier checkpoint, unchanged.

The original handoff follows. This header supersedes its working-directory,
dirty-file, and temporary-document-location statements.

## Current swarm policy — owner update, 2026-09-15

Use **ten agents concurrently**, not ten assignments with only three workers
active. Select the best currently available **free** model capable of each
assignment. Verify current zero pricing and explicitly pin the worker model;
historical Luna/Sol choices and inherited defaults do not establish free access.
Keep background/helper model calls free as well. Do not silently use a paid
fallback or reduce the quality and verification requirements to fit a model.

Qualify models on task-relevant reasoning and tool use before assigning critical
work. If no available free model meets an assignment's requirements, report the
limitation rather than accepting weaker work. Confirm actual ten-worker runtime
capacity; configuration or launched processes alone do not prove ten productive
workers. Preserve disjoint source ownership and centralized Cargo verification.

The first bounded continuation wave uses explicit
`kilo/nex-agi/nex-n2.5-pro:free` routing after comparison with two other free
candidates. This is a dated selection, not a claim that it is universally best
or independently qualified for critical protocol/security design. The owner
approved retaining those decisions with the primary and making all ten workers
read-only proposal authors. Primary alone applies reviewed patches and runs
centralized verification. Recheck price, availability and capability on later runs.

# OpenCode continuation prompt — odBus, 2026-09-15

You are continuing Jeremy's implementation and verification work in `/srv/git/odbus`. This is an execution task, not a request for another assessment or plan. Finish the retained plugin methods and bring `/srv/git/odbus-verified` to the completion criteria below. Proceed without routine confirmation. Stop only for a genuine blocking dependency after completing independent work. Missing implementation is work to do, not a blocker. Do not restart the investigation from scratch or relabel partial coverage as completion.

## Mandatory first reads

Read `AGENTS.md` and these four files completely before analysis, edits, or delegation. Every subagent must do its own preload:

- `.agents/skills/grpc-expert/SKILL.md`
- `.agents/skills/grpc-protocol-expert/SKILL.md`
- `.agents/skills/json-render/SKILL.md`
- `.agents/skills/ovs-db-analysis/SKILL.md`

Then read the current OD-58 entry/verification section in `WISHLIST.md`, recent `SIGNALS.md` entries, and `docs/src/architecture/verification/all-methods/`. WISHLIST is the task board; do not invent another TODO board. Source and the user's corrections take precedence over historical documentation.

## Completion criteria

1. Inventory every method of every retained plugin from a freshly built, privately sealed, manifest-pinned catalog, and reconcile it with current plugin-owned declarations. The prior denominator is **594 methods across 66 plugins**, not the historical 43/46-method repair list. Legitimate new methods must increase the denominator; no silent omission or retirement to manufacture coverage.
2. Every method needs positive behavioral execution through its actual implementation. For mutations, assert effects and readback, failure behavior, authorization and relevant sibling preservation. Schema/argv mapping, an unconditional mock ACK, unsupported errors, ignored tests, zero selected tests, and static review are not successful method verification. Distinguish owner, native provider/kernel, and canonical authenticated dispatch evidence. Exercise real native backends in isolated fixtures where practical.
3. Repair the defects the tests expose. Preserve one schema authority, identity ownership, committed SHM authority, and exact authorization. Do not weaken tests or change a method's meaning merely to get a pass.
4. Run `/verify` equivalent against stable source: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `cargo test --workspace --all-targets --all-features`. Explicitly run applicable ignored native fixtures and record actual results. CI does not provide these guarantees.
5. Record exact commands, tests, exit statuses, source/log hashes and coverage per method. Only then admit the verified current source and all reproducible build/release inputs to `/srv/git/odbus-verified`, verify that fresh checkout, and report remaining external limitations honestly. A passing workspace suite alone does not prove every method.

## User's nonnegotiable architecture and working rules

- This server is **Artix Linux, glibc 2.44**, Rust host `x86_64-unknown-linux-gnu`. Do not assume Chimera/musl. All 109 gate122 test executables use `/lib64/ld-linux-x86-64.so.2`; evidence is `docs/src/architecture/verification/all-methods/gate122-test-interpreters.tsv`. Codex itself is musl packaged. A separate upstream Qdrant executable is musl-static; that does not make the odBus tests musl builds.
- D-Bus is the control plane. Plugins expose canonical objects, with native backend adapters behind them. PluginSchema is the sole schema authority; the sealed blob IS the plugin. No parallel repository/plugin `schemas/` directory, file-schema fallback, or registry fallback for runtime existence. `op-blob` owns sealing. Consumers use manifest-pinned SHM.
- `tched_router` scaffolds the upstream `zeroclaw` binary. Only the autonomous upstream binary/name attribution retains `zeroclaw`. The native configuration document was imported once into the plugin-owned `tched_router_contract.rs`; methods derive from it, not hand-maintained duplicate schemas. Gemma is model agnostic.
- The identity owner mints SID1/SealedId and injects/preserves it in metadata. Identity blobs are distinct from plugin blobs. Do not create another token issuer or trust caller-provided identities.
- MutationEngine commits complete state, including children, into SHM. Streams and readers consume committed SHM. Preserve siblings; durable-only changes cannot silently become stream/render state.
- WireGuard terminates at the decoy. **No WireGuard on this server.** WARP transports identity. Isolated decoy/kernel fixtures do not authorize installation on the server.
- Waypipe is deprecated. Factory platform/plugin and Blitzy work are retired; Factory began as an SSH proxy for its CLI/billing. The assistant is the singleton control-plane chatbot, not a container.
- Runit/`sudo sv` only. No systemctl/s6 or manual runsv. No live deployment, service restart or catalog reseal has been performed in this continuation. Deployment uses btrfs send/receive, not hand-copying live binaries. Xray live config belongs only at `/etc/xray/xray_config.json` inside its container and is changed through its owner.
- Rust/shell for new helpers; no new Python files. CozoDB, no new SQLite usage. Active UI is `/srv/git/vercel-json-render-ui`.
- Use a ten-task swarm with the least expensive capable model. The previous run used ten Luna workers in waves with three workers active at once; only the difficult OVSDB protocol slice was escalated to Sol. Use available OpenCode models/capacity honestly; don't invent unavailable models. Keep source ownership disjoint and centralize Cargo. Workers should prepare patches while source is frozen for builds. Stay available to the user with concise progress updates.

## Repository and evidence state

- `/srv/git/odbus` contains substantial prior work: **518 dirty/untracked paths** at handoff inspection. Preserve it. Do not reset, clean, overwrite, or assume the diff belongs only to this session. Original HEAD: `1b82c594acd8d17a7e1b64b1b6ba52a21e5d9e97`.
- `/srv/git/odbus-verified` is a clean earlier checkpoint at `8d01947e497be9ab66e7bdd15717494c60b031d1`. It was **not updated with this continuation**. Do not call it exhaustive or copy unverified current work into it.
- Historical gate122: 1,862 tests passed across 109 suites, zero failed, nine ignored; 46 tracked method repairs. These are valid historical facts, not all-method completion. Records: `docs/src/architecture/verification/methods-122/`, `docs/src/architecture/plugin-baseline-revision.json`.
- Frozen old catalog: `/var/tmp/odbus-decoy-package-proof.cLuFA9/sealed` with `.manifest.json`; catalog hash `f0715eb44e8e6a3965cb7fe7a7c129fb1fc3e3d80a40da02faee3337555cfd37`.
- Inventories: `/var/tmp/odbus-swarm-catalog-methods.{json,tsv}` and `/var/tmp/odbus-swarm-source-contracts.json`. Current OVSDB contract changed; these old hashes are not current admission evidence.
- Ten audit mappings: `docs/src/architecture/verification/all-methods/audits/` and `/var/tmp/swarm-0*-*-coverage.json`; assignments in `assignments.json`. `mapped` means a candidate test was inspected, not that it freshly passed or provides sufficient behavior.
- New offline checker: `crates/op-plugins/examples/method_coverage.rs`, with `inventory <catalog>` and `check <catalog> <claims.json>`. It rejects missing/duplicate/unknown methods, stale schema hashes, changed test source/logs, nonzero exit, and missing exact passing test names. Its three helper tests passed, but its actual inventory/check CLI still needs execution. Full unchanged-source verification remains required.
- `claims.partial.json` currently has 51 rows (json_render15, DNS20, ghostbridge4, Btrfs12), bound to an earlier log. Some source has since changed, so regenerate receipts after reruns; do not treat these as current all-method admission.

## Applied work and actual test results

- Plugin library initial new batch: **489 passed, zero failed, one ignored**, `/var/tmp/odbus-swarm-plugins-lib.log`, permanent copy `.../all-methods/logs/plugins-lib-01.log`.
- Bridge focused batch: **57 passed, one failed**, `/var/tmp/odbus-swarm-bridge-focused.log`. Failure was `freedesktop_dispatch::tests::peer_methods_cross_private_dbus_and_read_sealed_identity`: zbus returned the real machine ID, while the fixture expected an invented constant. Root corrected it to mount a neutral application interface/use built-in Peer and compare a separate native read, with the shared SHM environment lock. **Correction not rerun yet.**
- That bridge batch passed new identity_sled provision/attach/touch tests, Xray native UDS stats test, and `generation_jobs_dispatch::tests::llm_stream_facade_and_job_listing_use_private_committed_state`. They cover private committed streaming and job list/owner isolation, but the failed process is not a clean receipt.
- `/var/tmp/odbus-swarm-plugins-lib-02.log` was a compile failure from new privacy-route tests importing simd_json instead of serde_json; fixed.
- `/var/tmp/odbus-swarm-plugins-lib-03.log`: **484 passed, nine failed, three ignored**. Seven failures were new OVSDB schema losslessness failures. Sol fixed the protected monitor state to typed arrays rather than constrained values under arbitrary map keys. One persona test incorrectly expected `rust-pro` to advertise `consult`; corrected to check Rust's `check` and execute `backend-architect.consult`. One Cozo reopen failed with a retained RocksDB lock; root made RuntimeDocuments blocking tasks explicitly drop captured native handles before publishing their result and strengthened reopen ownership assertion. **These latest corrections are being rerun in lib-04; see final status below.**
- Root also added or strengthened: all15 json_render output/readback assertions; all20 DNS typed native wire-argument checks; ghostbridge4 state reads; Btrfs12 effect/readback checks; privacy_routes list/delete errors, atomic private file replacement/fsync, serial mutations and effect/publication, real checkpoint/verification tests; seven canonical project membership/context methods; Web UI render/update canonical SHM, owner isolation and sibling tests; persona get/execute test. Latest project/Web UI tests have not compiled/run in bridge yet.
- Router integration `crates/op-grpc-bridge/tests/tched_router_backing.rs`: old 82 config getter extension passed two tests in `/var/tmp/odbus-swarm-router-getters.log`. File was then extended with all unary LLM facades, remaining role wrappers, catalog/tools and committed local-state getters. **Current extended file has not run.** Target: `cargo test --locked -p op-grpc-bridge --test tched_router_backing`.

## Immediate real failures to repair

1. **Qdrant native test failed** in `/var/tmp/odbus-swarm-qdrant-native-01.log`: `invalid Qdrant snapshot description: invalid type: null, expected a string`. Inspect `SnapshotDescription.creation_time: String` in `crates/op-plugins/src/state_plugins/qdrant.rs`; actual provider returned null. Repair contract/normalization faithfully and assert actual snapshot readback, not a fabricated timestamp.
2. **Native rtnetlink test failed** in `/var/tmp/odbus-swarm-kernel-native-01.log`: at `rtnetlink.rs:992`, gateway is `"Inet(192.0.2.254)"`, expected `"192.0.2.254"`. Trace `op_network::rtnetlink::get_default_route()` and fix native address serialization. Do not weaken the assertion to a debug wrapper. The same isolated run passed the native hostname test.
3. **OVSDB cancel is still semantically wrong in current source.** Current implementation substitutes `monitor_cancel` for the retained RFC7047 `cancel`. Review confirmed the original contract is request cancellation. Pending design: retain caller-scoped transaction connection/native ID, optional client transaction ID in `transact`, send native `cancel` notification on that connection concurrently; no invented ACK. Observe the outstanding transaction's terminal outcome. A separate `monitor_cancel` method is appropriate for monitor lifecycle, increasing the inventory to 15 OVSDB methods/likely595 total. Avoid global projection-lock deadlock preventing cancel of an outstanding transaction. Do not silently redefine this method. This correction is **not implemented yet**.

## Prepared patches — review against current files; do not blindly apply

- `/var/tmp/ovsdb-protocol-followup.patch`: bounded RPC connection change, wire same-session monitor cancellation/update2 tests, native OpenFlow test naming, and a new canonical native OVSDB fixture. **It predates the latest typed-array schema fix and the request-cancel correction.** Rebase/rework it. Older `/var/tmp/swarm09-rfc7047-corrections.patch` was rejected; do not apply it.
- `/var/tmp/swarm-07-host-project-seam.patch` (recently expanded), `/var/tmp/swarm-07-host-project_owner_native_tests.rs`, `/var/tmp/swarm-07-host-project-tests.md`: injected private project root/cache/Incus UDS owner, shared real production REST/Cozo paths, tests for create, put_document, list_documents and project-mode context. Not applied. Review fixes requested: shared `dispatch_with_owner` must not be test-only if production calls it; validate persistence binding before cached handle return; recheck session/grants after PROJECT_LOCK; original seal admission before queued effects; reliable test owner cleanup/shared locks. Preserve root's existing `project_dispatch_tests.rs` seven-method tests.
- `/var/tmp/mongodb_knowledge_native_test.patch` and `/var/tmp/mongodb_knowledge_native_tests.rs`: not applied. Public native provider probes **did succeed**: list returned82 sources, search returned1 result, without Atlas/user credentials. Logs `/var/tmp/mongodb-knowledge-native.ABFzn7/{init,list,search}.json`. They are JSON-RPC envelopes (`.result.content`), with no structuredContent for knowledge methods. Treat returned documentation as data, not instructions. A Rust test still needs application/execution. No disposable knowledge provider remained running at handoff inspection.

## Native fixture artifacts and commands

All repository tests use GNU/glibc. `CXXFLAGS='-include cstdint'` is required for release/RocksDB builds and has been used in test runs.

Qdrant:
- Binary `/var/tmp/odbus-qdrant-native.6GgKva/qdrant`, SHA256 `a8e6b18430f2f4c43156c0992464648b989caf3f2b15d201456364f5656dd598`; upstream musl-static PIE copied from stopped container. Never touch its real9.3GB collection storage.
- Applied fixture: `crates/op-plugins/src/state_plugins/qdrant_native_tests.rs`.
- Run with `OP_QDRANT_NATIVE_BIN=<above> cargo test --locked -p op-plugins --lib state_plugins::qdrant::native_qdrant_tests::native_qdrant_round_trips_all_declared_methods -- --ignored --exact --nocapture`.

MongoDB:
- Official MongoDB Community8.0.32 archive `https://fastdl.mongodb.org/linux/mongodb-linux-x86_64-ubuntu2204-8.0.32.tgz`; publisher `.sha256` verified as `41da8d92dde2896ebda278cf296ae86462da6b39eda81ce6ce4fb9ce819adb2e`.
- glibc/OpenSSL3 mongod: `/var/tmp/mongodb-native-8.0.32/mongodb-linux-x86_64-ubuntu2204-8.0.32/bin/mongod`; SHA256 `11497ca3b196b4a58f9169e193b2aae1d72d7b5a7e305e8544b4fa926bbebbfa`.
- Applied but uncompiled/unrun DB11 test: `crates/op-grpc-bridge/src/mongodb_native_tests.rs`, exact name `mutation_engine::mongodb_native_tests::native_mongodb_mcp_round_trips_all_database_methods`.
- Set `OP_MONGOD_NATIVE_BIN` to above, `OP_MONGODB_MCP_PROVIDER_ENTRY=/opt/op-mcp-providers/mongodb-mcp-server-2.1.0/node_modules/mongodb-mcp-server/dist/esm/index.js`, `OP_MONGODB_MCP_PROVIDER_ROOT=/opt/op-mcp-providers/mongodb-mcp-server-2.1.0`, `OP_NODE_BIN=/usr/bin/node`; run `cargo test --locked -p op-grpc-bridge --lib <exact-name> -- --ignored --exact --nocapture`.
- Fixture uses isolated mongod/provider, temp dbpath/logs/exports, no real credentials, actual provider's driver seeding, MCP init, shared production `dispatch_mongodb_mcp_owner` mapping and PluginSchema input/output validation, bounded cleanup. Verify it in execution.

Native hostname/netlink tests (explicit root execution of built test binary avoids root-owned Cargo outputs):
- `state_plugins::full_system::tests::native_hostname_owner_effect_isolated_to_uts_namespace_and_file` — passed, dedicated UTS thread and temporary hostname file.
- `state_plugins::rtnetlink::tests::all_declared_methods_apply_and_readback_in_isolated_network_namespace` — failed as above; fresh network namespace, no host network edits.
- Last binary: `target/debug/deps/op_plugins-054a02ad934753cd`; rebuild before claiming current-source proof.

## Other gaps still to close

Use all66-plugin audits as the actual remaining checklist; these are notable gaps, not a replacement denominator:
- NotebookLM45 of47 largely still missing behavior proof. Correct CLI is `/usr/local/bin/nlm`0.11.2, not `/usr/bin/nlm`. A native notebook-list read succeeded (94notebooks); private output `/var/tmp/odbus-swarm-notebook-native-list.json` is mode600 and must not be copied into repo. Read auth is available. Use owned disposable notebooks/profiles for tests; do not delete user data or clear live auth readiness. Existing auth methods have hardcoded live readiness paths needing isolation.
- cognitive_mcp20 require actual runtime behavior, not mapper tests.
- persona.set_identity rejects; authenticate always false. Implement display-only durable overrides and verify existing identity-owner SID1 for authentication, with security review. Never invent another identity/token authority or accept caller credential maps as grants.
- workflows5 have no completed execution owner: start appends a journal; pause/resume/cancel explicitly reject. Connect real workflow execution/control, not fake state transitions.
- fail2ban.reload needs the native complete validated configuration stream; bare `["reload"]`/fake ACK is wrong. Installed native protocol is `["reload", jail_or_--all, opts, stream]`; empty stream can remove untouched jails. Design in `/var/tmp/swarm-07-host-fail2ban-reload-design.md`. SIGHUP is ignored by fail2ban-server; generic runit reload cannot prove reload.
- Canonical evidence still needed for several host/network/provider wrappers, plus native OVS/OpenFlow and current expanded router suite.

The support email/reset request is already handled; do not reopen that task. No live deployment is requested for this handoff.

## Suggested execution order

Check the final running-process/test status below first. Repair the concrete native failures, review/rebase prepared patches, and keep a stable centralized test batch. Run the expanded bridge/router/native suites, finish remaining implementations with cheap capable workers, regenerate the sealed method inventory/evidence, run the full frozen verification gate, then update and independently verify the fresh repository. Do not declare completion while any required method remains unverified.

## Final handoff checkpoint — supersedes earlier pending-run notes

- `/var/tmp/odbus-swarm-plugins-lib-04.log` completed with exit0: **493 passed, zero failed, three ignored**, 50.67s test runtime. The OVSDB typed-state schema repair, corrected persona operation test, RuntimeDocuments handle-release/reopen change, DNS assertions and privacy-route tests now pass this plugin library batch.
- No Cargo/rustc/plugin test process remained running at final inspection. All three Codex workers were interrupted for the OpenCode handoff; do not expect them to finish or apply their pending patches.
- The native Qdrant snapshot-null failure and native rtnetlink gateway-format failure remain unresolved. Hostname native effect passed in the two-test kernel batch, whose overall exit was101 because rtnetlink failed.
- The latest bridge changes, expanded router integration, native Mongo Rust test, prepared project and knowledge patches, and required OVSDB request-cancellation repair remain unverified. Overall completion criteria are **not met**. `/srv/git/odbus-verified` is unchanged and clean.
- This handoff prompt is outside the repository at `/var/tmp/odbus-opencode-handoff.md`; it is continuation context, not another canonical task board.
