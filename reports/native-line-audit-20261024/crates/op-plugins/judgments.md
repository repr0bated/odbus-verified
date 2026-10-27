# Declarative JEV receipts

## J1 — initial contexts

Installed `judge()` via eval JS, no model overrides or routing. Actual successful call in worker transcript. State contexts:

- C001 `crates/op-plugins/Cargo.toml:48-51`: `# Incus's privileged Unix socket needs the native UDS transport added in 0.12.\n# Keep other plugin adapters on their existing workspace client.\nreqwest-unix = { package = "reqwest", version = "=0.12.28", default-features = false }`.
- C002-C003 `examples/method_coverage.rs:94-117,170-177`: validate_proof requires exit_code == 0, nonempty exact test name/behavior; `matches!(proof.scope.as_str(), "owner" | "canonical" | "native")`, diagnostic `proof scope must identify owner, canonical, or native behavior`. Checks source/log SHA256 and exact passing test log line. Limits explicitly require review of test assertions and full unchanged-source admission gate; owner fixtures do not prove canonical transport/live integration.

Questions and explicit outcomes:

1. `uds`: Evaluate C001 native in actual wording/intent. Does it accurately describe library-provided Unix-domain socket transport rather than model/provider selection or privilege grant? Do not infer deployment or independent release-history proof. Choices preserve (legitimate library UDS terminology; no grant/AI routes), change (misleading wording/declaration), insufficient (cannot distinguish). Actual answer: `preserve`, confidence **0.88**, probabilities preserve0.92/change0.07/insufficient0.01. Decision preserve; not an independently verified reqwest release-history claim.
2. `scope`: Evaluate BOTH C002-C003 actual wording and operation, semantic correctness beyond routing. Is native a descriptive evidence category with limits, not proof all integration passes/runtime override? Choices preserve (descriptive label/diagnostic with stated limits), change (misleading semantics/runtime override), insufficient. Actual answer: `preserve`, confidence **0.94**, probabilities preserve0.96/change0.02/insufficient0.02. Decision preserve both tokens; source-only offline validator, not executed proof.

## J2 — ordinary substrings

Evidence: canonical.rs:82-87 exact deprecated Alternative legacy-path comment/constant; antigravity.rs:893-900 and antigravity_chat.rs:358-365 exact `if let Some(alternatives) = node.get("anyOf").or_else(|| node.get("oneOf")).and_then(JVal::as_array) { for v in alternatives { collect_subids(v, out); } }`. All five positions mapped individually in occurrences.tsv.

Question `ordinary`: Are Alternative and all four alternatives binding/use identifiers accurate vocabulary for a different legacy path and JSON Schema union branches? Actual intent/wording, not only routing. Outcomes preserve (accurate ordinary vocabulary), change (misleading semantics), insufficient. Answer preserve, confidence0.74; probabilities preserve0.83/change0.15/insufficient0.02. Preserve all five.

## J3 — credential and bridge semantics

Evidence state supplied each exact native-bearing text individually: antigravity.rs:3,330,334,342,646,656,789,851,920; antigravity_chat.rs:16,22,25,41,47,84,94,142,199,209,253,261. Their texts, meanings and per-occurrence mapping are in occurrences.tsv. State additionally included:

- Antigravity current_state `unobserved`, empty/default auth and selected_model, provider gemini and Google v1beta URL, default llm_plugin tched_router; declared get_auth_status/get_usage_report/configure_safety with exact read/write capabilities; generic lifecycle rejects without authoritative readback. Test920 only checks absence of old fields and local usage/no billing.
- Chat ConfigureInput expected_revision/provider gemini|openai_compatible/base_url/selected_model; read bridge, mutation refresh/configure with exact capabilities; no generation dispatcher here; tched_router default conflicts with already-read retired registry gate.

Question `vocabulary`: Are all native/Native compounds coherent direct-provider/credential vocabulary rather than names proving unauthorized AI selection? Outcomes preserve_terms, change_terms, insufficient. Answer preserve_terms confidence0.97; probabilities0.98/0.01/0.01. Preserve terms; no blanket rename.

Question `claims`: Do these declarations/test substantiate stronger proven/actually-consumed/bounded-readback/applied-policy claims or active tched_router delegation? Outcomes receiver_reconciliation (need receiver/caller evidence and retired-owner reconciliation), verified, defect_proven (every operation demonstrated broken). Answer receiver_reconciliation confidence1; probabilities1/0/0. This is unresolved evidence/overlap, NOT an established broken credential implementation. Do not auto-replace delegation with another provider.

## J4 — kernel backend, provenance and refusal vocabulary

Evidence:

