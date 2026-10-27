# Declarative JEV receipts — op-grpc-bridge

Task native-line-audit-20261024/op-grpc-bridge. Source-only judgments, not runtime proof, permissions or model routing. Exact evidence and questions were submitted through installed eval judge/judge_batch; source references below identify the already-read contexts. Fixed execution model unchanged.

## Tool interface discrepancy

Read `xd://eval/judge` completely. Its documented `await judge_batch(...)` syntax raised `TypeError: object JudgmentBatch can't be used in 'await' expression`. The call had started successfully. Completion notice named `jdgb-15b323cb6cbb7bf6`; recovered its actual completed results with `judge_batch.attach(...).results()`, without resubmitting. Reported discrepancy to `xd://report_issue`. Subsequent calls use synchronous batch creation and `await batch.drain()`.

## Batch jdgb-15b323cb6cbb7bf6

Question shared: classify actual native occurrence for correctness/intended meaning using supplied exact contexts; distinguish legitimate wording from misleading semantics; no model routing/permissions. Choices preserve / clarify / runtime_defect / insufficient.

### J1 — examples/provision_container.rs:1-41

Evidence: documentation identifies `identity_sled.provision_container`, Incus creation followed by durable Cozo/protected-SHM transaction, then says `the native effect may require recovery before retrying`. Example submits explicit `wireguard_pubkey`, instance image raccoon/profiles identity through RemoteOperationClient, capability identity_sled.write.

Answer preserve, confidence **0.21**; probabilities preserve .41 / clarify .38 / runtime_defect .01 / insufficient .20. Low confidence not accepted as decisive. Supplement J1b below.

### J2 — proto/emqx_exhook_v3.proto:423-462

Evidence: Message headers map documents username/protocol/peerhost/allow_publish and says headers may be missing; MQTT/MQTT-SN/CoAP/LwM2M and other `natively supported protocol clients` carry these headers, unlike possible HTTP-API publications.

Answer preserve, confidence **0.72**; probabilities preserve .79 / clarify .20 / runtime_defect 0 / insufficient .01. Decision: native protocol-support vocabulary, preserve. Not a deployed broker guarantee certification.

### J3 — proto/operation.proto:512-540

Evidence: heading `OVSDB Mirror Service - RFC 7047 native gRPC bridge`, immediately followed by `1:1 gRPC projection` and D-Bus mirror forwarding, RFC-linked RPC declarations and explicitly non-RFC convenience DumpDb. Criteria noted RFC7047 uses JSON-RPC, not gRPC; asked if heading could imply RFC-native gRPC.

Answer clarify, confidence **0.80**; probabilities preserve .13 / clarify .85 / runtime_defect 0 / insufficient .02. Initial decision: investigate ambiguity before patch; see full-context J3b. No source edit made on this result.

## Batch jdgb-15b32408917b7bf7

Shared choices preserve / clarify / runtime_defect / insufficient, with instructions to distinguish local terminology from unverified dependencies.

### J4 — src/agent_ui_proxy.rs:1-175,233-356

Evidence submitted includes native UI/HTTP-WebSocket relay documentation; native synchronous prompt endpoints; absent-Origin comment `Native client; authentication still belongs to the server`; before-gRPC-CORS native UI challenges; preserving both native UIs' root routing; exact loopback backend addresses/host matching, opt-in flag and unmodified auth/cookies; all native fixture values: Basic realm="native", native=challenge; HttpOnly, native authentication required, native=accepted; HttpOnly, /health?native=1, native=fixture-only, /native/ws, native echo, and test names native_auth_challenge_and_credentials_are_forwarded_without_injection / sse_first_chunk_streams_before_native_completion_and_disconnect_cancels / websocket_native_auth_origin_echo_and_close_survive_relay.

Question: is terminology legitimate for backend-owned UI/auth/transport and fixtures, without certifying unexamined backend auth or production placement?

Answer preserve, confidence **0.88**; probabilities preserve .91 / clarify .05 / runtime_defect .01 / insufficient .03. Decision: retain all mapped occurrences. `native` denotes external agents' own API/auth, not platform-native widgets or a billing binding. Upstream auth and production exposure remain unverified.

### J5 — src/native_sled_identity.rs:1-58; src/agent_runs.rs:59-70,298-309,345-361,401-419

Evidence submitted: canonical SID1 forwarding docs and complete local credential algorithm: reject empty actor/session/genesis; resolve exact canonical projection; obtain mcp_sealed_id_header; decode SID1 claims; match principal/session/genesis; return encoded payload. RouterCompleter forwards via with_sealed_identity/chat_under_guard after agent_scope verification; actions dispatch under same scope with declared capability. Empty ChatInput provider/model and unreviewed downstream runtime explicitly disclosed.

Question: classify native identifier/comment/call as canonical credential forwarding versus minting or provider/model selection; no end-to-end approval of unexamined runtime.

Answer preserve, confidence **0.85**; probabilities preserve .89 / clarify .04 / runtime_defect 0 / insufficient .07. Decision: retain local vocabulary/call. Runtime routing remains pending.

### J1b — examples/provision_container.rs:1-41

Question: is `native effect` reasonably precise for the Incus creation identified immediately above rather than implicit native model routing? Wording only; backend recovery unreviewed.

Answer preserve, confidence **0.61**; probabilities preserve .71 / clarify .18 / runtime_defect 0 / insufficient .11. Manual evidence agrees: noun antecedent explicitly identifies the Incus container operation. Retain local wording; backend creation/recovery correctness not yet certified.

## Batch jdgb-15b3243269bb7bf8

Choices preserve / clarify / trace_override / runtime_defect / insufficient. Instructions explicitly require caller/config trace when authorization cannot be decided locally.

### J6 — src/antigravity_dispatch.rs:1-287; src/antigravity_canonical_tests.rs:156-343

Evidence: native provider observations/effects module doc; NativeBridgeStatus import and fallback construction from explicit runtime.provider/base_url; state copies explicit provider and selected_model; configure deserializes supplied input; refresh_token requires gemini and calls GoogleCredentialOwner.refresh with `native Google credential refresh failed`. The native_refresh fixture constructs actual google_cloud_auth SDK credentials with isolated token URI, explicitly configures provider/model, observes refresh_token grant and fixture credentials; does not claim quota/billing.

Question: judge native wording/NativeBridgeStatus/native_refresh identifiers as SDK/backend status-refresh terms versus implicit provider/model rewrite; no dependency/live certification.

Answer preserve, confidence **0.90**; probabilities preserve .92 / clarify .01 / trace_override .05 / runtime_defect 0 / insufficient .02. Decision: preserve local occurrences. Explicit configure invocation is not implicit native selection.

### J7 — src/chat_mode_routing.rs:1-142

