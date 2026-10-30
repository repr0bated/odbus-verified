# Native contextual audit — op-grpc-bridge

Task: native-line-audit-20261024/op-grpc-bridge. Audit of all readable source inventory entries complete; three deleted-in-worktree entries are dispositioned as unavailable. This is not a crate-wide clean bill: N1 provider/model authority and upstream runtime semantics remain unresolved.

## Coverage and method

209 readable inventory files were read sequentially through EOF; three of 212 inventory entries were individually not found and remain missing-dispositioned. `coverage.tsv` records all entries. `occurrences.tsv` now maps contextual native tokens across reviewed source files, including exact positions in generation-native, legacy-dispatch, and runtime files. `judgments.md` preserves J1–J75 and the recovered judge-batch receipts. Original hashes are inventory receipts, not current hashes.

Mandatory skills/references were independently preloaded; root AGENTS and HANDOFF/recent SIGNALS were read. No source search, extraction, arbitrary eval, recursive delegation, shell/process/network operation, Cargo/build/test/lint/format/smoke/runtime/seal/deploy/grant/credential mutation was performed. JEV was used only for declarative judgments on read source. One judge_batch await-interface discrepancy was reported; the completed batch was recovered without rerun.

Supported source edits: additive tonic TLS trust documentation (`grpc_client.rs`), verification-only logical attachment wording (`mutation_engine.rs`), and cooperative generation-task cancellation wording (`tched_router_generation_native.rs`). Existing scoped test-comment/name corrections from prior work are retained. No executable behavior changed. No tests or runtime actions were authorized or claimed.

All remaining native contexts were evaluated or preserved at their local API/backend meaning. The audit is complete for available source, but source-level evidence does not establish live deployment or prove upstream `threetched` execution semantics. N1 remains a concrete unresolved authority/caller contract; no provider/model selection hunk was changed without that authority.

## N1 — unresolved role-route selection precedence; tched overlap

Sources fully read:
- `src/chat_mode_routing.rs:29-45,48-90,130-141`.
- `src/control_plane_routing.rs:24-36,41-88,139-249`.

Observed behavior:
- ControlPlane/Accountability modes unconditionally clear explicit input.provider/input.model and select dedicated role aliases.
- Control-plane complete observes the configured control_plane agent and unconditionally overwrites request provider/model with that selection, checks admission before/after, then requires output match observed selection.
- Credential forwarding calls the canonical SID1 owner; concrete provider/model strings in this module are test fixtures, not production hard-coded model allocation.

JEV J7 trace_override .84 and J10 trace_override .65: behavior needs actual caller/admission/config-authority trace. It is **not yet proved unauthorized**. A request's provider/model fields may not be the authoritative control-plane configuration; module comments explicitly reserve that authority to operator-owned role configuration. Conversely a role label alone does not prove the selection overwrite was explicitly admitted.

Important current-source distinction: complete chat_service.rs rejects request-level overrides and submits/observes mounted chat_response turns, not the old Chat route. Complete MutationEngine now establishes a separate actual admitted Chat caller5748/5864 (below); this supersedes the previous caller-unread checkpoint. No live deployment claim.

Overlap disposition: **no hunk changed**. Affected candidate hunks are `apply_mode_route` clearing lines42-43 and `apply_native_selection` cloning lines34-35. Required caller contract before choosing a patch: establish which sealed method admits the request, whether its input explicitly permits provider/model authority, which persisted selection/role config is authoritative, and which runtime call uses that admission. If explicit caller selection is admitted, do not discard it; if role config owns selection, reject unsupported request overrides at the admitted contract rather than silently treating them as authority. This is a reconciliation requirement, not an approved patch. Preserve OpenCode/tched worker priority.

Source inventory tracing is complete. Caller/config authority remains unresolved after the actual MutationEngine caller and complete tched runtime reads: the sealed producer contract and the upstream executable semantics are not established by these local adapter sources. `ctl_plane_chatbot_dispatch.rs` has no calls to N1 helpers; its production surface is config/episode/embedding dispatch.

Generation publication/admission/stream files and the exact runtime adapter are fully read. StartGenerationInput is cloned into the durable job and forwarded to backend.spawn; the runtime constructs expected provider/model from its observed upstream selection and verifies terminal ACKs before returning. This establishes local adapter behavior, not who is authorized to choose the upstream selection or what the pinned executable does.