- btrfs_plugin.rs:251,281,290,810,812,887 exact names/call/error/schema wording from occurrence ledger; all BtrfsBackend implementations call btrfs_uapi with descriptors, enforce device type/rdev and membership readback, and create/snapshot/delete postconditions. State declares transport topology btrfs_send_receive; twelve methods do NOT include transfer. Generic verify_state true is separate and was explicitly not blessed.
- common/llm_projection.rs:168-204 ConfigSchema including exact native schema/native type comment, native-type subid and native_type identifier. Provenance strings, SDK/OpenAPI metadata and env_vars, not executable schema authority.
- chat_response.rs:116-131,835,860 and chat_response_methods.rs:204-222,597-604,648-657: exact Refusal/CannotPerformInput private Alternatives description/field, bounded validate_items("alternatives", alternatives, MAX_LIST_ITEM_BYTES), `alternatives: self.alternatives`, and both private-field expectation literals.

Questions/outcomes:

1. `btrfs_terms`: Do all six terms accurately denote direct kernel/uapi backend/error with separate send/receive topology rather than falsely exposing transfer? preserve/change/insufficient. Answer preserve confidence0.80; probabilities0.87/0.09/0.04.
2. `metadata_terms`: Are all four descriptive provenance/type fields, not independent schema authority/selection? preserve/change/insufficient. Answer preserve confidence0.95; probabilities0.97/0.01/0.02.
3. `refusal_terms`: Do every supplied Alternative substring, literal, identifier, same-line position and privacy expectation accurately describe bounded refusal suggestions? preserve/change/insufficient. Answer preserve confidence0.61; probabilities0.74/0.19/0.07. Above0.6 threshold; manually observed plain suggestion payload/validation and privacy contexts support preserve. No execution inferred.

## J5 — Cozo engine vocabulary and proof boundary

Evidence: cozo.rs:38-44 exact import comment; cozo_methods.rs:1,60,143,160,165,186,192,211,338,508,582,596,904,1254,1306,1345,1391 exact occurrence texts individually supplied (see ledger). Additional supplied source evidence:

- Supplied store.db().run_script then flatten NamedRows; export_relations -> encoding cozo-json; imports build parameterized :put with metadata comparison and100000 row bound.
- Custom VecF32Bits/VecF64Bits backup codec, finite/typed values, BTreeSet<Vec<DataValue>> duplicate keys, rejected upstream Vec/Regex/internal tags, from_bits reconstruction.
- Restore issues combined checks/script before committed/atomic receipt; no model dispatch. Upstream dependency implementation was not read.
- cozo_methods_tests.rs:36,213 fixture descriptions/assertions: seven methods + indexed readback on in-memory engine; invalid import type rolls back input/trigger log then successful import has trigger output. Explicit NOT RUN.

Question `terms`: Evaluate EVERY supplied native comment/diagnostic/name/value/test name semantically; is engine/format/value/endianness vocabulary warranted, without blanket removal/AI routing inference? preserve/change/insufficient. Answer preserve confidence0.97; probabilities0.98/0.02/0.

Question `guarantees`: Do construction and unexecuted assertions prove upstream endian decoder, index/trigger semantics, transaction/rollback and durability? dependency_unverified/runtime_proven/defect_proven. Answer dependency_unverified confidence1; probabilities1/0/0. Preserve vocabulary; no runtime proof or confirmed engine defect.

## J6 — remaining union and fixture names

Exact cognitive.rs:1523-1530 union binding/use provided; cozo_methods_tests.rs:381-418 detailed type parser and vector codec assertions supplied.

`union`: Are BOTH alternatives identifiers accurate anyOf/oneOf traversal vocabulary? preserve/change/insufficient. Answer preserve confidence0.85; probabilities0.90/0.06/0.04.

`tests`: Do BOTH native fixture names fit supplied assertions without execution/routing inference? preserve/change/insufficient. Answer preserve confidence0.34; probabilities0.56/0.03/0.41. **Rejected as decisive evidence** because confidence<0.6; supplemented with exact assertion code in J7.

## J7 — resolving weak fixture judgment

State contained complete assertion-bearing bodies from cozo_methods_tests.rs:380-418: injected/malformed type strings, fixed list width, infinity/nullability; nested F32/F64 vectors including negative zero, codec roundtrip equality, rejected Regex/Vec tags and oversized F32 bit word. Decoder allowlist/custom bit-tag handling and TypeSpec variant checks were supplied.

Question `lexical`: ONLY whether native accurately denotes Cozo DataValue/upstream serde forms in the two identifiers; do not claim tests pass, exact negative-zero bit strength, or upstream decoder safety proof. keep_native_word/rename_native_word/cannot_tell.