Evidence submitted: complete dedicated_alias and apply_mode_route behavior, local comments, resolve call chain, native_sled_identity call and native_aliases test. ControlPlane/Accountability use control_plane/accountability aliases and unconditionally clear input.provider/input.model; other modes preserve them. Persisted caller mode/current grants are consulted; downstream for_agent_alias/runtime config and admission not yet reviewed. Test explicitly expects clearing caller selection.

Question: does source implement unconditional role-mode override, is native descriptive, and does policy correctness require explicit admitted-request/caller/config trace? File touches tched runtime and independent edits are not authorized there.

Answer trace_override, confidence **0.84**; probabilities preserve .04 / clarify 0 / trace_override .87 / runtime_defect .08 / insufficient .01. Decision: **unresolved overlap/context**, not proven unauthorized routing. Coordinator must reconcile with tched worker. All five source occurrences mapped to J7/J5 in occurrences.tsv.

### J8 — src/bin/op-decoy-network.rs:1-6

Evidence: `Decoy-only native network owner. No host bridge startup path invokes this.` Entrypoint only initializes tracing and calls decoy_network::run_decoy().

Question: evaluate native locally as network implementation owner, not model route; cross-module absence claim untraced.

Answer trace_override, confidence **0.29**; probabilities preserve .29 / clarify .01 / trace_override .43 / runtime_defect 0 / insufficient .27. **Not decisive**. Manual reading: shown entrypoint selects no model/provider; native network-owner terminology is descriptive. Claimed startup separation remains pending until decoy_network/server/lib review. No patch inferred.

## J3b — direct judge full-context follow-up

Source proto/operation.proto:512-546. Submitted exact full service excerpt including header, projection and D-Bus explanations, all declarations and convenience comments. Disclosed J3 .80 and competing interpretation: native gRPC may mean first-class exposed gRPC projection, not native RFC wire transport.

Question: does full context establish inaccurate native wording requiring correction, or only optional ambiguity? Judge heading WITH explanatory lines; no runtime/policy claim. Choices required_clarification / optional_only / trace_required.

Actual answer: optional_only, confidence **0.74**; probabilities required_clarification .17 / optional_only .83 / trace_required 0.

Decision: **preserve**. Immediate text explicitly defines the gRPC projection via D-Bus. Earlier J3 ambiguity is optional wording, not established factual defect; no independent churn. Handler conformance remains pending overall crate audit.

## Limits

These are successful declarative source judgments, not a complete audit. Coverage is in coverage.tsv; pending paths/contexts remain pending. No arbitrary eval execution, source extraction, traversal, search, routing extension, Cargo, tests, runtime or deployment operations.

## Batch jdgb-15b32530a2fb7bf9 — episode and incidental synchronization

### J9 — src/chatbot_episode_state.rs:25-85,94-110,316-338,427-468

Evidence: exact native queries/archive and native effect comments; full distinct-database path/alias validation; EpisodeEffect identity scope/seal/owned lock; verified admission and retained durable/publication task.

Question: judge both occurrences independently as database query/archive interface and backend work across owned admission/receipt; any misleading wording or hidden selection locally? Choices preserve / generic_gap / native_defect / insufficient.

Answer preserve **0.31**; probabilities preserve .48 / generic_gap .14 / native_defect .11 / insufficient .27. Mixed rubric produced low confidence; not decisive. Supplement J9b below.

### G1 — src/chat_response_state.rs:73-87,297-310,397-419,464-475

Evidence submitted verbatim in structural form: operation is an Arc mutex map to per-owner Arc mutexes. admit obtains map guard only to clone/create owner mutex, drops map guard and takes owner guard. commit_reporting retains only owner guard. recover holds the map guard across recovery, without taking existing owner guard.

Question: does holding map mutex in recover exclude an already admitted commit holding only owner mutex? Generic issue outside native edit scope; no runtime reproduction.

Answer generic_gap **0.75**; probabilities preserve .13 / generic_gap .81 / native_defect .02 / insufficient .03. Decision: generic report-only synchronization concern. PrivateProjectionDirectory may provide lower-level exclusion; dependency not reviewed. Do not claim reproduced corruption.

### J9b — successful jev_verify supplement

Claim: in these local comments native queries describes the generic Cozo query/archive interface; native effect describes backend work retained across owned permit; neither occurrence specifies model/provider selection. Does not certify unexamined callers/query implementations.

Evidence: exact comments at25-26/94-100; complete local EpisodeEffect fields and session/lock/preflight/pin/commit sequence; no model/provider fields or routing in completely read module.

Actual result: verdict **supports**, confidence **0.92**, misleading **0.4/3**, verbatim **0.10** (no quote requested), tokens791/78. Decision preserve both local occurrences.

## Batch jdgb-15b3255deb7b7bfa — control-plane and scheduler

Shared question: evaluate local native meaning and concrete behavior; distinguish vocabulary from policy correctness. Choices preserve_local / trace_override / clarify / runtime_defect / insufficient.

### J10 — src/control_plane_routing.rs:1-250

Submitted actual contexts for module doc, NativeGenerationAdmission/Error references, native_alias/native_agent receipts, every native selection/error comment and helper call, fixture type/route names, unexpected native property error and configured native answer literals.

Evidence: complete builds for_agent_alias(control_plane) with sealed identity, checks admission, reads current_selection, apply_native_selection unconditionally overwrites input provider/model, checks admission again, executes guarded completion or chat and requires output equal observed selection. Tests assert arbitrary configured providers supersede supplied overrides; native Anthropic fixture expects SID1 acknowledgment.

Question: are local terms descriptive external-agent contracts, while authorization of overwriting selection requires caller/config trace? Tched overlap, no independent edits.

Actual answer trace_override **0.65**; probabilities preserve_local .24 / trace_override .72 / clarify .01 / runtime_defect .02 / insufficient .01. Decision unresolved overlap/context. No proven hard-coded production provider/model: concrete model strings are fixtures. Caller/config authority remains unreviewed.

### J11 — src/cron_dispatch.rs:1-159; src/cron_gateway.rs:1-247

Submitted all native contexts: configured native agent's risk/tool policy; seal changed during native operation; complete native Cron readback/observation publication; native gateway; four native cron indeterminate completion/error strings. Evidence explicitly describes install-global autonomous scheduler not retaining/rechecking caller OP-DBUS grants, method readmission with exact actor/cap/session/genesis, CRON_OPERATION exclusion, complete readback, post-seal validation, shared runtime client and typed method routing. CronMethods, observer and runtime not yet read.

Question: do local native terms describe backend scheduler/risk owner/readback/effect transport rather than implicit model routing, without certifying autonomous scheduler admission or transport dependencies?

Actual answer preserve_local **0.65**; probabilities preserve_local .72 / trace_override .01 / clarify .02 / runtime_defect .24 / insufficient .01. Decision preserve local wording; scheduler policy and external transport review remain pending. No assumption that native vocabulary itself grants permission.