`grpc_server.rs` forwards verified identity and exact schema hash through `lungs_socket::invoke_mounted`; that call site is read, but the mounted implementation is outside this native occurrence audit. Legacy ChatService is mounted behind `unsupported_legacy_domain`, so its compatibility handler is not an admitted route through this builder.

## N2 — corrected additive TLS trust documentation

Fresh `grpc_client.rs:97-139` confirms unconditional native roots plus optional `ca_certificate`. Independently read complete compatible tonic0.12.3 builder/connector shows both sources added to the same root store. Workspace manifest declares tonic0.12 and crate inherits it; no tonic patch. Locked version was not independently established, and no such claim is made.

J37 docs_only_correction .97: replaced only101-105 documentation. It now states additive native roots plus optional PEM certificates, neither replacement nor exclusive pinning. Removed unverified live run-script assertion. Exact affected-hunk BEFORE excerpt, explicitly not whole file, retained in assignment directory with existing full-read receipt. Transport/credentials unchanged. No tests/runtime actions authorized or claimed.

## Preserved local contexts

`occurrences.tsv` maps each observed contextual native token, including separate positions on the same line, to its source judgment and disposition. Three unavailable inventory entries are recorded separately as missing-dispositioned, not as judged source occurrences.

- Incus native effect example: J1 low confidence .21, clarified by J1b preserve .61; native denotes the explicitly identified backend container operation. Backend recovery implementation still pending.
- EMQX natively supported protocol clients: J2 preserve .72; retained upstream protocol vocabulary.
- OVSDB native gRPC heading: J3 initially clarify .80 because RFC7047 wire transport is JSON-RPC. Full exact service context J3b optional_only .74: immediate lines explicitly define a gRPC projection via D-Bus, so no established factual error requiring churn. Preserve; handler conformance not yet reviewed.
- Agent native UI relay/auth fixture values: J4 preserve .88; refers to external agents' own HTTP/WS interfaces. Does not certify deployed backend authentication or host exposure.
- Canonical SID1 forwarding: J5 preserve .85; no minting/selection in local module. Downstream runtime still pending.
- Antigravity SDK refresh/status: J6 preserve .90; explicit configuration copied into observed state, typed status and real SDK credential refresh fixture. No billing/quota binding claim.
- Decoy network/entrypoint: J8 .29 not decisive; full decoy network/kernel/account/mTLS source and explicit isolated canonical fixtures supplemented by J14 preserve_local .95. No model/provider selection. Startup absence claim still awaits server/lib trace.
- Episode database queries/backend effects: J9 .31 not decisive; J9b jev_verify supports .92. Local native wording does not select models/providers.
- Cron local backend/readback/indeterminate-effect vocabulary: J11 preserve_local .65, full methods/observer/fixtures J12 preserve_local_upstream_pending .98. Explicit model is validated/forwarded/compared; native CronAddBody null/default and native GET/runs upstream comments still require source/capture authority. Autonomous scheduler grant-lifetime and shared transport remain pending.
- Control-plane embedding fixture vocabulary: J13 preserve .61. NativeEmbedding/NativeResponse and native variable uses denote local mock API; actual adapter-to-mock transport is not live-provider proof. No test executed.
- Audit native tonic status: J15 preserve .75; concrete `tonic::Status` contract. External-session native SID1 attribution: J16 preserve .91; actor-filtered task scope does not override cached sealed-digest SID1 resolver locally.
- FreeDesktop native explicit mutation contract: J17 preserve_local .73; opaque JSON Schema generic Set rejection preserves explicit plugin operations.
- Generation publisher/backend vocabulary: J18 preserve_local .94. Fixture names/variables/literals J19 .59/J19b supports .55 both LOW; not proof. Manual full source shows random-loopback WS simulator, injected fixture.direct/fixture-model ACK/done, canonical SID1 forwarding and private-state boundaries. Upstream production adapter remains unread.
- Generation runtime doc: J20 preserve_local_upstream_pending .99. Local transport names accurate; upstream versioned ACK/no Agent-tools-memory behavior still pending. gRPC library load balancing uses actual `Channel::balance_list`; J21 .41 LOW not decisive. Native chat large-response size assertion remains caller-pending; framing-only fixture does not establish production sizes or admission.
- Complete gRPC server native tonic error term and observation startup log: J23 preserve_local_start_pending .97; exact MutationEngine observer startup still unread.
- Clean/physical adoption and explicit record-retirement native Incus metadata/UUID/root/pool/profile/config/readback and fixtures, plus host statvfs ABI comments: J24 .92/J25 .91 preserve_local_dependencies_pending. Modules locally read backend metadata/storage and commit identity binding/retirement rather than mutate Incus storage. Complete retirement_native helper supports prior clean-adoption caller context; backend/default-project and preflight/runtime-config/recovery helpers still pending.
- Physical proof/publication/root repair contexts: J26 .85/J27 .91/J28 1 preserve_local_dependencies_pending. Complete root runtime-config/start helpers support the previous callers' local semantics: retained full config outside proven idmap/power/ready/template deltas; first mapping must match next or digest pin; observed status gated. Actual Incus7.4 If-Match/rebuild idmap/template/move cloud-init behavior remains upstream-pending. MD5 producer width likewise not independently read here. Native fixture counters/uncertainty labels simulate callbacks, not real Incus effects.