Actual answer keep_native_word confidence0.97; probabilities0.98/0.02/0.00. Preserve both native terms. Bitwise assertion strength remains separate generic observation, not a claim this check proves every bit.

All calls above used the installed declarative judge facility with already-read evidence. No routing, model override, arbitrary eval execution, source extraction or synthetic endpoint. Full call states/results are also retained in the worker transcript. Remaining unread contexts remain pending.

## J8 — external scheduler ownership

State supplied every exact native-bearing context in cron.rs:3,41,153,215,349,381,390,393,411,508,512 (both positions). Texts/individual positions are retained in occurrences.tsv. Additional evidence: GlobalDaemon only, Unsequenced/BestEffort event declarations, nine methods with cron.read/cron.invoke, all generic lifecycle methods reject, no public state paths; explicit confirmed/provider-completed-unverified/indeterminate DTOs and absence readback. No actual gateway receiver in file. Agent create accepts an optional explicit model; shell variant separate. Registry retirement of tched_router does not itself prove HTTP receiver retirement; threetched captures the binary contract.

`words`: Are ALL twelve terms coherent external scheduler/transport/agent-owner vocabulary rather than mandatory AI selection? preserve_terms/change_terms/insufficient. Actual answer preserve_terms confidence0.91; probabilities0.94/0.02/0.04. Keep vocabulary.

`claims`: Can declarations prove endpoints/version/snapshot bound/SID1 admission/future autonomous policy/current catalog-owner alignment? reconcile_receiver_and_owner/all_verified/all_broken. Actual answer reconcile_receiver_and_owner confidence1; probabilities1/0/0. Record explicit tched overlap; do not rename provider, grant access or infer all endpoints broken.

Checkpoint: eight successful declarative calls, fifteen typed answers; J6.tests confidence0.34 is non-decisive and supplemented by J7. No native-specific source patch or runtime proof.

## J9 — episode/provider/network owner terminology

Exact native-bearing texts at ctl_plane_chatbot.rs:57,365,541,549,594,612, ctl_plane_chatbot_methods.rs:358-364 and decoy_network/mod.rs:1-4 were individually supplied; mapped in occurrences.tsv. Additional state: build_completed_chat_episode108-152 constructs one UUIDv7 scoped record per call, pii_flagged=true, plugin_id=tched_router, hashes/validates before return; plan_vectorization347-357 omits all three flagged text fields. Config rejects reserved policy changes and has no chatbot-specific route/model override. Actual Chat invocation path, embedding owner mapping and decoy constructor callers unread. Registry retires tched_router.

`words`: Evaluate all EIGHT contexts semantically as application/provider-feature/backend-owner vocabulary rather than a hidden mandatory model gate. preserve_terms/change_terms/insufficient. Answer preserve_terms confidence0.92; probabilities0.95/0.01/0.04.

`claims`: Distinguish proven pure builder/redaction structure from every-completed-Chat invocation, active retired-router selection, embedding role mapping and decoy-only constructor exclusivity. partial_support_reconcile/all_proven/all_defective. Answer partial_support_reconcile confidence1; probabilities1/0/0.

Checkpoint now nine successful installed declarative calls/seventeen typed answers. J6.tests remains weak/non-decisive, supplemented by J7. More source/caller review is pending; no runtime or completion claim.

## J10 — resumed decoy owner/readback contexts

Updated common instructions read; retained source/J1–J9 evidence preserved. No retained eval kernel reused; installed judge API reread and a fresh declarative call succeeded.

Evidence supplied all eleven exact occurrence contexts now mapped in occurrences.tsv: wgcf_owner.rs289,346,394,439,851,1003; wireguard_owner.rs182,523,529; wireguard_owner_tests.rs380,384. State supplied provider GET/validation before status/update/key-rotation commits, requested-value comparisons, local HTTP fixture/injected-kernel assertions and mismatch stored-state preservation; kernel absent-device aggregation, host-bit interface vs canonicalized AllowedIPs, effect/readback checks. Kernel fixture opt-in guard, root/unshare before runtime/backend, six+two method paths/schema/secrecy/link readback were explicit. Public DecoyRole marker/allowlist does not itself authenticate caller identity.

`meaning`: Evaluate each native occurrence semantically and for wording, not merely routing; preserve accurate provider/kernel/opt-in fixture terminology versus change concrete misleading description versus insufficient. Actual preserve0.86; probabilities preserve0.91/change0.08/insufficient0.01.

`boundary`: Do these files prove decoy-only constructor callers or live kernel/provider success? source_only/runtime_proven/broken. Actual source_only1; probabilities1/0/0. No live/kernel/provider pass or caller exclusivity inferred.