## Checkpoint status evidence check

Successful jev_verify claim: this audit remains partial, no completed-crate acceptance, no crate patch, unresolved role selection precedence pending caller/config trace. Evidence: 212 inventory paths; rows2-47 minus missing34 plus early row121 =46 fully read; three independent missing reads;163 remaining readable paths pending; J7/J10 actual trace_override results; report/HANDOFF-only mutations and partial result receipt.

Actual result: **supports**, confidence **0.98**, misleading **0.1/3**, verbatim **0.07** (no quote requested), tokens735/78. This checks honest checkpoint reporting, not unseen source or runtime correctness.

## J12 — continued cron group after checkpoint

Sources completely read: cron_gateway_tests.rs:1-198, cron_methods.rs:1-747, cron_methods_tests.rs:1-428, cron_observer.rs:1-321.

Question: evaluate all local native terms/identifiers/fixtures as descriptive external scheduler API/transport/uncertain-effect terms versus inaccurate claims/implicit model selection; explicitly leave unreviewed upstream behavioral claims pending.

Evidence submitted: NativeFixture/native_gateway exact fixture identifiers; native CronAddBody null-to-None/true-default comment; native adapter/completion/owner and GET/runs documentation; exact cross-job native run response diagnostic; test native run may still be executing; observer's complete event-driven readback algorithm and every native event/frame/chunk/encoding/JSON/job-id/subscription/publication/gap/closure/reconnect diagnostic and output-exclusion/queued-response/wakeup comment. Explicit model validation/forwarding/create_matches were supplied. No local forced model, but upstream null-default/history comments and autonomous scheduler authorization remain unverified.

Actual direct judge answer: **preserve_local_upstream_pending**, confidence **0.98**, probabilities preserve_local_upstream_pending .99 / local_native_defect .01 / insufficient 0. Decision preserve local terms; two upstream-contract comment occurrences remain context-pending. No source patch.

## J13 — control-plane chatbot embedding fixture

Complete source read ctl_plane_chatbot_dispatch.rs:1-1598. Submitted every native occurrence with actual line and identifier/context: NativeResponse/NativeEmbedding struct/type/Drop/impl/channel/construction; native fixture request assertion; canonical_vectorize_executes_native_embedding_and_rechecks_both_grants; native binding/endpoint/request/respond/assert_no_request uses and real native call comment.

Evidence: fixture's isolated loopback POST /api/embed server captures HTTP body and returns queued mock responses; explicit test-only provider/endpoint/model/dimension. Production facade derives nested exact capability, admits sealed target args, checks both grants/session, calls canonical embedding target, validates dimensions and commits scoped result. The real native call comment refers to actual Ollama adapter HTTP transport to the mock API, not actual live external-provider service; no tests executed. Fully read production has no native occurrences and does not call N1 route helpers.

Question: evaluate every native fixture identifier/comment/use in this context; distinguish actual adapter-to-mock path from live-provider proof. Choices preserve / clarify / selection_defect / insufficient.

Actual answer **preserve**, confidence **0.61**; probabilities preserve .71 / clarify .28 / selection_defect 0 / insufficient .01. Decision retain fixture vocabulary with explicit mock-provider limit; no source edit.

## J14 — decoy native network source and fixtures

Sources fully read decoy_network.rs:1-921, decoy_canonical_tests.rs:1-896, prior entrypoint op-decoy-network.rs:1-6. Submitted actual contexts for all native D-Bus/kernel/account documentation, DecoyNative/DecoyNativeService/NativeNetworkReceipt symbols, native object path, complete native-owner output/state/credential/schema error strings, native effects/readback lifetime comment, all OP_DECOY_NATIVE_* required transport variables and repeated fixture settings, native listener/peer/receipt diagnostics, and malformed_native_state fixture name.

Evidence: exact SID1 owner forwarding after caller readmission; private transport CA and pinned bridge leaf before D-Bus service admission; local schema/capability/args/receipt ownership/secret/exact-state validation; no model selection; run_decoy explicitly requires decoy role and interface allowlist before kernel/account owner construction. Canonical fixture docs explicitly limit effects to in-memory kernel and loopback Cloudflare mock. Startup absence claim still requires unreviewed server/lib/caller trace, not established by this module alone.

Question: accurate native network/transport/backend ownership vocabulary versus misleading semantics or native-specific selection/identity bypass; do not certify unexamined dependencies/startup.

Actual answer **preserve_local**, confidence **0.95**; probabilities preserve_local .96 / clarify .03 / native_policy_defect 0 / insufficient .01. Decision preserve local native terms/contracts. Supplements J8's low-confidence local-meaning result; cross-module startup separation remains context-pending.

## Batch jdgb-15b3278ff27b7bfd — audit and external-session terms

### J15 — event_chain_access.rs:1-374

Actual native contexts:107 concrete native tonic status at check_contract_and_grant returning Result<(),Status>;174 Native tonic error contract comment at replay_after returning Result<(u64,Vec<ChainEvent>),Status>. Exact grant/sealed Snowball validation and OutOfRange/ResourceExhausted stream status behavior supplied.

Question: are both occurrences correct concrete-library error vocabulary, not routing/config gates? Choices preserve / clarify / native_defect / insufficient.

Actual answer preserve **0.75**; probabilities preserve .81 / clarify .14 / native_defect .01 / insufficient .04. Decision preserve both.

### J16 — external_mcp_session.rs:1-491

Supplied actual native caller/SID1 resolver comments and every native test name/binding/literal/use. with_authoring_scope only installs a task-local actor-filtered secondary scope for cached external sessions without sealed digest after connected-session/current grants/registry checks. SID1-digest contexts keep no secondary override. Fixture distinguishes two external origins from canonical native SID1 context and missing-cache case.

Question: accurate native SID1 versus external-session secondary-attribution distinction locally, without certifying architecture or unreviewed engine resolver? Same choices.

Actual answer preserve **0.91**; probabilities preserve .93 / clarify .04 / native_defect 0 / insufficient .02. Decision preserve all mapped local terms/fixture uses; no model/provider selection here.

## Batch jdgb-15b327e301bb7bfe — explicit mutation and generation boundaries

J17 source freedesktop_dispatch.rs:1-1375, sole native at408: JSON Schema opaque fields require explicit native mutation contract, not generic whole-property Set. Supplied field protection, admitted target method input/capability/seal validation. Question: accurate native plugin contract vocabulary with explicit methods retained? Choices preserve_local / trace_override / native_defect / clarify / insufficient. Actual preserve_local **.73**; probabilities .78/.01/.04/.04/.13. Preserve wording; no native-specific fix.