## Incidental generic observations — outside native repair scope

### G1: recovery exclusion differs from commit exclusion

`src/chat_response_state.rs:73-87,297-310,397-419,464-475`: admit/commit retain a per-principal mutex, but recover holds the mutex map rather than that principal's mutex. An already-admitted commit no longer holds the map guard, so that guard alone does not exclude it. JEV G1 generic_gap .75. Lower-level PrivateProjectionDirectory exclusion has not been reviewed here; **no reproduced race/corruption claim**, no patch.

### G2: historical documentation/example drift

`SPEC.md` and `compare-op-grpc-bridge.md` describe an old module inventory and zero binaries, contradicted by manifest/directory contents. `src/bin/dump-protos.rs:13,35` defaults to the retired dashboard path. Several examples use legacy genesis/trace headers or plaintext loopback door addresses. Generic drift, not native-specific defects; unchanged. No examples executed.

### G3: explicitly unfinished descriptor modules

`descriptor_mount.rs:109-123` returns an empty table with explicit implementation TODOs; `descriptor_router.rs:6-7,18-22,35-38` labels itself a compilation placeholder and only supports lookup. No native text in either file; generic incomplete feature outside native-only repair scope. Their actual current caller/reachability remains unreviewed; no claim they serve production traffic.

### G4: shared-root module integration unestablished

`identity_physical_root/shared.rs:199` invokes `storage::verify_shared`, absent from complete `identity_physical_root/storage.rs:1-383`; complete parent `identity_physical_root.rs:1-786` declares only storage. This may be an unintegrated source artifact rather than a compiled call path. No source/module/reachability search, no build and no repair. Do not certify it as active shared-root behavior.

## Completion record

Audit continuation complete: all 209 readable inventory entries were sequentially read through EOF; the three missing entries remain dispositioned. Every contextual native occurrence has a ledger row. Generation-native J67 and J73–J75 plus legacy-dispatch J69–J72 complete those ledgers; runtime J53 covers its local API/config/transport terms. No build, test, runtime, or deployment claim.

## Completion assessment

Accuracy4/5: source changes are documentation-only and evidence-backed; provider/model authority and upstream executable semantics remain unresolved. Completeness4/5: all readable inventory and native occurrences reviewed; three missing files cannot be inspected. Clarity4/5: local vocabulary is distinguished from unresolved authority. Actionability4/5: N1 caller/config authority trace is explicit. No further source change is supported without that evidence.

## Continued source findings

- J29-J32 preserve locally accurate Incus lifecycle, ownership, private fixture and observer vocabulary; external producer semantics remain unverified.
- J33 .95 tched overlap: `json_render_generation_tests.rs:11-23,27-103` still refers to `RuntimeSelection`, `ensure_binding_matches`, `RouterCaller`, `readmit_call`, `generation_admission` and other APIs absent from its now-complete parent. Reconcile/remove retired fixture with OpenCode's mounted response cutover; do not restore duplicate route. No independent source edit.
- Complete lib/grpc_server/lungs source shows no N1 helper calls in these modules; server entry and remaining runtime/MutationEngine callers still pending. J34b .98 preserves exact Incus name/UUID/lifetime fields and backend effect wording.
- Generic observation G5: `mcp_frontend.rs:298-317` authors external MCP session explicitly without a container, while `mcp_dbus_dispatch.rs:75` always invokes mounted guest operation. No execution claim; trace pending MutationEngine/current guest dependency. This is not itself a native wording defect.
- J35b supports .82 preserves mail backend fixture/local MCP credential/built-in protocol vocabulary. All native occurrences through memory_binding.rs mapped.