Ten successful judge calls/nineteen answers cumulative. No searches/commands/runtime/tests. Continue every remaining file.

## J11 — DNS and embedding vocabulary, weak combined answer

Actual successful judge supplied four DNS source positions343,1187,1414,1447 and nine embedding/chatbot positions (all listed under J12). Evidence covered all20 direct resolve1 D-Bus method matches, typed RegisterService and fixture tuple comparisons; provider-specific configured embedding request shapes, conditional roles, model enumeration, credential origin checks and unexecuted fixtures.

`terms`: preserve/change/insufficient for each native wording and meaning, not routing alone. Actual preserve confidence0.53, probabilities0.69/0.28/0.03. **Non-decisive; supplemented J12.**

`evidence_limit`: actual source_supported_only confidence1, probabilities source_supported_only1/live_proven0/all_broken0. No live provider/resolver pass inferred.

First attempted call had JavaScript parse error before execution (apostrophe); retry above succeeded. A subsequent documented judge_batch attempt failed ReferenceError: judge_batch is not defined. Reported API inconsistency via xd://report_issue; enabled no tools. Neither failed call counts as a successful judgment.

## J12 — separate DNS and embedding semantics

Successful installed judge used one evidence object with two explicit questions after unavailable judge_batch. DNS actual contexts:343 direct native Manager API;1187 fixture must match native ABI;1414 native wire arguments;1447 native argument diagnostic. State supplied exact typed method/tuple evidence and distinguished normative fixture ABI requirement from runtime proof.

Embedding actual contexts: embedding_model.rs1 and269 direct configured adapters; methods.rs1 adapter header,169 conditional provider role support,193 provider model-list vs configured-only provenance,604 unsupported adapter truncation diagnostic,1018 Ollama tags/embed fixture,1225 corresponding rejection expectation; ctl_plane_chatbot_methods.rs361 conditional native role hint. Supplied configured model unchanged, Voyage input_type/truncation/output_dimension, Ollama truncate/dimensions/no input_type, OpenAI-compatible float/dimensions/no input_type/reject truncate, provider lists vs configured Voyage and all embedding_support_verified=false. Unexecuted fixtures explicitly identified. Custom upstream extensions not assumed.

`dns_semantics`: preserve confidence0.77; probabilities preserve0.85/correction0.13/insufficient0.02.

`embedding_semantics`: preserve confidence0.77; probabilities preserve0.85/correction0.14/insufficient0.01. Resolves chatbot retrieval-field source mapping; not live provider proof.

## J13 — EMQX direct backend transport and fixtures

State supplied each of34 source positions and filename: emqx.rs239,245,528,550,595,742,767,849,876,902,1006,1050,1089,1143,1147,1804,1805; emqx_native_tests.rs317,318,324,354,388,389,445,503,515,525,535,568,569,579,591,606,607; filename emqx_native_tests.rs.

Exact type/module/fixture names supplied with each function context. Evidence: local bounded credentialed broker API plus fixed runit D-Bus transport; injected peer-bus/HTTP fixture; auth/method/path/status/mTLS validation and rejection assertions; static get_mcp_status declaration not treated as remote readback; no AI selection; tests unexecuted. Separate publish_responder_wake dispatcher/schema mismatch explicitly generic out-of-scope.

`semantics`: preserve/correction/insufficient for each native term's meaning and wording. Actual preserve confidence0.72; probabilities0.81/0.18/0.01. Preserve terms; no native-specific defect shown.

Cumulative13 successful declarative calls/24 typed answers; two failed invocations excluded. J6 and J11 weak semantic results supplemented by J7 and J12 respectively.

## J14 — daemon/hostname backend vocabulary and JSON values