J18 source generation_admission.rs:1-190; generation_jobs_dispatch.rs:1-1187; generation_jobs_selection.rs:1-126; generation_jobs_stream.rs:1-353. Supplied all native adapter types/error codes/events/queue/session/gate/credential symbols and comments, exact current identity/grant/catalog admission, unchanged input passed to backend, random backend session, durable-before-SHM-before-wake, safe withdrawal/error mapping and local-only cancellation limit. No N1 helper calls in these files. Question: supported local backend vocabulary without proved provider/model overwrite; adapter/runtime still pending? Same choices. Actual preserve_local **.94**; probabilities .95/.04/0/0/.01. Preserve local terms, actual backend selection remains unreviewed.

## Batch jdgb-15b3282ff27b7bff — generation fixtures and runtime adapter

J19 source generation_jobs_tests.rs:1-1262; supplied NativeFixture random loopback WS implementation, handshake/SID1/ACK values, native variables/PRIVATE_NATIVE_PROMPT literals/helper call and every native test name. Facade/private-state/caller-drop/cancel/disconnect/revocation/recovery/lag/queued-grant-or-seal/burst contexts supplied; no tests executed. Question: accurate isolated backend fixture language without live-provider or unauthorized model selection claim? Choices preserve_local / preserve_local_upstream_pending / clarify / native_defect / insufficient. Actual preserve_local **.59 LOW**; probabilities .67/.28/.02/.01/.01. Not decisive.

J19b actual jev_verify supplemental claim: native/NativeFixture denote isolated backend WS simulator and do not establish live-provider verification or production model selection; preserve terms while production adapter pending. Evidence exact fixture fields, random 127.0.0.1:0 bind, accept_hdr_async, fixture.direct/fixture-model injected ACK/done, native prompt/SID1 strings, private prompt/public-negative check and native usages. Actual supports **.55 LOW**, misleading .4/3, verbatim .08; tokens887/78. Both weak receipts retained, not elevated to proof. Manual full source supports local naming: explicit private fixture server, not live provider; upstream behavior/adapter still pending.

J20 source generation_runtime.rs:1-24; supplied Native transport adapter doc including upstream one-selection/versioned ACK/no Agent-tools-memory assertion and unchanged input/native_session/gate forwarding with NativeGenerationAdmission/Task. Question: local transport naming accurate while upstream behavior claim unverified? Same choices. Actual preserve_local_upstream_pending **.99**; probabilities0/.99/0/0/.01. Preserve local naming, do not certify upstream claim before adapter/runtime read.

## Batch jdgb-15b3289140fb7c00 — gRPC client terminology

J21 source grpc_client.rs:1-1150, native chat response-size comment27 and Native Tonic Load Balancing158 with Channel::balance_list; 64MiB plugin/state limit and 5MiB framing-only fixture supplied. Question: locally accurate backend/balancer vocabulary, no auth/production-size verification from fixture? Choices preserve_local / clarify_docs / dependency_pending / native_runtime_defect / insufficient. Actual preserve_local **.41 LOW**; probabilities .53/.13/.29/.01/.04. Not decisive. Actual Channel::balance_list supports library name manually; production chat size assertion remains dependency/caller-pending.

J22 source grpc_client.rs:97-140, exact doc says trust anchors in priority order: optional OP_DBUS_GRPC_CA_FILE PEM to pin, then Native system roots (`with_native_roots`). Actual configure_endpoint initializes with_native_roots unconditionally, then optional ca_certificate; no native-roots disabling branch. Dependency implementation unread. Question: materially misleading priority/pin-only docs established, or dependency contract needed? Same choices. Actual dependency_pending **.36 LOW**; probabilities .01/.47/.49/0/.03. Neither a transport defect nor definitive doc defect concluded. N2 tentative doc clarification: describe actual native-root builder + optional CA addition, not unverified priority/exclusive pin; defer until dependency contract established. No source edit.

## J23 — complete gRPC server source

Evidence grpc_server.rs:1-6308. All two native contexts supplied:1447 warning after MutationEngine::start failure (observer implementation pending),2416 concrete tonic Status stream-error comment at state_snapshot_frames. Exact sealed grant/schema -> lungs_socket admitted dispatch preserved; no N1 helper or run_decoy calls in server; legacy ChatService mounted behind unsupported_legacy_domain. Question: local tonic wording accurate/start label coherent, exact observers engine-pending versus clarify/native_override/insufficient? Actual preserve_local_start_pending **.97**; probabilities .98/.02/0/0. Decision preserve tonic term and retain observation-start context pending. First eval failed before submission because direct judge does not accept intent; corrected only failed call without that argument. No fabricated first-call result.

## J24 — clean adoption/partitioned/storage native vocabulary

Complete evidence identity_clean_provision.rs:1-738, partitioned.rs:1-181, storage.rs:1-369. Supplied all native Root/UUID/name/proof/parsed metadata/previous native record/ETag/readback/security/profile/idmap/pool diagnostics and symbols; preflight/retirement/start helper calls with definitions pending. native-id fixture label compares binding only. native statvfs comments describe host libc ABI; partitioned code verifies already-prepared readonly overlay, no mount/member addition. Exact actor/session/grant/seal/revision adoption evidence preserved; no provider/model selection.

Question: accurate Incus backend or kernel ABI native vocabulary, without external-preparation/deployment/unreviewed-helper correctness claim? Choices preserve_local_dependencies_pending / clarify / native_defect / insufficient. Actual preserve_local_dependencies_pending **.92**; probabilities .94/.04/.01/.01. Preserve local symbols, comments, errors, fixtures; helper authority pending.

## J25 — configured chatbot retirement and physical root adoption

Complete evidence identity_delete_provision.rs:1-232 and identity_physical_root.rs:1-786. All native comments, identifiers, diagnostics and fixtures supplied: exact stopped Incus name/type/status/UUID/ETag, complete default-project absence proof and repeated metadata readback before identity-row deletion; physical canonical Incus root/UUID/profile/devices/config/pool proof, adopted external storage and retained identity/genesis/history; guard/power readback and UUID/layout drift fixtures. Unreviewed native preflight/runtime-config/storage/recovery helpers explicitly pending. No provider/model selection, no local Incus mutation.

Question: every native use accurate Incus/backend provenance locally, while helper authority stays pending? Choices preserve_local_dependencies_pending / clarify / native_defect / insufficient. Actual preserve_local_dependencies_pending **.91**; probabilities .93/.04/.02/.01. Preserve local native vocabulary, not a dependency/runtime certification. J24 retirement_native caller context now supported by complete local helper source.

## Batch jdgb-15b32a80a57b7c01 — physical proof, identity publication and repair

J26 full shared.rs:1-234/storage.rs:1-383 native proof/NativeSharedRootProof/verify_native/identity metadata/root/idmap/native variables and diagnostics supplied. Readonly Incus GET and digest-pinned current runtime UUID; human/runtime durable revision and exact child checked separately; storage verifies externally mounted Btrfs root/device/inode/loop read-only. Shared module integration not established: calls storage::verify_shared absent from complete displayed storage; parent displayed only storage inclusion. Question: local native terms accurate without integration/kernel/runtime certification? Choices preserve_local_dependencies_pending / clarify / native_defect / insufficient. Actual preserve_local_dependencies_pending **.85**; probabilities .89/.04/.01/.06. Preserve local terms; generic integration observation pending actual caller.