### N1 actual caller established; coordinated cutover overlap retained

Complete MutationEngine8074: dispatch_tched_router_owned Chat5725 -> resolve5748 -> route.input5759 -> ControlPlane complete5864 or route.client complete5880. Guarded admission retains identity/session/exact grant. Which input/config owns provider/model precedence still requires sealed producer/runtime; J41 tched_overlap .91, no candidate selection hunk changed. Complete project_dispatch681 restricts mode/session/memory/capabilities/agents but does not assign provider/model, so does not settle that precedence.

Concrete additional tched integration overlap: MutationEngine3844 calls json_render_generation_dispatch::dispatch absent from fully read current parent143, alongside stale generation fixturesJ33. Reconcile with OpenCode's mounted response cutover; do not restore duplicate route. No build run or compile failure claimed.

### N3 corrected native Btrfs attachment ownership comment

MutationEngine3187 claimed native Btrfs attach ownership; complete identity implementation and fresh247-268 only verify externally mounted storage, never mount/unmount/member changes. J40 docs_only_fix .68. Two comment lines corrected, exact BEFORE affected-hunk excerpt retained; executable behavior unchanged.

MutationEngine3019-3068 traces Incus/Cron/OVS observation startup once, resets flag after terminal task, no timer restart in that function. J42 preserves Incus creation/CAS/readback labels. J43 preserves exact OpenFlow backend/fixture contexts1.00; J44 preserves OVS/project/private fixture contexts.60. No fixtures executed or external backend certified.

Generic G6: complete schema_router.rs keeps non-cfg load_from_monolith438-454 and load_routes_from408-413 file fallback, with legacy monolith fixtures. No native occurrence in file; no repair in this native-only assignment. Production default constructor selects catalog directory; do not infer configured live fallback or runtime exploit.

J46/J47: complete server startup has no run_decoy call, matching complete lib/grpc_server/MutationEngine; bounded source absence only, no deployed process or repository-wide claim. Specialist306 resolves default ChatInput with conversation only and417 invokes dedicated control-plane completion; no explicit provider/model supplied on that caller, so no demonstrated discarded override there. Native backend/admission/Unix accept terms preserved; generation stateless ACK remains conditional upstream contract pending runtime source.

J49 tched catalog overlap.83: select141 bails on every mismatched family; fixture205-213 supplies custom.zen then openai.go with target.family=openai and expects openai.go success. Source contradiction, not test execution. Preserve coordinated owner priority; no binding hunk changed. OAuth-origin exception153 and claims32-36 remain actual-runtime-producer pending. J50.95 preserves exact config backend fields/adapter types and own-agent guarded explicit catalog selection; no global native CAS implied. J48.61 preserves graph declared/bootstrap-versus-native-observation distinction.

J52 identified the concrete Drop/cancellation mismatch. Applied the comment-only correction at `tched_router_generation_native.rs:139`: Drop requests cooperative cancellation and does not abort the task; J67 supports. J73–J75 preserve exact local protocol/type contexts; pinned binary task-id/stateless/no-tools-memory semantics remain unverified. Independently read the updated common instructions and coordinator follow-up; no agent was launched.

### Final test-inventory continuation

All 209 readable crate inventory entries have sequential EOF coverage; three missing entries remain individually dispositioned. J65 preserves tched router config/backend terminology in the isolated fixture; the bundled backend-wording judgment remains low confidence (.28). J66 supports clarifying the test-only phrase “native tonic client” to “tonic Rust client.” J67 supports the cooperative-cancellation comment; J68 confirms additive native roots plus optional PEM certificates in tonic TLS; J69–J72 preserve bounded legacy adapters; J73–J75 preserve generation-native local protocol/type terminology. No runtime or behavior claim follows from fixtures. Native occurrence ledgers are complete for readable content; N1 provider/model caller/config authority and upstream semantics remain unresolved. No tests/builds were run.