State supplied all five Fail2ban positions (fail2ban.rs4/152/212; methods.rs1/795), all fifteen full_system native positions (198/202/216/218/228/230/886/947/1131/1142/1143#1/#2/1149/1152/1153), and freedesktop.rs74/583. Exact names/messages and implementation/fixture facts supplied. Daemon uses framed pickled Unix-socket requests and ack counts/status, reload explicitly refuses; hostname uses sethostname/file write with ignored UTS/tempfile fixture; JSON field is serde_json::Value with canonical target/unknown-field input fixture, actual bridge target-field enforcement unobserved.

`adapter_words`: actual correction confidence0.49; probabilities preserve0.32/correction0.66/insufficient0.02. **Low-confidence, not decisive; preserved contrary result, supplemented J15/J16.** State also mentioned unrelated full capture incompleteness; no inferred explanation for model result.

`json_words`: preserve confidence0.80; probabilities0.87/0.12/0.01. Preserve native JSON vocabulary; bridge enforcement claim remains coordinator receiver evidence.

## J15 — narrower daemon and hostname semantic evidence

Same five daemon and fifteen hostname actual contexts, with direct request lists, ack return/error, reload refusal, syscall/file-write implementation, UTS fixture isolation and each fixture literal supplied. No generic full-capture guarantee treated as native claim.

`daemon`: preserve confidence0.56; probabilities0.71/0.28/0.01. **Still low, not decisive; supplemented J16.**

`kernel`: preserve confidence0.94; probabilities0.96/0.04/0. Preserve accurate direct OS backend/method/ignored fixture identifiers. No production atomicity/readback or test pass asserted.

## J16 — grounded Fail2ban verification

Actual installed jev_verify call supplied all five exact native contexts plus direct request lists/framing, configured UnixStream, response ack/count/status, reload refusal, and local UnixListener canned fixture assertions. Claim: native/daemon-native denotes direct daemon control-socket protocol; native_fixture explicitly denotes adapter fixture, not installed daemon/runtime proof or model routing; terminology matches shown implementation.

Actual verdict supports; confidence0.82; misleading0.5/3; verbatim0.09 (no quote requested); tokens839/78. Preserves terms; no runtime/protocol interoperability pass inferred.

## J17 — hardware OS observation and units

Actual state supplied all fourteen hardware positions116/125/155/180/285/291/294/297/569/570/571/572/573/632 and exact descriptions/identifiers/diagnostics. Evidence: new sysinfo System per request, memory/CPU refresh, fresh disk list, raw-value checks, bytes/1024 conversion, output paired with same inventory, observed not mutation/hotplug, no atomicity assertion, unexecuted supplied fixtures. Bridge state-commit caller not proven here.

`semantics`: preserve confidence0.77; probabilities preserve0.85/correction0.14/insufficient0.01.

Cumulative16 successful judge calls/29 typed answers plus one successful jev_verify receipt (17 successful declarative invocations). Two failed API/syntax invocations excluded. All weak J6/J11/J14/J15 dispositions retained and supplemented rather than silently accepted.

## J18 — Incus native contracts, provider fields and stale fixture claims

Entire incus.rs4261 lines, incus_device.rs762 and identity_sled.rs1236 read sequentially. Actual state enumerated all60 Incus occurrences individually (function/member/fixture positions map one-to-one in ledger), all10 sled occurrences101/245/336/342/378/387/390/423/434/1028, and two device primitive/discriminant contexts. Incus source position labels without numeric anchors identify exact enclosing function/member and occurrence ordinal, not guessed line numbers; multiple native substrings in3690/3707 and all declaration/use/message contexts separate.

Evidence supplied direct bounded HTTP-over-UDS with no redirects/retries, validated sync/async completion envelopes and one long wait, default-project snapshots, strict/lossless device-map conversion, ETag raw sibling preservation, represented-mutation readback/error handling and lifecycle wakeups. Native fields/names refer provider metadata/primitive types; actual bridge allocation/admission/idmap/storage/ticket/SHM caller contracts not claimed proven. Device primitive visitor supports bool/i64/u64/string; u64 cast-wrap generic risk explicit, not silently endorsed.

Specific contradictory claims supplied: bound power accepts canonical SID and numbered ID-N plus pinned chatbot; sled101 says SID except chatbot alias only, unlike its387 durable creation-binding description. Power fixture name says only pinned native name. Legacy create fixture3710–3734 calls rejected create_instance/name=demo without operation_id and expects POST/created=true, while dispatcher unconditionally bails and actual reserved POST belongs to bridge durable owner.

`terms`: backend_vocabulary confidence0.98; probabilities backend_vocabulary0.99/ai_override0.01/insufficient0. No AI model override shown by these terms.

`needed_changes`: specific_reconciliation confidence1; probabilities specific_reconciliation1/all_accurate0/all_broken0/insufficient0. Native words remain accurate, but exact stale documentation/fixture claims need reconciliation. No runtime execution inferred. Creation fixture removal remains actionable after ownership reconciliation; chatbot naming/doc hunk overlaps tched/identity owner.

Cumulative17 successful judge calls/31 typed answers plus one successful grounded jev_verify receipt (18 declarative invocations).
## J20 — generation boundary wording

Source `crates/op-plugins/src/state_plugins/json_render_generation.rs:1-2`: “Provider-only, memory-free generation of one validated chat Canvas. Admission and native execution belong to the bridge, not the caller's mode.” This describes a generated UI proposal, while authenticated `invokePlugin` backend operations are bridge-admitted and grant-checked.

First choice judgment confidence0.18 (`preserve`0.23, `change`0.45, `insufficient`0.32) was too weak. Follow-up choice confidence0.77 (`change`0.85, `preserve`0.05, `insufficient`0.10): “native execution” ambiguous; clarify to backend operation execution while retaining bridge/caller-mode boundary. Applied minimal comment-only edit at line 2; exact BEFORE excerpt under assignment `BEFORE/json_render_generation.rs.native-execution.txt`. No runtime/test claim.

Cumulative successful declarative judgments now include the two calls above; earlier 17-call/31-answer subtotal remains from J18 receipt.
## J21 — React Native renderer fixture

`json_render_methods.rs:870` sets `default_renderer` to `"react-native"`; 875 binds the `validate_spec` JSON to `native`; 880 passes it to that validator. `json_render.rs:198-213` declares `RendererTarget` with kebab-case serialization and `ReactNative` as a variant. Initial grouped judgment confidence0.45 preserved0.63/change0.35/insufficient0.02 was weak. Follow-up exact-enum judgment confidence0.94 preserved0.96/change0.04/insufficient0: the provider renderer name and local fixture variable are accurate and scoped. No runtime/test run.
## J22 — SSH agent test-name accuracy

`keypair/agent.rs:142-176` test uses `UnixStream::pair`, a spawned fixture peer that checks the request and emits a fixed identity response in partial chunks, then verifies protocol decoding. It does not connect to `SSH_AUTH_SOCK` or a live agent. Declarative choice confidence0.95 (`change`0.97): the old `native_agent_inventory...` name misleadingly implied native/live SSH-agent integration. Renamed narrowly to `ssh_agent_protocol_fixture_retains_public_key_and_fingerprint`. No test run; user explicitly prohibited tests.
## J23 — login1 native wire terminology

`login1/methods.rs:1-3` names native wire mappings and cites the upstream systemd login1 XML also used by elogind. Same module dispatches exact manager methods through zbus with explicit typed D-Bus arguments/variants (`SessionPropertyValue`, CreateSession tuple, leases); tests use a local D-Bus wire fixture. JEV choice confidence0.88 preserved0.92/change0.07/insufficient0.01: accurate direct D-Bus wire-contract terminology. Preserve; this does not mean arbitrary OS/native-code behavior.
## J24 — mail backend terminology

`mail_server.rs` and `mail_server/methods.rs` use native for the explicit Postfix/Dovecot owner module, owner-managed state, private D-Bus interface/peer, readback, and effect. Evidence in `mail_server/native_owner.rs` shows a concrete D-Bus service backed by durable Cozo state and Postfix/Dovecot materialization/queue protocol; the adapter calls it. Mutations currently fail closed pending verified principal/capability context, which the code says explicitly. JEV confidence0.76/0.72 preserve0.84/0.81; retain these scoped role/state terms; they do not claim mutation authorization or success.
## J25 — mail owner fixture vocabulary

`native_owner_tests.rs` uses `NativePaths` for temporary and configured filesystem paths; its opt-in environment/container constants select a real Incus-backed fixture; `native_command` executes command binaries, and test `owner_outputs_are_consumed_by_native_postfix_and_dovecot` verifies generated files through Postfix `postmap` and a separately configured Dovecot instance. The fixture is not run. JEV confidence0.76 preserved0.84/change0.11/insufficient0.05: retain names; they accurately distinguish a concrete backend integration fixture from the in-memory protocol/unit fixtures, and do not claim the test ran.
## J26 — mail owner path and migration names

`mail_server/native_owner.rs` defines `NativePaths` for production Postfix/Dovecot files, queue sockets and mail root, with fixture instances supplying temporary paths; the type appears at its declaration, constructors and materialization/readback functions. `mail_server_native` is a durable document identifier; `pre-owner` refers to legacy flat-file state; `.Native` names the D-Bus interface; the access-denial error points to this owner. JEV confidence0.82 preserved0.88/change0.09/insufficient0.03: preserve these concrete names; no broader host-native or authorized-mutation claim follows.
## J27 — MCP target naming

`mcp_methods.rs:1,122` adapts configuration requests to sealed `tched_router` `GetMcpConfig`/`PatchConfig` methods; its test asserts `PatchConfig` and argument retention. First and exact-context choices were weak (0.33, 0.27; each had substantial insufficient mass). Source proves this is a compatibility adapter to the canonical target; changed the prose/test label from native to canonical, which names what the wrapper actually targets. No behavior changed; tests not run.
## J28 — plugin module blanket claim

`state_plugins/mod.rs:1` described every plugin as using native protocols, while the module tree spans system services, databases, network APIs, captured external schemas, and other declared backends. JEV confidence0.88/change0.92: overbroad for this heterogeneous set. Reworded the header to “each manages a domain through its declared backend”; no runtime behavior changed.

## J29 — model registry router-profile terminology

`model_registry.rs:106-108,238-245` describes fixed router profile families, fallback `custom.{id}` profiles, and an eventual router lookup. JEV judged changing `native serve families` to `router profile families`; generic choice for router accepts/adds wording was weak (0.30), but retained the claim scoped to router config pending independent evidence. For the provider-list projection, the function explicitly does not query runtime config or dial Chat; JEV confidence0.71 says `native_inventory` in the test name risks implying live inventory. Renamed the test to `profile_inventory_projects_from_provider_list_without_router_chat`. Native-prefixed profile field/type/API names and `native auth-profile` remain accurate, scoped configuration identifiers with explicit caveats (JEV confidence0.77). No runtime behavior changed; tests not run.

## J30 — network backend terminology

`net.rs:476,481,512,547` calls the actual `op_network::rtnetlink` interface native netlink/rtnetlink; these accurately name the concrete kernel netlink API. `NativeNetBackend` at 1043, 1046, 1375, and 1382 is the production adapter: rtnetlink for links/addresses/routes and OVS operations through the D-Bus client. JEV preserved the protocol/backend terms (confidence0.41/0.79). JEV confidence0.89 says `partial_failure_reports_completed_native_operations` overstates the MockBackend test, which records fixture operations only; renamed it `partial_failure_reports_completed_backend_operations`. No tests run.

## J31 — OCI provider API terminology

`oci.rs:1` identifies the backend as the native Incus REST API. The plugin uses Incus `/1.0` endpoints through `IncusApi` and explicitly distinguishes the provider from Docker/runc bundle tooling. JEV confidence0.95 preserves this scoped platform API term.

## J32 — OpenFlow owner observations

`openflow.rs:51,78` called flow-table members “native” observations. The adapter uses a D-Bus proxy to the controller and sends live flow mutations only through the canonical compare/exchange bridge adapter; the serialized observation describes owner-confirmed backend state, not an OS-native API. Initial JEV confidence0.18 was weak. Exact source-supplemented JEV confidence0.91/change0.94 supported replacing the terms with “compare/exchange owner” and “backend observation”. BEFORE excerpt is `BEFORE/openflow-owner-terms.txt`; no behavior changed.

## J33 — OpenFlow obfuscation owner wording

`openflow_obfuscation.rs:8-10,412` used “canonical native controller” and “native switch readback.” The source provides a canonical controller/method contract but generic state apply/verify/checkpoint operations explicitly fail because no canonical backend supports generated rules. JEV confidence0.91/change0.94 for replacing native with canonical controller; exact-context choice confidence0.83/change0.89 for narrowing schema text to owner-reported state. Header now says “method contracts describe” rather than claiming installed behavior; schema description says “owner-reported state.” BEFORE excerpt: `BEFORE/openflow-obfuscation-native-terms.txt`. No behavior changed.

## J34 — OSCAL method heading

`oscal_subid_registry_methods.rs:1` called the plugin's Rust/bridge-dispatched methods “Native methods.” The module implements service methods over the sealed catalog and Cozo runtime documents; “native” could imply OS-native behavior. Initial JEV confidence0.55/change0.70 was weak; source context confirms this is a plugin service adapter, so the scoped heading is now “Service methods.” BEFORE excerpt: `BEFORE/oscal-subid-methods-native-term.txt`.

## J35 — OVSDB terminology

`ovsdb_bridge.rs:51,363,464` said “native owner,” “native initial monitor observation,” and “native OVS bridge topology.” The implementation receives OVSDB monitor snapshots through `OvsdbDbusClient` and treats OVSDB as source of truth; “OVSDB owner/observation/topology” makes the backend explicit. Initial JEV choices were weak for preserving “native” on the snapshot (0.37) and schema (0.27); exact source semantics supported the narrower terms. The request contract's “native owner” was also made explicit. BEFORE excerpt: `BEFORE/ovsdb-bridge-native-terms.txt`.

## J36 — OVSDB topology references

`ovsdb_topology.rs:1,119,123` described the source as “native IDL/topology”; `ovsdb_topology_tests.rs:97` named invalid encodings “native types.” These are OVSDB `OvsdbSnapshot` tables, rows, and RFC 7047 JSON encodings. JEV favored explicit OVSDB at confidence0.82 for source/error terms and 0.73 for the test name. Reworded to “OVSDB IDL/topology/types.” BEFORE excerpt: `BEFORE/ovsdb-topology-native-terms.txt`. Test behavior unchanged.

## J37 — remaining backend contexts

Grouped exact-source decisions: `procfs.rs` native snapshot wording is accurate for the `sysinfo` host snapshot, not proof of direct `/proc` reads. `qdrant_native_tests.rs` names actual Qdrant server/protocol fixture behavior. `rtnetlink.rs` names the kernel rtnetlink interface. Confidence was respectively 0.80, 0.79, and 0.76. Runtime-document wording was judged misleading/uncertain at 0.43; tched filesystem semantics were low-confidence (0.09); service wording and snowball/shared-socket contexts returned incomplete/low-confidence answers. Those terms remain unresolved, not endorsed.

## J38 — tched, platform and remaining terminology

`xray.rs` host-native lifecycle/HandlerService/account-protocol terms were preserved as direct Xray API/backend vocabulary (confidence 0.88). The `tched_router.rs` and `tched_router_generation.rs` results were weak (0.38 and 0.30), so no semantics were inferred from them; these files remain owner-overlap review. Legacy router method names were assessed accurate (0.64). `wg_opdbus.rs` and `wgcf.rs` judgments were low-confidence (0.02 and 0.67); no wording change. `runtime_documents.rs` remained uncertain at 0.58. Service-method semantics were insufficient at 0.67. Transcript package/install semantics remained insufficient at 0.64. No terms in these weak/insufficient cases are treated as accepted findings or changed.

## J39 — Incus identity-binding documentation and obsolete fixture

Coordinator follow-up explicitly authorizes source-established Incus corrections within this crate. `identity_sled.rs` now states that the committed durable identity binding determines the Incus instance name and distinguishes canonical SID, numbered ID-N, and configured chatbot alias as separately validated forms. This supersedes the earlier overlap-based deferral for this exact documentation hunk. The obsolete create fixture that expected `create_instance` to POST and return `created=true` was removed because current dispatch refuses creation absent the durable MutationEngine allocation owner. Its BEFORE evidence is in assignment `BEFORE/identity-sled-instance-binding.txt` and `BEFORE/incus-native-create-fixture.txt`. No tests run.

The previously recorded alias-test name `bound_identity_power_requests_target_only_the_pinned_native_name` was absent from the complete initial `incus.rs` source read; this earlier finding is a false positive, not a pending source-snapshot discrepancy. No test rename was appropriate.

## J40 — shared socket, runit and wgcf precision

One exact-context choice call: `shared_unix_socket_methods.rs:1` “Durable native adapter” was selected for change (confidence 0.73; change probability 0.82); revised to “Durable adapter,” while preserving the concrete registration-metadata scope. `wgcf.rs:190,193,196` “uses admitted native methods” was selected for change (confidence 0.97; change probability 0.98); the StatePlugin fallback methods themselves bail, while admitted plugin methods are the dispatch path, so the errors now say “admitted plugin methods.” `service/methods.rs:1,32` was selected for change at weak confidence 0.30; exact source shows a root-guarded D-Bus provider and Runit supervisor, so those concrete terms replace “native.” The first Snowball judgment was mis-keyed and low-confidence; exact-source follow-up J41 supports changing “native methods” to “plugin methods.” `runtime_documents.rs` is source-grounded in CozoDB APIs, but no persistence or runtime guarantee is inferred. BEFORE excerpts for changed prose are in assignment `BEFORE/shared-unix-socket-methods-native.txt`, `BEFORE/wgcf-native-method-errors.txt`, and `BEFORE/service-methods-native-terms.txt`.

## J41 — Snowball method comment

Exact source context `snowball_plugin.rs:566-568` distinguishes two EventChain queries from seven methods dispatched through `SnowballMethods`. JEV confidence 0.94 (change probability 0.96): “native methods” is ambiguous; changed only that comment phrase to “plugin methods.” BEFORE excerpt: assignment `BEFORE/snowball-native-methods-comment.txt`. No tests run.

## J42 — runit service type local identifier

At `service/methods.rs:76-83`, the D-Bus `GetUnitType` response is checked against the expected service definition type and returned as `definition_type`. Exact-context JEV confidence 0.91 (rename probability 0.94): local name `native` was ambiguous; renamed to `reported_type` without changing wire behavior. BEFORE excerpt: assignment `BEFORE/service-methods-native-local.txt`. No tests run.

## J43 — source coverage sufficiency

After sequential EOF review of all 171 readable inventory files plus disposition of the one deleted entry, and reconciliation of later candidate occurrences into the ledger, JEV selected complete coverage (confidence 0.94; complete probability 0.96). This supports static-audit completion; it does not convert weak semantic judgments into acceptance or establish build, test, runtime, catalog, or deployment status.