J27 full identity_publication.rs:1-407/tests:1-486 actual native digest/lifecycle comments and fixture AtomicUsize native binding/uses, native-completed label and native outcome uncertain injected error supplied. Current last_hash bytes passed unchanged (16|32 accepted), actual MD5 producer unread. Owned task waits publication lock then durable callback/SHM; fixtures simulate callback effect, not real Incus. Question: accurate local meanings without producer width/live backend certification? Same choices. Actual preserve_local_dependencies_pending **.91**; probabilities .93/.02/.01/.03. Preserve local terms and fixture labels.

J28 full identity_root_repair.rs:1-2590 all native Incus backend trait/implementation/parameters/fields/variables/receipt/preflight/runtime-config/start/status/readback/error/phase/fixture families supplied, one-shot owned-session SID1/grant/seal/revision/private digest-plan admission, existing mounted storage proof, before-each-effect checks, ambiguity journal and post-effect comparison. Explicit native-moved-identity-uncommitted phase never claims success. Native CAS comment expressly limits ETag to drift check. Actual Incus7.4 If-Match, rebuild idmap/template and move cloud-init version behavior remain upstream-pending. Question: accurate native effect/provenance terminology without inferred AI-routing defect or dependency proof? Same choices. Actual preserve_local_dependencies_pending **1**; probabilities1/0/0/0. Preserve local terms; upstream implementation/behavior pending. Per-occurrence ledger for this file still pending at this checkpoint, not a complete occurrence accounting claim.

J28 occurrence follow-through: complete observed root repair native occurrence ledger now persisted through last observed token2385. Historical ledger-pending note above records ordering only; current coverage row is read-complete, not a complete-crate or upstream behavior claim.

## Partial checkpoint grounded check — 87-file prefix

Actual jev_verify claim: partial static audit87 readable fully read /3 missing /122 pending; no source patch/build/test/deploy; next unread identity_seed_provision.rs:1; N1/N2 unresolved. Evidence: inventory rows2-88 less missing34 gives86, early full121 adds1;212-87-3=122; missing176/210 individually dispositioned. Current full-source ranges, occurrence ledger through root repair2385, actual J26-J28 receipts, unchanged source authority boundaries, explicit partial status/result and pending N1/N2 supplied.

Actual verdict supports **.89**; misleading .4/3, verbatim .08, tokens904/78. This checks truthful partial reporting/accounting, not unseen-source absence or complete audit coverage.

## J29 — continued seed provision source

Read complete identity_seed_provision.rs:1-400;401-800;801-1245. Supplied native inputs/readback/config-error/instance-match/backend field/call/intent/retired gate/committed-power/fixture contexts. Valid digest forwards clean external adoption; invalid fails validation; compose_leaf refuses independently before old execution. Native terms mean Incus plan/metadata/effects or isolated comparisons, not AI routing. Incus-added metadata comment remains upstream-dependent.

Question: all native terms locally accurate while old creation continuation disabled and upstream behavior unproven? Choices preserve_local_upstream_pending / clarify / native_defect / insufficient. Actual preserve_local_upstream_pending **.99**; probabilities .99/.01/0/0. Preserve local vocabulary and exact backend identifiers. Updated common instructions and own coordinator-followup.md read; N2 additive trust evidence awaits independent dependency/version reading.

## J30–J32 — continued lifecycle and observer contexts

Actual declarative batch `jdgb-15b32c9a08411b20`, completed 3/3, typesafe/jev-1.14.0.
Question: evaluate wording and runtime meaning of every grouped native context; preserve locally with dependency limits, clarify wording, identify independent native defect, or insufficient evidence.

- J30 `identity_sled_dispatch.rs:1-5043`, complete sequential reads. Evidence: existing Incus identity binding185-224; name/session/key/UUID checks421-423; seal/publication preflight631-637; storage verifier264; owned start/stop1223-1475; bound session lookup1486-1507; creation delegation2676-2704; isolated lifecycle/error fixtures2882-4689. No model/provider selection. Answer `preserve_local_dependencies_pending`, confidence .99, probabilities preserve .99 / clarify .01 / defect 0 / insufficient 0. Preserve accurate Incus terminology. Creation helper and backend semantics remain dependency work.
- J31 `identity_sled_private_fixture_tests.rs:1-667`, complete sequential reads. Evidence: isolated private namespace, NativeDrift69/248/328, Incus metadata306-308/389-397/618-635; historical provisioning cases and root repair explicitly ignored. Question includes stale gate wording as generic concern, not native semantic defect. Answer `preserve_local_dependencies_pending`, confidence .87, probabilities .90/.06/0/.04. Preserve terminology; fixture assertions and ignored tests are not execution proof.
- J32 `incus_observer.rs:1-299`, complete sequential reads. Evidence: Incus snapshots and wakeups1/79/103/117/133; host lock152; bound ownership lookup/recheck184/194/196; authoritative full readback validated before MutationEngine commit. Answer `preserve_local_dependencies_pending`, confidence .80, probabilities .85/.07/0/.08. Preserve local wording, external producer unverified.

## J33/J34/J34b — generation fixture and mounted identity terminology

Batch `jdgb-15b32d0f87c11b21` completed 2/2. J33 actual `record_tched_overlap`, confidence .95, probabilities preserve .01 / overlap .96 / independent fix .01 / insufficient .02. Complete parent143 and child103 lines show the native catalog pair fixture references absent `RuntimeSelection`, `ensure_binding_matches`, `RouterCaller`, `readmit_call`, `generation_admission` and other old generation API. Its native pair means exact provider/model matching, but current parent only validates mounted scoped response arguments and assembles prompts. Concrete overlap: reconcile or remove retired child tests with OpenCode's tched response/generation cutover; do not restore an independent route or weaken admission. No execution/compilation claimed.

J34 broad summary of fully read facade/lib/lungs files returned **insufficient**, confidence .21 (probabilities .34/.21/.04/.41); not used as a decisive verdict.

J34b narrowed to exact code: `lungs_socket.rs:203-227,425-431` obtains Incus backend metadata, UUID, init PID, and compares bound identity; `lungs_ingress.rs:627-632` sends only opaque ticket to exact bound Incus name; `large_language_model_dispatch.rs:69-85,109-115` spawned canonical target survives caller cancellation through readback/publication; `lib.rs:71,90,137` module declarations. Asked only whether native denotes backend binding/effects rather than implicit selection, not backend certification. Actual `accurate_backend_vocabulary`, confidence .98, probabilities .99/.01/0 (accurate/misleading/insufficient). Preserve local terms and identical fixture fields; external producer behavior remains unverified.

## J35–J37 — mail, MCP and additive TLS trust

Actual batch `jdgb-15b32d7370411b22`, completed 3/3. J35 mail fixture contexts returned preserve_with_limits confidence .43 (probabilities .57/0/.01/.42); J36 MCP contexts returned preserve_with_limits confidence .35 (.51/.04/.02/.43). Both LOW, not decisive.

J35b actual `jev_verify` supplemental exact context: temporary NativeFixture/NativePaths and mail owner files, nested tched_router config admission with same actor/session/grants, MCP local authoritative_session_identity genesis comparison versus external origin branch, successful SID1 ping-only receiver metadata, and native-named built-in protocol fixtures using injected test auth/backend. Claim: native means backend/local identity/protocol handling, not selection; fixtures not runtime proof. Actual **supports, confidence .82**, misleading .5/3, verbatim .07 (no quote claimed). Preserve local terminology, no backend/live certification.

J37 exact current `grpc_client.rs:97-139` plus complete tonic0.12.3 builder132 and connector101 lines: ca_certificate pushes cert retaining other fields, native flag remains true, connector adds platform certs and PEM to one RootCertStore. Workspace Cargo.toml fully read284 lines: tonic0.12 with TLS roots, no tonic patch; crate fully read136 lines inherits workspace. Compatible registry source independently read; locked version not claimed. Question: replace misleading priority/exclusive pinning docs only? Actual `docs_only_correction`, confidence .97, probabilities preserve .01/docs .98/runtime fix0/insufficient .01. Applied only documentation101-105; exact BEFORE affected-hunk excerpt saved in assignment directory, clearly not whole-file backup. Native trust remains enabled; transport, identity, capability and code unchanged. No runtime check authorized or claimed.

## J38/J39 — failure diagnostics and configured profile metadata

Actual batch `jdgb-15b32de725c11b23`, completed2/2.

J38 complete `method_receipt_audit.rs:1-932`: exact static native failure mapping33-198 describes NativeHttpFailure/config/catalog/agent/generation/identity failures, never assigns model/provider; emitted as closed failure_kind656-659, untyped failure receipt unknown661-665. Native CAS361 describes revision-zero RuntimeDocuments reserve364 before effects. Question includes alternative native defect, tched conflict and insufficient evidence. Answer `preserve_local_dependencies_pending`, confidence .96, probabilities .97/.02/.01/0. Preserve local diagnostics; message producers pending.

J39 complete validation144, registry1165, registry fixtures1157: native profile pipeline/readiness requires configured endpoint/key; native IDs/overrides are governed metadata; native_list_providers498-530 reads TOML+Cozo hints, not runtime config; binding target only uses declared/derived helper and excludes dedicated references; own-agent target uses requested provider/model and rejects shared profiles. Static model names are fixture data. Fixtures assert default/alternative/stale/unmapped/removed mappings, not execution proof. Answer `preserve_local_dependencies_pending`, confidence .67, probabilities .75/.23/.02/0. Helpers in op_plugins and downstream tched binding unreviewed here; potential stale ordinary_reference-none fixture conflicts with current required family929-937, reconciliation pending actual helper. No independent edit.

## J40–J42 — MutationEngine caller trace and creation owner

Actual batch `jdgb-15b32e6a9b011b24`, completed3/3. Full MutationEngine8074, creation173 and nlm_cli1596 lines read. Mutation read4001-5000 hit45.1KB limit at4948; explicit4949-5500 filled gap.

J40 actual docs_only_fix .68, probabilities docs .76 / overlap .07 / preserve .17 / insufficient0. Exact comment3186-3187 claimed native Btrfs attach ownership, contradicted by fully read identity implementation and fresh247-268: logical attachment only verifies existing external mount, never mount/unmount/member changes. Applied only those two comment lines, no executable change; exact affected-hunk BEFORE excerpt retained with full-read receipt. Other listed native config/effect/observer/transport terms denote backend operations or exact schema keys; no blanket rename.

J41 actual tched_overlap .91, probabilities0/.93/.06/.01. N1 actual caller now established: `dispatch_tched_router_owned` Chat5725 parses admitted ChatInput, resolve5748 then route.input5759; ControlPlane complete5864 or route client complete5880. Exact guarded admission retains caller/session/grants. Which input/config owns provider/model precedence still requires sealed producer/runtime; do not silently discard explicit admitted selection or restore old handlers. `apply_own_agent_route`5338 and catalog selection5480 have separate explicit requested-model/protected authority. Concrete new cutover overlap: json_render generation arm3844 calls `json_render_generation_dispatch::dispatch`, absent from current fully read parent. Report to tched owner, no independent route fix.

J42 actual preserve_local_dependencies_pending .75, probabilities docs.01/overlap.15/preserve.81/insufficient.03. Native creation uses current exact grant/session/seal admission, durable Cozo reservation before sole POST, reconciliation never repeats POST, exact stopped config/security/devices/ownership/UUID/full-readback checks then SHM publication and durable complete. Native names are labels, not lifecycle authority. External Cozo/Incus helpers not independently certified. nlm_cli526 native gRPC accurately names the shared normalized result transport.

## J43 — exact OpenFlow backend and fixture contexts

Complete sequential reads: obfuscation canonical192, methods513, OpenFlow canonical236, methods469, permit258. Exact contexts supplied: private Switch comparison errors; NativeFlowOwner system Dbus VerifyFlow and CompareExchangeFlow methods; exact returned verified/flow comparisons; durable receipt persistence after readback; injected p2p fixture CAS; native Value binding/action/flow decoding in actual adapter. Asked local terminology only, with external controller and live-switch behavior explicitly unverified. Actual preserve_local **1.00**, probabilities preserve1/clarify0/insufficient0. Preserve these backend and fixture terms. No tests executed.

## J44 — observer, private-error fixture, project backend binding

Actual direct judge preserve_local_dependencies_pending **.60**, probabilities preserve.70/native_fix.20/tched_overlap.02/insufficient.08. Evidence: complete OVS observer210, owned-task fixture215, project dispatcher681. Exact observer monitor_db/watch changed snapshot/projection/pinned-schema checks; deliberate PRIVATE...NATIVE_DETAIL log-exclusion marker; NativeCreationRecord ownership checks and shared creation/reconcile owner337-349. Local terms describe backend effects/snapshots or private diagnostic fixture, no model choice. External monitor and creation producers remain uncertified; no fixtures run. Complete project context resolution restricts session/memory/capabilities/agents, does not itself assign provider/model, so it does not settle N1 precedence.

## J45 — durable CAS and integer representation

Actual direct judge preserve_local **.72**, probabilities preserve.81/native_fix.07/insufficient.12. Complete routing resources223, store607, integer arguments157. Exact budgets/CAS callback/final admission/pure transition/cancellation fixture contexts and original integer number.is_f64 gate supplied. Native means durable backend CAS/owner or already-integer u64 representation, not AI routing or machine-width claim. RuntimeDocuments helper internal producer not independently certified. No tests executed.

## J46/J47 — server transports, socket accepts and specialist caller

Actual batch jdgb-15b32fbe5f411b26 completed2/2. Complete server1563, shared socket644/fixtures402, specialist621.

J46 preserve_local_upstream_pending **.68**, probabilities preserve.76/overlap.06/fix.13/insufficient.05. Native gRPC means tonic Unix routes; MCP HTTP means existing Axum protocol; accept_native fixture opens actual disposable UnixStream and polls sole ControlledIncoming, no fixture executed. Native observation restart only follows explicit reload and calls reviewed engine.start. Stateless native ACK comment863 explicitly conditional; actual runtime/version producer still pending. Complete server/lib/grpc_server/mutation have no run_decoy call: bounded startup-module absence only, not repository-wide/live claim.

J47 preserve_local_upstream_pending **.76**, probabilities preserve.82/overlap.10/fix.07/insufficient.01. Exact retained admission/schema/identity forwarding and guarded completion terms supplied. Specialist selection_guard288 -> readmit290 -> mode resolve306 with default ChatInput/conversation only -> ControlPlane complete417 or other complete_under_guard421. This is another actual N1 caller but does not supply explicit input provider/model to discard. Native completion type names do not allocate model; returned provider/model are observed. Pending dedicated-role precedence stays unchanged, not an independent specialist fix.

## J48–J50 — graph evidence, catalog overlap and config adapters

Actual batch jdgb-15b3300ddfc11b27 completed3/3; all named source files fully read.

J48 preserve_local_upstream_pending **.61**, probabilities preserve.71/overlap.12/fix.02/insufficient.15. Exact graph mail635/648 treats unknown bootstrap status as declared configuration, omits health and emits Declared evidence; native observation distinction accurate locally, external producer unverified.

J49 tched_overlap **.83**, probabilities preserve.11/overlap.87/fix.01/insufficient.01. Exact select141 rejects every candidate outside target.family; fixture205-213 has custom.zen before openai.go with openai target, yet unwrap expects success. This is a local source contradiction, not an executed failure. Coordinated binding ownership; no independent hunk changed. Native OAuth credential/origin claims and endpoint-match exception await actual pinned runtime producer.

J50 preserve_local_upstream_pending **.95**, probabilities preserve.96/overlap.02/fix.01/insufficient.01. Config dispatch119/engine443/secret490/own-agent343: exact sealed native_config document and sensitivity policy, shared backend methods, readback/output validation and pin checks; no global CAS claimed. Native Configurable natural-key addressing upstream remains pending. Explicit $MODEL token catalog resolution is scoped by provider path, not an ambient/Cargo model allocator. Own-route changes require canonical CP session/$ROOT/exact self-route grant/ControlPlane mode; prepare tests refs and rejects sharing; verify_readback excludes only intended model+role fields. Local terms preserved, dedicated-role N1 precedence unchanged.

## J51/J52 — final admission and complete native generation adapter

Actual batch jdgb-15b3306bcd811b28 completed2/2. Complete admission124/fixtures141 and generation1415.

J51 preserve_local_upstream_pending **.84**, probabilities preserve.88/overlap.05/fix.06/insufficient.01. Exact final readmission after guard retains caller/facade/current same seal/grant/session, acknowledges reseal/revocation race5. Fixture29 first possible native effect explicitly means counter, not real backend; unchanged.

J52 tched_overlap_documentation **.89**, probabilities preserve.08/overlap.92/fix0/insufficient0. Exact139 documentation says Drop requests cancellation and aborts task, contradicted by Drop187-190 only calling cancel(), sending oneshot180-183, no join.abort. Proposed coordinated hunk139: “Owned native turn. Dropping it requests cooperative cancellation;” followed by existing owner-state mapping line140. Do not restore abort behavior or overwrite OpenCode's shared tched surface. Backend generation types/errors/request/session/fixture terms otherwise describe reviewed bounded loopback WS protocol. ACK/session/stateless/SID1/provider/model constraints checked before prompt; done must match deltas and ACK. Closed diagnostics and deadlines/cancellation reviewed; actual pinned binary stateless/no-task-id/no-tools-memory semantics unverified. Fixtures simulated private loopback, not executed. Full per-position generation token ledger still pending at this checkpoint.

## J53/J54 — tched runtime and tracing terminology

J53: Complete sequential reads of `tched_router_runtime.rs:1-4181` (ranges listed in coverage). Declarative JEV rated local native labels as adapter/config/gateway/API terms, not implicit provider/model selection or proof of upstream execution: `preserve_local_upstream_pending`, confidence .96, probabilities .97/.02/0/.01. Upstream native HTTP/WS semantics remain unverified. No source change.

J54: Complete `tracing.rs:1-186`. Exact line5 “native-gRPC side” distinguishes tonic interceptor path from tched_router-hosted HTTP/gRPC-Web side. JEV chose `preserve_local_terminology`, confidence .41 (LOW; probabilities .56 preserve/.39 clarify/.00 defect/.04 insufficient). Treat as provisional, not decisive; local wording retained pending context/dependency evidence.

## J55 — UI workspace owned-task terminology

Complete `ui_workspace_dispatch.rs:1-962`; sole occurrence line497 said caller cancellation is survived after “native submission,” while the code spawns an owned Tokio publication worker. JEV chose clarify, confidence .73 (preserve .04/clarify .82/defect .14). Changed the comment to “after task submission” (with “owned task” immediately preceding it). Behavioral code unchanged. No runtime verification claimed; comment-only change.

## J56 — Xray handler/API terminology

Complete `xray_methods.rs:1-564`. Contexts: Native Xray HandlerService adapter (line1), parser identifier for existing Xray wire account (85), receiver protobuf decode in preflight error (437), and Xray AlterInbound effect after validation (449). Source shows `op-xray-daemon::commander_client` and typed messages; local “native” distinguishes Xray handler state/API from OP-DBUS facade. JEV preserve confidence .32 (LOW; preserve .49/clarify .36/defect .07/insufficient .08); not decisive. Keep local terms provisionally; do not infer unverified daemon/version semantics.

## J57 — Xray test fixture terminology

Complete `xray_methods_tests.rs:1-726`. JEV chose clarify, confidence .61 (preserve .20/clarify .71/defect .07/insufficient .02): fake HandlerService/StatsService in disposable Unix socket fixtures prove message behavior, not a live/native daemon. Reworded ten occurrences in assertions, fixture error, test names and type fallback to “Xray counter/HandlerService/Xray config.” No assertions or behavior changed. Tests not run.

## J58 — audit timing/cache terminology

Complete `tests/accountability_audit_trail.rs:1-421`. Native Btrfs cache at line381 contrasts optimized optional cache with authoritative timing records; “native timing trail” at402 is the timing directory asserted inside the isolated Btrfs fixture. JEV chose clarify confidence .43 (LOW; preserve .28/clarify .57/defect .01/insufficient .14). No independent behavioral defect established; preserve tentatively pending exact cache ownership/runtime verification.

## J59 — Incus exact-schema test field

Complete `tests/exact_state_children.rs:1-99`. “native” is the intentional exact Incus fixture schema field/key exercised in initial state, nested paths, sibling assertion and rejection path; variable `native` holds the schema JSON. JEV preserve confidence .88 (preserve .91/clarify .05/defect 0/insufficient .04). No edit.

## J60 — explicit model provider binding identifiers

Complete `tests/integration_model_routing.rs:1-89`. `native_profile_overrides`,
`native_provider_id`, and `native_provider_id_for_model` are model-registry API
identifiers used to resolve explicit profile/provider mappings; test comments
describe configured override and unmapped rejection. JEV preserve confidence
.87 (preserve .90/clarify .03/defect .01/insufficient .06). Preserve names and
meaning; no change.
## J61 — protobuf test-name precision

Complete `tests/native_method_contracts.rs:1-262`. The two names call a
generated `DynamicMessage` protobuf encode/decode roundtrip “native wire” and
plugin schema return values “native results”; neither exercises native
platform APIs. JEV chose clarify, confidence .73 (preserve .17/clarify .80/
defect 0/insufficient .03). Renamed to
`required_scalar_presence_survives_protobuf_wire` and
`plugin_results_survive_generated_protobuf`; assertions and behavior unchanged.
No tests run.

## J62 — generated protobuf response naming

Complete `tests/notebooklm_response_contracts.rs:1-290`. The `native` binding
is a generated `NotebooklmNotebookGetResponse`, while the test names describe
typed/generated protobuf response fields. The phrase “native response” is
ambiguous about the source of those fields; JEV chose clarify, confidence .93
(preserve .01/clarify .95/defect .04/insufficient 0). Renamed the local binding
to `response` and the two test names to explicitly say “typed protobuf.”
Behavior unchanged; tests not run.

## J63/J64 — E2E client and validator wording

Complete `tests/oracle_assertion_e2e.rs:1-3460`. At line 3000, “native
client” refers to the concrete `RemoteOperationClient` reading the configured
credential; JEV chose clarify, confidence .81 (preserve .12/clarify .86/
defect .02/insufficient 0). At line 3114, “native validator” refers to the
server's `AssertionValidator::from_env`; JEV chose clarify at confidence .47
(preserve .39/clarify .60/defect .01/insufficient 0), a low-confidence answer.
The human disposition uses exact OP-DBUS component names for both and does not
imply native-platform behavior. No behavior changed; tests not run.

## J65 — tched router backing fixture terminology

Complete `tests/tched_router_backing.rs:1-1501`. The `complete_native_config`
fixture and `native_config_defaults` are sourced from the imported upstream
contract; JEV chose preserve, confidence .98. Other occurrences describe the
external tched_router runtime/backend and its HTTP observation in the isolated
fixture. The bundled backend-wording judgment was low confidence (.28,
preserve .48/clarify .52); do not generalize it. Preserve accurate fixture
semantics; no source change in this file. No tests run.

## J66 — tonic client terminology

Complete `tests/tched_router_integration.rs:1-344`. “Native tonic client”
contrasted the Rust tonic client with gRPC-Web clients; JEV chose clarify,
confidence .62 (preserve .21/clarify .75/change .04). Changed the test comment
to “tonic Rust client.” Comment-only; no tests run.
## J67 — generation task Drop contract

Declarative JEV job `jdgb-15b334c3eab04bfa`, TypeSafe jev-latest. Exact source
`tched_router_generation_native.rs:139-190` says Drop requests cancellation and
abort, but `Drop::drop` calls `cancel()`, which only sends the oneshot signal;
the JoinHandle is not aborted. JEV chose `clarify`, confidence .85 (preserve .03,
clarify .89, defect .05, insufficient .03). Corrected only the comment to
cooperative cancellation; no behavior change. Exact prior hunk is preserved at
`/var/tmp/native-line-audit-20261024.ILroaQ/crates/op-grpc-bridge/generation_native_drop_before.txt`.

## J68 — additive TLS roots

Declarative JEV job `jdgb-15b334c3eab04bfa`, TypeSafe jev-latest. Source evidence:
`grpc_client.rs:97-139`; tonic 0.12.3 `transport/channel/tls.rs:37-42` appends
configured CA certificates while retaining the builder, and
`transport/channel/service/tls.rs:36-61` adds platform roots and configured
certificates to the same RootCertStore. JEV chose `preserve`, confidence .77
(preserve .83, clarify .14, defect 0, insufficient .03). Existing doc correction
accurately says additive trust, not exclusive priority/pinning. No trust or
transport behavior changed.

## J69–J72 — legacy tched_router dispatch contexts

Declarative JEV job `jdgb-15b334f8f8304bfb`, TypeSafe jev-1.14.0, covered the
actual complete `tched_router_legacy_dispatch.rs` source contexts separately:
J69 (`89-168`, tool structs and external API mapping) chose preserve/.49
(preserve .62, clarify .37, defect 0, insufficient .01); J70 (`170-195`,
coherent observation and admission boundary) chose clarify/.27 (preserve .41,
clarify .45, defect .04, insufficient .10); J71 (`196-270`, dispatch/config
state) chose preserve/.45 (preserve .59, clarify .34, defect .01, insufficient
.06); J72 (`273-358`, local HTTP fixtures) chose preserve/.45 (preserve .59,
clarify .40, defect 0, insufficient .01). All are low-confidence except the
zero-admission interpretation is locally visible: “native” names the external
router data/config/adapter surface, not global model/provider authority. Manual
review preserves these terms; J70 does not prove a material wording error.

## J73–J75 — generation adapter token contexts

Declarative JEV job `jdgb-15b33547f9304bfc`, TypeSafe jev-1.14.0. J73
(`tched_router_generation_native.rs:1-190`, request/admission/task type and
transport documentation) chose preserve/.83 (preserve .87, clarify .13, defect
0, insufficient 0). J74 (`193-774`, bounded WebSocket protocol, selection,
identity, error and session helpers) chose preserve/.71 (preserve .78, clarify
.21, defect 0, insufficient .01). J75 (`776-1415`, local protocol fixtures and
test identifiers) chose preserve/.81 (preserve .86, clarify .13, defect 0,
insufficient .01). These judgments support the local adapter/protocol/type
vocabulary only; they do not certify the pinned upstream producer or establish
provider/model authority. J67 separately corrects the false abort claim.
