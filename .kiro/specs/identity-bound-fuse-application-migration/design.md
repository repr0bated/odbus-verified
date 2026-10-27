# Design — Identity-bound FUSE Application Migration

Status: SPECIFICATION ONLY (revision 5). Evidence-linked. No implementation/build/test/
reseal/deploy/restart performed. Citations are to source as checked out in
`/srv/git/odbus-verified`; runtime/deployed state is not asserted except where marked as a
prior SIGNALS/HANDOFF receipt. Companion: `requirements.md` (R#/C#/PB#/NG#/U#),
`tasks.md` (T#). Spec id: `identity-bound-fuse-application-migration`.

Revision 3 removed recovery / durable cross-stage workflow correlation / lost-result
reconstruction scope (NG10). Revision 4 renamed the spec and applied six review fixes plus
three corrections. Revision 5 (this version) puts clean-cutover removal in the deployed build
and defers acceptance/parity/smoke/formatting/Clippy/tests until after deployment and an
explicit owner request (R19.6). This changes execution order, not architecture or scope.

---

Historical revision 1 → 2 changes (owner architecture correction + nine review findings):

- **Architecture correction (established threetched mechanism).** The
  seal→type→D-Bus→FUSE→execute pipeline is ESTABLISHED. Revision 1 implied it was missing
  and framed "reach threetched inside the sealed plugin backend" as new design — that
  framing is WITHDRAWN. The pipeline (verified in source):
  1. Seal-time schema refresh from the verified binary: `threetched.rs::prepare_schema_capture`
     → `capture()` → `ThreetchedSurface::capture_with_config(&binary, &config_dir)`, with
     manifest format/name/sha256/version all checked.
  2. Typed methods sealed into the blob: `schema_from_capture` inserts one `MethodDecl` per
     captured command with per-method capability/subid (`command_policy`); `op-blob` seals.
  3. D-Bus surface generated from the typed methods (schema-derived; `grpc-expert`: the
     plugin IS the schema, `build.rs` + dynamic reflection, dispatch via
     `PluginService.CallMethod`).
  4. Sealed methods projected through the identity-bound FUSE namespace: `tched_fs` protocol
     + `threetched-fs` mount expose `plugins/<enc>/operations/<enc>/call` +
     `plugins/<enc>/schema.json`.
  5. Explicit binary execution on an admitted method: `threetched.rs:~322-345` builds the
     `Command` — `chpst -u jeremy:jeremy <binary>` when the bridge is root, else direct exec
     as the configured owner, with a cleared env and `THREETCHED_CONFIG_DIR`.
  The migration MOVES the HTTP/WS callers onto THIS mechanism; it does not build a new one.
- F1: one authoritative Svelte whole-application UI contract + registry migrated together
  (§3.5, §3.11); not a loosened validator.
- F2: concrete chatbot execution-boundary tasks, not UID docs only (§3.9, §3.12).
- F3: caller→mount lifecycle resolved before the executor (§3.3, §3.13); `Lungs::Credential`
  reframed as mount-daemon config.
- F4: explicit human→chatbot response handoff (§3.7).
- F5: UI-generation model-selection path included (`json_render_generation_dispatch.rs`,
  §3.6).
- F6: discovery/schema provenance corrected — `describe()` returns descriptors, `schema()`
  returns the full sealed schema, catalog hash lives in `view.json` (§3.4).
- F7: complete caller inventory + dependency graph (§2.0, §4). (Cross-stage correlation/
  recovery, previously under F7, is REMOVED per NG10 — see §3.8.)
- F8: WG peer-exclusion + interceptor-minting tracked consistently (§3.9, §3.15).
- F9: terminology rename task; preload completion corrected (§0.1, §1).
- Rev 3 (owner): recovery / durable correlation / lost-result reconstruction removed from
  scope (NG10); §3.7 step 6, §3.8, §3.12 updated to retain only receipts / honest
  failure-unknown reporting / cleanup / no-replay.
- Rev 5 (owner): T13/T14/T18 cutover removal occurs before build/deployment; no parity gate
  or compatibility path remains. Acceptance/parity/smoke/format/Clippy/tests are deferred
  until deployment and an explicit owner request (R19.6/T21). A post-deployment fix loops
  through build/deploy before re-verification; no deploy or tests are authorized here.

### 0.1 Terminology
This design uses **threetched** / precise component names, not "native". Source
identifiers (`NativeGenerationAdmission`, `current_generation_selection`, A2A client) are
cited verbatim and scheduled for scoped rename/removal (T18). No `native="threetched"`
alias is introduced (NG8).

---

## 1. Preload manifest (corrected — finding 9)

CORRECTION to revision 1: revision 1 said "remaining skills read only to a one-line
purpose" yet declared preload COMPLETE — that did not satisfy "read every skill
completely." Revision 2 re-ran the preload reading EVERY `SKILL.md` end to end.

- Count: the directory holds **96** skill directories (revision 1's "94" undercounted).
- Every `SKILL.md` was READ COMPLETE (top to bottom); 0 blockers; 0 broken symlinks.
- Tooling note: skills symlinked outside the workspace are not readable via the `read_file`
  tool (workspace-escape deny) and were read via read-only shell (`readlink -f` + `cat`)
  against the resolved real paths; the 18 in-repo (real-dir) skills read directly.
- The full per-file completion manifest (96 rows: name | resolved path | lines | READ
  COMPLETE/BLOCKED) is recorded in the task evidence and summarized here; it is not pasted
  in full to keep this document readable. Load-bearing constraints carried into this design
  are unchanged from revision 1's summary plus deeper reads of: fuse, kernel-fuse-diagnosis,
  btrfs, incus, grpc-expert, json-render, svelte, sveltekit, svelte-code-writer,
  application-security, rust-memory-optimization, web-design-guidelines, quality-auditor,
  verification-before-completion, receiving/requesting-code-review, agent-self-evaluation,
  systematic-debugging, writing-plans.

Preload status: COMPLETE (96/96 read in full). Blockers: none.

Key constraints applied: plugin-IS-the-schema + sealed blob (grpc-expert); flat spec +
schema-hash-pinned promotion + server-side ACL on generated/streamed UI (json-render);
`@json-render/svelte` renderer + registry (svelte/svelte-code-writer); every FUSE request
must get a reply, bounded workers, EIO = worker failure (fuse/kernel-fuse-diagnosis);
identity storage is Btrfs subvolumes, no add/remove on login (btrfs/incus); measure before
allocation changes, RAII charges, 256-slot budget (rust-memory-optimization); default-deny
server-side authz (application-security); no completion claim without fresh evidence,
separate implemented/built/deployed/tested (verification-before-completion,
quality-auditor); root-cause before fixes (systematic-debugging).

---

## 2. Current architecture (as-is, with evidence)

### 2.0 Source-backed application caller inventory (finding 7, R17.1)

Callers of the direct threetched transports (HTTP A2A for chat; the stateless-generation
WebSocket for UI generation) and the model/provider/config/generation operations that this
migration must account for:

| Caller (source) | Method/surface | Transport today | Disposition |
|---|---|---|---|
| `mutation_engine.rs` Chat arm (~5831) ControlPlane | `tched_router.Chat` | `control_plane_routing::complete` → `chat_under_guard` HTTP A2A `:8084/a2a/<alias>` | MIGRATE (T11) |
| `mutation_engine.rs` Chat arm (else) | `tched_router.Chat` non-control-plane modes | `route.client.complete_under_guard` → stateless-generation WebSocket | MIGRATE (T11/T13) |
| `json_render_generation_dispatch.rs:~157` | `json_render.generate_spec` | `for_agent_alias("dashboard")` + `complete_under_guard` → **WebSocket** generation adapter (`tched_router_generation_native.rs`, `tokio_tungstenite`) | MIGRATE (T11c, selection via control-plane) |
| `tched_router_runtime.rs` config/gateway | `PatchConfig`/`config_set`/`config_patch`/gateway GET/POST/PUT/PATCH/DELETE | HTTP `:8084` | MIGRATE or justify maintenance (T13) |
| `mutation_engine.rs` model routes (`GetModelRoutes`/`ResolveRoute`/`Set*Model`…) | model/provider routing reads/writes | via runtime client | MIGRATE or justify (T13) |
| generation jobs (`generation_jobs_dispatch`, `is_generation_method`) | generation-job methods | via the WebSocket generation adapter | MIGRATE (T13) |
| `control_plane_routing.rs` | control-plane chat/provider | HTTP A2A / WebSocket `complete` | MIGRATE (T13) |
| `threetched.rs` executor | binary exec of admitted methods | **already the established FUSE/D-Bus mechanism** (not HTTP/WS) | KEEP (target of migration) |

Transport note (correction): `complete_under_guard` is NOT HTTP — it drives the stateless
generation **WebSocket** (`tched_router_generation_native.rs`: "Bounded adapter for the
pinned binary's stateless generation WebSocket"). Chat (`chat_under_guard`) is HTTP A2A
`message/send`. Both are direct transports to be moved onto the FUSE mechanism.

T3b finalizes this inventory from a fresh grep; every row is assigned to a migration task
(T11/T13) or a justified non-application maintenance task (R2.4).

### 2.1 Chat / UI-generation path (the direct-transport callers to move)

1. Door admits `PluginService/CallMethod` for `tched_router.Chat` (private-arg gate lists
   `tched_router` `Chat`/`PatchConfig`/`config_set`/`config_patch`,
   `large_language_model.stream_generate`, `json_render.generate_spec`, gallery,
   `gemma_brain` UI-workspace methods — `mutation_engine.rs`).
2. `chat_mode_routing::resolve` loads grants, re-reads the persisted mode
   (`project_dispatch::current_chat_context`), builds `ResolvedChatRoute { mode, context,
   client: TchedRouterRuntimeClient, input }`, stamps `with_sealed_identity(credential_for(
   actor, session, genesis))`. Role modes (ControlPlane, Accountability) clear
   provider/model + dedicated alias; ordinary modes keep selection + `from_env()`.
3. Dispatch arm (`mutation_engine.rs:~5831`): ControlPlane →
   `control_plane_routing::complete(args, before_prompt, false)`; else →
   `route.client.complete_under_guard`.
4. `control_plane_routing::complete`: `TchedRouterRuntimeClient::for_agent_alias(
   CONTROL_PLANE_AGENT).with_sealed_identity(before_prompt.sealed_identity())`, re-check
   admission, `apply_native_selection`, then `chat_under_guard`/`complete_under_guard`.
5. `TchedRouterRuntimeClient` (`tched_router_runtime.rs`): `DEFAULT_ENDPOINT =
   http://127.0.0.1:8084`; `chat_under_guard` POSTs JSON-RPC `message/send` to
   `{endpoint}/a2a/{alias}`, sealed id as header `x-opdbus-sealed-id-bin`.
6. UI generation (`json_render_generation_dispatch.rs`): pins `tched_router`, builds
   `TchedRouterRuntimeClient::for_agent_alias("dashboard")`, reads
   `runtime.current_generation_selection()`, `complete_under_guard(ChatInput{provider,
   model, messages,…})`, then `decode_generated_canvas(&generated.content, &validator)`.
   `complete_under_guard` drives the stateless-generation **WebSocket** adapter
   (`tched_router_generation_native.rs`, `tokio_tungstenite connect_async_with_config`) —
   NOT HTTP A2A.
   => This path selects the **"dashboard" alias**, NOT the control-plane chatbot's
   authoritative selection (finding 5; R6), and uses the WebSocket transport (correction).

### 2.2 Response commit (designated-responder semantics; finding 4)

`chat_response_dispatch.rs`:
- `respond_to_user` validates any canvas (`validate_canvas` →
  `read_manifest_pinned_schema_document(RENDERER)` + `PinnedCanvasValidator`), then
  `commit_response`.
- `commit_response` (`:534-...`): `let claim = engine.chat_turns().claim(turn_id,
  &caller.principal_id)` (:541) → `verify_owner` → `commit_claimed`/`commit_turn`: durable
  row, SHM publish, turn-hash chain, `response_committed` signal. Output =
  `RespondToUserOutput{response_id, conversation_id, turn_index, turn_hash, revision,
  canvas_schema_hash}`. Dispatch spawned so commit survives cancellation.
- `chat_response_turns.rs:3-8`: the bridge is the ONLY `turn_id` issuer; it opens a turn
  for a verified human and designates the agent principal that may answer; a response is
  admitted only while pending+unexpired, only from the designated responder, only once;
  pending turns are in-memory and a restart drops them (late response refused, fail closed).

=> The commit caller is the designated responder (an agent), and it commits into the asking
human's scope. Generation (2.1) and this commit are separate ops today; R8 reconciles them
into one responder-committed workflow.

### 2.3 FUSE surface (established)

- `tched_fs` plugin owns the protocol/contract (3 read methods: `get_protocol`,
  `get_contract`, `get_receipt`); `FilesystemProtocol::default()` fixes the invocation
  convention (`plugins/{plugin}/operations/{method}/call`, `O_RDWR`, `fsync`,
  seek(0)+read receipt, `metadata.json` declaration, `automatic_retry=false`).
- `MountedNamespace` (`threetched-fs/src/mounted.rs`): `open`/`open_for_mode` OPEN and
  VERIFY an existing mount (fstype `fuse.3tchedFS`, `.3tchedfs.json` with `format=3tchedfs/1`,
  `instance_uuid`, typed `mount_identity`; `view.json` with `format`/`view_id`/
  `source_catalog_hash`/`source_catalog_generation`/`objects`, object-count cross-check).
  They do NOT create or rebind a mount. `verify()` re-pins before/after each op.
  - `describe(selected)` → `{methods: [{plugin_id, method, path, declaration}]}` — method
    DESCRIPTORS, no full schema, no `schema_hash` field (`mounted.rs:314-332`).
  - `schema(plugin)` → the full sealed `PluginSchema` from `plugins/<enc>/schema.json`
    (`mounted.rs:184-...`); the per-plugin object hash is `view["objects"][plugin]`.
  - `call`/`call_with` → one `O_RDWR` handle, write args, fsync (executes), seek(0),
    `read_bounded(MAX_RECEIPT)`, `terminal_receipt` (Err on `error`; else validate
    plugin/method/subid/capability/side_effect/result). `invoke_with` requires
    `--confirm-side-effects` for non-Read; no auto-retry.
- FUSE engine (`fuse_fs.rs`): `TypedFuse` holds `Lungs` (`Socket{path,session}` or
  `Credential{bus_address, credential: PathBuf}`), `budget`/`slots`/`jobs`, `target_guards`
  (per-`(workspace_id,plugin_id)` write CAS → `EBUSY`). `open_handle` takes a slot
  (`slots.take(1, HANDLE_LIMIT)`), runs the call on a spawned `3tchedfs-operation` thread
  (`catch_unwind`), returns the receipt on the fd. Production dispatch = `run_lungs_call` →
  `dispatch::call_as_identity(bus_address, plugin, method, decl, args, credential)` (D-Bus)
  or `call_over_socket(path, session, …)`. The `#[cfg(test)]` threetched-binary branch is
  for lifecycle fixtures; production uses the sealed backend route.
  - `Lungs` is MOUNT-DAEMON CONFIG: `main.rs` takes `--mount <path>` to use an existing
    namespace, and `--bus_address` (default `unix:path=/run/opdbus/session-bus.sock`) +
    a credential file; the credential is a FILE PATH baked into the daemon, not a per-request
    argument (finding 3).
- Budgets (`fuse_buffers.rs`): `INPUT_LIMIT=1 MiB`, `DOCUMENT_LIMIT=16 MiB`,
  `TOTAL_LIMIT=64 MiB`, `HANDLE_LIMIT=256`; `Charge::Drop` subtracts (RAII).
- EIO: `open_handle` maps `refresh_for_inode` failure → EIO (the live-value path backing
  `value.json`); lungs-call failures → `Err((EIO, …))`. So value.json EIO can coexist with
  readable schema/metadata (R3.5).

### 2.4 Browser admission (door only; finding — net-new adapter)

`op-web/src/door_proxy.rs`: same-origin gRPC-Web connector; `door_connector` forwards
gRPC-Web to the TLS door, preserving OIA1/SID1. No browser→FUSE path exists. UI contract
(`3tched.com/AGENTS.md`): one door, one credential, `operation.v1.PluginService/CallMethod`,
method shapes from the sealed schema (`zcall help`), backend behind `ChatAdapter`, reference
catalog in `catalog.ts`, runtime subset in `chatCatalog`/`chatRegistry`.

### 2.5 Canvas profiles reject interactive UI (finding 1, C5)

- `json_render_chat_canvas.rs:7-9`: "distinct from `canvas_profile`… admits no actions,
  bindings or state: a chat canvas is a static composition." `:149-156`: rejects
  `HOST_COMPONENTS` (`antigravityChatContainer`, `tchedRouterPicker`, `chatMessage`) and
  requires `component.events.is_empty()`. Profile id `3tched-chat-canvas-v1`. The UI exports
  its catalog; `json_render_chat_canvas_contract.rs` embeds the export; this module seals
  the `chat_canvas_profile` field + validator.
- `json_render_canvas_profile.rs:166-184`: validates the interactive-Gemma profile but
  requires exactly 8 components, exactly one action (`setState`, `RendererTarget::React`,
  `mutates_state`), and excludes `antigravityChatContainer|tchedRouterPicker|pageHeader|
  chatMessage` host controls.
- `chat_response_dispatch.rs:159-167`: `validate_canvas` uses
  `PinnedCanvasValidator::from_manifest_pinned_schema`.

=> The backend structurally forbids the requested whole interactive UI (overlays, actions,
bindings, state). R16 requires a NEW authoritative Svelte profile, not a loosened gate.

### 2.6 threetched executor runs as jeremy (finding 2, C6)

`threetched.rs:~322-345`: re-verifies the sealed source digests, builds the CLI argv, then
`if geteuid().is_root() { Command::new("/usr/bin/chpst").args(["-u","jeremy:jeremy"]).arg(
binary) } else { … run as configured owner jeremy }`, with `env_clear()` +
`THREETCHED_CONFIG_DIR`. Comment: "Match the existing tched-router service owner… never a
caller-supplied identity." => A root bridge runs the child as `jeremy`, NOT in the chatbot
container as root. The designated chatbot execution environment is not the current target
(R13).

### 2.7 Existing generation/UI building blocks (reusable)
`json_render_generation.rs` `METHOD = "generate_spec"`; `json_render_canvas_profile.rs`
`PinnedCanvasValidator`; `json_render_chat_canvas*`; `json_render_gallery.rs` (12 methods,
hash-pinned promotion); `schema_renderer_dispatch.rs`; `ChatMode::UiRender` exists and is a
non-dedicated-alias mode.

### 2.8 Identity minting discrepancy (finding 8, C3)
`interceptor.rs::anchor_verified_human_session`: on first arrival with no current anchored
record, calls `engine.mint_and_store_genesis(session_id, wireguard_pubkey)`. Contradicts the
settled decoy-mints/interceptor-anchors model. Tracked SEPARATELY (R15; §3.15).

---

## 3. Proposed architecture (to-be)

### 3.0 Principle
Collapse "two transports" into one by moving the HTTP/WS callers onto the established
threetched FUSE mechanism (§0). The application's only path to plugins/schemas/model ops/UI
generation is the caller's identity-bound mount, reached from the browser through an
authenticated adapter, and from the mount to the backend through the existing D-Bus lungs
dispatch, ending in the established explicit threetched binary execution.

```
Browser (3tched.com, Svelte + @json-render/svelte, one SID1/OIA1)
  │ gRPC-Web operation.v1.PluginService/CallMethod       (unchanged door contract)
  ▼
op-web door_connector ──TLS──▶ op-grpc-bridge :8090 interceptor (verify OIA1/SID1)
                                   │ resolve caller principal/session/grants (R1.1)
                                   ▼
                         FUSE EXECUTOR (new) — resolve caller→mount (R11), open+verify
                                   │ MountedNamespace::open_for_mode (existing mount)
                                   ▼
                         MountedNamespace.call(plugins/<p>/operations/<m>/call)
                                   │ same-fd write→fsync→seek→read receipt (R1.5/R9)
                                   ▼
                         FUSE engine run_lungs_call → D-Bus call_as_identity
                                   ▼
                         sealed threetched binary backend / unrelated json_render contract
                                   ▼
                         explicit threetched binary exec in the CHATBOT execution env (R13)
```

### 3.1 Established mechanism — reuse with sealed command-binding invariant
Reuse seal-time capture, blob sealing, D-Bus generation, FUSE projection and direct
binary execution. R18.9–R18.12 / §3.1.1 require command-binding agreement; existing
pipeline presence is not acceptance of every invariant. No second service/schema/executor.

**Owner unified-authority correction (2026-10-24, R18.13):** threetched/zeroclaw
has ONE binary application schema/blob, `threetched`. A separately registered
`tched_router` 38-method application catalog is not an acceptable sibling authority
or future caller target. The former schema producer, inventory registration,
generation method catalog and direct exports/blob prototype have been removed;
MCP retains only independently owned gateway tool-group policy, not cloned binary
config contracts. Corrected full-registry isolated seal completed 2026-10-24 with
71 members and no tched_router. The canonical threetched blob contains 148 methods
and identical same-blob argv/input/output bindings (143 leaves plus five optional
parents). This is retained artifact evidence, not deployment/runtime acceptance.

Remove the competing authority, not merely its visible methods or name. Do not copy
its declarations into a handwritten threetched table. Existing callers/bridge arms
(`mutation_engine.rs:4999–5042`, Chat at 5732, selection at 5968) are migration gaps;
future application cutover must select actual sealed binary methods. Producer gaps
must be named explicitly; provider-only CLI is not automatically equivalent to
legacy Chat/jobs/route/config APIs. Any separate operational maintenance requires
a real non-application purpose, not relabeling those APIs. No broad transport/UI
cutover, deployment or unrelated edits are authorized by this bounded checkpoint.


**Owner U3 correction applied first (2026-10-23):** this is capture/passthrough,
not a missing API catalog to implement. `opblob::seal_plugins` explicitly calls
`threetched::prepare_schema_capture` before registry/schema construction. The
verified binary supplies the full `config schema` bytes and recursive CLI help;
`SurfaceSnapshot` retains those bytes, hashes and command contracts. Only those
captured commands populate threetched methods, and `schema.example` retains the
full snapshot inside the canonical blob. Runtime `sealed_schema`/`sealed_state`,
manifest-pinned catalog reads and `ThreetchedSurface::from_snapshot` consume that
authority; `from_snapshot` does not run discovery.

No new discovery/catalog implementation is needed. The director subsequently identified
a real producer-input provenance gap: patch0004 was applied without a digest pin,
build verification or emitted provenance field. The source correction adds mandatory
`PATCH_APPLICATION_GENERATION_CLI_SHA256` →
`patches.application_generation_cli_sha256`, refreshes the already-expanded inventory
pin, and enforces complete package provenance in build/apply/regeneration/golden helpers.
Seal/guest/runit manifest readers reject incomplete digest sets; package preparation
owns comparison to actual pinned patch bytes. Capture/passthrough and runtime catalog
authority remain unchanged. Neither inventory contents nor upstream base hashes changed.

Follow-up review found that clearing only the newly introduced key left omitted older
pins inheritable. All four helpers now clear and require every actually consumed input:
build/golden use six package digests plus five upstream/archive fields; apply uses six
digests plus the upstream commit; regeneration uses five retained digests (0002 is its
output). `MINIMUM_RUST_VERSION` is unused metadata, not an added enforcement condition.
The source-only behavioral fixture exercises omitted keys with legitimate inherited
values; it has not run. Prior source acceptance does not establish this follow-up's
execution acceptance. Broader migration is held for director review again.
Owner clarification: the verified binary is the sole schema producer. Config-schema
bytes are captured directly and verbatim. Recursive CLI help provides feature commands
and token constraints; `schema_from_capture` turns each discovered leaf into an actual
`PluginSchema.methods`/`MethodDecl` with args plus a process-receipt return; existing
sealed D-Bus/FUSE projections consume those declarations for validation and execution.
This path is present in source; it is not a new catalog. A process receipt is not the
feature's domain result, and CLI tokens are not domain value types. No semantic type is
inferred from config properties, command names or output samples. Static inspection
does not establish which commands the actual verified binary emits, any additional
producer-declared result semantics, or whether `stateless-generate` is present and
captured by a fresh seal. Those facts do not imply a new-emitter prerequisite. No binary
execution/seal was authorized or performed. The current source delta labels input and
result kinds in captured metadata and tests the existing feature→MethodDecl generator;
there are no transport/envelope changes.

The candidate `vendor/tched-router/patches/0004-application-generation-cli.patch`
uses the provider-only `StatelessGeneration` core rather than `agent --message`;
its bytes were preserved. Pinning that source is not proof of application/build,
verified-binary or sealed availability. Availability must arrive through a future
authorized real seal of the verified producer, never a hand-authored/schemars catalog.

The existing/disputed `invocation.rs` call-file envelope and all guest execution,
ticket callback, discovery consumer and UI cutover edits are separate migration
work. They are preserved uncommitted and **HELD at the owner's U3-first checkpoint**;
this clarification neither endorses their design nor credits them as U3 acceptance.
No build, capture execution, seal, deployment or test was performed for this correction.

#### 3.1.1 U3 executable command binding sealed with schema (R18.9–R18.12; T3)

At every seal, capture the verified binary's full config schema and recursive CLI
contracts. Each exposed binary-backed threetched method MUST have exactly one captured
callable binding: method ID, exact command/subcommand argv prefix, argument encoding
and constraints, and binary identity/provenance in the same canonical `PluginSchema`
document and blob. This is executable authority, not descriptive help/configuration.
The invariant does not require a new physical `MethodDecl` field or make unrelated
plugins shell out. A shared sealed method-keyed snapshot is valid only when consumers
enforce agreement; no duplicate catalog or separately mutable command authority.

**Current source evidence, not execution acceptance:** `threetched-surface.rs:54-70,
248-261` serializes `SurfaceSnapshot.commands[method]` with `argv`, captured help and
input/output contracts. `threetched.rs:131-138,180-204` puts that snapshot, selected
runtime binary path and provenance in `schema.example` and builds methods from the same
captured command set. `op-blob/src/lib.rs:84-99` serializes the FULL state-store schema;
`op-blob/src/blob.rs:179-188,268-294` hashes and seals that JSON, including `example`.
Thus `/example/binary` and `/example/surface/commands/<method>/argv` are already within
the schema's sealed bytes. `MethodDecl` itself has no executable-binding field
(`op-state-store/src/plugin_schema.rs:128-146`). `PluginSchema.example` is described
as documentation (`:293-295`), but this producer/executor uses its sealed contents as
the immutable discovery descriptor. The existing `threetched-cli/1` format and captured
constraints define token encoding; no new emitter is required. Do not mistake that
placement for external mutable state authority, or demand a redundant second mapping.

Production reads mounted `schema.example` (`threetched-fs/src/main.rs:344-374`);
`fuse_process.rs:36-69` verifies the sealed binary/manifest, restores `SurfaceSnapshot`
and invokes the content-checked executable descriptor directly. `from_snapshot`
(`threetched-surface.rs:267-304`) checks method ID against captured argv/usage and
reconstructed contracts against retained contracts; `arguments` (`:394-452`) looks up
the method key, starts with captured argv and encodes caller tokens. It parses SEALED
help, not live `--help`. The bounded source correction now passes the complete outer
MethodDecl map to `prepare_application_command`; `SurfaceSnapshot` compares its
cardinality, keys, names, args and returns before executable preparation. Source
and authored fixtures are not runtime acceptance. These checks do not cure the
separate competing `tched_router` application authority identified above.

Required consumer sequence: pin one canonical schema → select declared method and
unique captured binding → require matching method ID, args/encoding contract, argv and
binary provenance → validate caller arguments → verify executable identity → direct
argv invocation under existing identity/grant/guest-root admission. Unknown, missing,
ambiguous or mismatched bindings fail closed before invocation. Never infer command
words by splitting the method name, consult ambient config/aliases or a handwritten
table, rediscover help/schema, or interpolate shell command text. Caller values remain
literal tokens encoded per the sealed contract. Binary/catalog changes cannot silently
retarget the call, refresh its mapping or authorize replay.

Acceptance scenarios are R18.12(a–d): nested feature prefix; exact alias/token encoding;
unknown/missing/duplicate/cross-field mismatch rejection; binary/catalog-change pinning.
The generic process receipt remains `{stdout,stderr,exit}`; no semantic emitter,
new invocation envelope or recovery is introduced. Criteria are NOT RUN; tests wait
until deployment and explicit owner request (R19.6). The verified producer build
succeeded; binary feature capture/fresh-seal availability remain unverified.
The next seal is held for the unified-authority correction, not a broader cutover.



### 3.2 (reserved)

### 3.3 Browser→FUSE adapter (R1) + ingress routing boundary (finding 4, R10.5) [Q1]
Keep one door, one credential. The adapter is a bridge-side executor reached through the
existing door:
- For a mounted plugin operation, route through the FUSE executor instead of the in-process
  HTTP/WebSocket chat path.
- Derive the caller's identity from the verified interceptor context (never the request
  body) and resolve the caller's mount via §3.13 (R11).
- Enforce the mounted invocation convention; translate `MountedDiagnostic`/terminal error →
  gRPC `Status` + envelope `MutationError` (UI reads the status channel). No auto-retry.
- Path/plugin come only from the pinned catalog (R1.4).

**Ingress-only routing boundary (R10.5).** The design names two distinct layers with a
one-way edge, not a shared dispatch with a "no recursion" caveat:
- **Outer ingress adapter** = `fuse_executor` (new), the ONLY entry reached from the gRPC
  door (`door_proxy` → interceptor → executor). It opens the caller's mount and issues the
  mounted `call`. This is the sole ingress; it is not re-entrant.
- **Inner backend path** = `MountedNamespace.call` → FUSE engine `run_lungs_call` → D-Bus
  `call_as_identity` → sealed plugin backend → (threetched methods) explicit binary exec.
- **Edge rule**: inner FUSE-originated D-Bus calls return results to the engine/plugin; they
  SHALL NOT call back into `fuse_executor` (the outer ingress). The executor is not exported
  to, nor reachable from, the inner D-Bus plugin handlers. The separation is STRUCTURAL —
  the ingress entry function is a distinct module not referenced by the backend — not a
  client-controlled bypass flag and not merely a runtime "am I re-entrant?" check.
- T6 defines the executor entry point (ingress) and the backend return path, and asserts the
  backend cannot reference the ingress symbol (compile/call-graph check).

### 3.4 Discovery + schema provenance (finding 6, R3/R7/R16.5)
Corrected provenance (revision 1 overclaimed `describe()`):
- **Discovery**: `MountedNamespace::describe()` → method descriptors `{plugin_id, method,
  path, declaration}` for the caller's authorized catalog. It does NOT return a full schema
  or a `schema_hash`.
- **Plugin schema / target-method contract**: `MountedNamespace::schema(plugin)` → the full
  sealed `PluginSchema` (`plugins/<enc>/schema.json`), including typed inputs/outputs and
  capability per method. Use this to render interfaces and to pin action bindings to target
  contracts.
- **Renderer component/action profile**: the sealed Svelte UI profile (R16); the renderer
  catalog/registry is a separate pin from the target plugin contracts.
- **Catalog generation / mount instance / applicable pins**: `view.json`
  `source_catalog_hash` + `source_catalog_generation`; `.3tchedfs.json` `instance_uuid`;
  per-plugin object hash `view["objects"][plugin]`.
- A new discovery method that exposes the caller-authorized catalog + per-plugin schema
  references MUST be a plugin-owned method with typed input/output and a declared read
  capability, generated through the established pipeline (NG2); candidate owner: `tched_fs`
  (it already owns protocol/contract/receipt reads) with a new `get_catalog`-style read
  method — named, typed, capability-scoped in T7b. It MUST NOT be a client-invented
  endpoint or a bridge-only side channel.
- **Pin lifecycle (R7.2)**: a stored spec is revalidated against BOTH the renderer profile
  AND the target-plugin method contracts (via `schema(plugin)`); renderer-only revalidation
  cannot prove a changed/removed target method is still valid. Removed method → binding
  fails validation → regenerate.

### 3.5 Whole-UI generation + the authoritative Svelte UI contract (findings 1, R4/R16)
- One authoritative Svelte whole-application contract covers: the generated canvas, the
  chatbot overlay panel, the model-selection overlay panel, and their actions/events/
  bindings/state. These are declared as first-class trusted catalog components (the exact
  interactive features `json_render_chat_canvas.rs`/`canvas_profile.rs` reject today), not
  smuggled past a widened validator.
- Authored once in `3tched.com`, exported, imported into the backend, sealed as a profile
  (mirroring the existing chat-canvas export/import/seal — `json_render_chat_canvas_contract.rs`).
- Both the generation validator (`json_render_generation_dispatch.rs` `decode_generated_canvas`)
  and the response-commit validator (`chat_response_dispatch.rs:159-167`) validate against
  this ONE sealed Svelte profile.
- The Svelte `chatRegistry` implements exactly the contract's components/actions; an action
  absent from the registry fails validation (R4.2). Action admission enforces server-side
  ACLs (R4.4). Overlays live inside the generated application's catalog/registry (R5.2); the
  only fixed non-generated surface is bootstrap chrome (R4.5).
- Action bindings are pinned to target plugin contracts (§3.4, R16.5).

### 3.6 UI-generation model selection = control-plane authoritative (finding 5, R6)
Replace `json_render_generation_dispatch.rs`'s `for_agent_alias("dashboard")` +
`current_generation_selection` with the control-plane selection read (the same selection
`control_plane_routing::complete` enforces: output provider/model must equal the observed
selection). Pin the selection per generation; a concurrent change/revocation fails the
in-flight `check_current()` re-check (existing pattern) rather than mixing selections. No
second model/provider state; no stale dashboard selection; no hidden fallback.

### 3.7 Reconciled response lifecycle + human→chatbot handoff (findings 4, R8/R10)
One workflow, with the bridge as turn issuer and the chatbot as designated responder:
1. **Reserve + record**: the bridge opens a turn for the verified human in one of that
   human's conversations (`chat_response_turns.rs` — bridge is the only issuer), records the
   human's request as a human-authored turn (`commit_human_turn`), and designates the
   chatbot principal as responder.
2. **Bind delivery + prompt delivery (finding 3, R8.7)**: the designated-responder chatbot
   must learn both that a turn is pending AND what was asked. Source today provides only
   half of this:
   - `list_pending_turns` (`chat_response`, requires `chat_response.respond`) returns
     `ListPendingTurnsOutput { pending_turns: Vec<PendingTurnInfo>, pending_count }`, and
     `PendingTurnInfo` is METADATA ONLY: `turn_id`, `conversation_id`, `turn_index`,
     `owner_key`, `owner_principal_id`, `opened_at` — no prompt text.
   - `get_conversation` / `get_turn_response` read the CALLER's own principal scope, so the
     chatbot cannot use them to read a human's prompt.
   The design therefore specifies a NEW authorized input-delivery method, owned by the
   `chat_response` plugin, typed, and scoped to a single designated turn:
   `get_pending_turn_input { turn_id } -> { prompt, context }` (illustrative name/shape;
   finalized in T11b). Its contract:
   - Capability: `chat_response.respond` (responders only), AND an admission check that the
     caller is the DESIGNATED RESPONDER for that exact `turn_id` (the same designation the
     commit `claim` enforces) — not a general read of humans' conversations.
   - Payload: only the asked prompt and the minimal context needed to answer that one turn
     (e.g. prior turns of THAT conversation required for continuity), never a cross-human or
     cross-conversation read.
   - It does NOT widen `chat_response.read`, does NOT expose other humans'/conversations'
     content, and adds no recovery (NG10). The method is defined as a plugin-owned typed
     method and projected through the established seal→D-Bus→FUSE pipeline (NG2), like T7b.
   - The initial-page generation trigger is this same flow seeded by a bridge-opened
     "initial UI" turn designating the chatbot, so the first canvas is generated through the
     identical designated-responder path rather than a special-case bootstrap generator.
3. **Generate**: generation runs in the chatbot execution environment (§3.12) via the
   threetched FUSE mechanism, pinned to the control-plane selection (§3.6).
4. **Commit**: the chatbot commits via `respond_to_user`; `claim(turn_id,
   chatbot_principal)` succeeds only because the chatbot is the designated responder; the
   canvas is validated against the sealed Svelte profile; the turn commits into the asking
   human's scope; SHM published; chain extended; `response_committed` emitted.
5. **Read**: the human reads the committed result via `get_conversation` /
   `get_turn_response` in the human's own scope, in the ordinary course — the answer lands
   in the human's scope WITHOUT granting the human root or the chatbot's identity (PB3c/d).
6. **Failure (not recovery)** (R8.6/R9): failure between generate and commit, turn expiry,
   or bridge restart → fail closed (pending turns are in-memory; a late response is refused).
   An uncertain outcome is reported as unknown with its terminal diagnostic and the operation
   cleans up (no auto-replay). A turn already durably committed is simply answered by that
   existing `chat_response` commit; the spec adds NO cross-stage recovery, re-drive, or
   lost-result reconstruction (NG10). The human SHALL NOT impersonate the chatbot and
   turn-ownership checks are NOT weakened (R10.4).

#### 3.7.1 Source cutover integration: guest responder and existing call metadata

This is caller integration (T6/T11/T13), NOT a change to the completed U3
capture authority or its 148 binary methods.

- Human request submission belongs to `chat_response`, which records the human
  message, reserves the designated response and returns the ordinary submission
  receipt. It grants no threetched generation capability to the human. The guest
  responder, not a bridge-constructed identity, performs scoped input read,
  selection observation, provider-only generation and response commit.
- OD-90 owner correction: BOTH model context and response commit belong to the
  requesting human's private conversation. The chatbot is the responding actor,
  never the history owner. Only `get_pending_turn_input` for the bound human
  conversation/turn supplies prior content, plus tools authorized for that turn.
  Do not inject chatbot/plugin history or agent-private memory unless the human
  explicitly pulls it into this conversation. Caller-supplied cross-conversation
  context is rejected; the same human owner/conversation/turn is revalidated at
  input read, final provider admission and response commit.
- Reuse `3tchedfs.call/1` and its existing `CallMetadata`/ticket machinery; no new
  envelope, version, dispatcher or binary catalog. Optional `response_turn_id`
  is an UNTRUSTED pending-turn reference, never identity, permission or capability.
  Admission resolves the kernel caller first, verifies designation/expiry, and
  pins the requesting human's session, admission contract/grant, source and mount
  incarnations. Direct GuestKernel calls acquire the same proof and lease cleanup
  as ticketed ingress; host/unbound calls and invented ticket IDs gain no route.
- The responder must supply the turn binding for every response operation;
  omission must not silently turn response generation into an unbound effect.
  Standalone non-response config operations retain their existing exact method
  admission; they do not acquire permission to read human input or answer a turn.
  Response generation carries the validated binding into the existing application
  ticket; its FINAL prompt callback revalidates designation, human admission,
  source/mount/model pins and calls existing `begin_generation` exactly once.
  Failure/cancellation/unknown outcome never reset that single-use admission.
- Observe `agents.control_plane.model_provider`, then its exact
  `providers.models.<family>.<alias>.model` through captured `config.get --json`.
  The producer's `{path,value}` JSON is inside the sealed process receipt, not
  a new MethodDecl return schema. The provider-only command retains the exact
  family.alias and model through captured expected-provider/model arguments.
- No outer selection/context/publication lock is held across a mounted call:
  the canonical threetched backend alone owns its selection guard. Final producer
  selection comparison and callback pinning preserve admission without re-entering
  that mutex. No alternate guard, bypass, sequential config.set replacement for an
  atomic config.patch, streaming/jobs fiction or tool-agent equivalence.
- Existing guest mounts are `/run/mount/3tchedfs` and `/run/mount/threetched`;
  their systemd units run in-container root. Responder launch must be hooked only
  into the verified chatbot-specific guest lifecycle, never every identity.
  Existing host relay `ProcessPendingTurn` has no receiver; a principal argument
  and host loopback MQTT default do not establish guest identity or connectivity.
  Source launch/endpoint/credential evidence is required before deployment edits;
  no provisioning, grants, network changes or runtime execution are authorized.

### 3.8 Failure reporting (recovery/correlation removed — NG10)
Recovery, durable cross-stage workflow correlation, per-stage recovery authority, and
lost-result reconstruction are OUT OF SCOPE (NG10). This design does NOT define correlation
ids spanning human request → generation → commit, nor a recovering identity per stage.

What is retained (safety properties, no recovery required):
- Each operation independently returns its ordinary terminal receipt/diagnostic on its own
  descriptor (the `tched_fs` convention), including on denial.
- An uncertain mutation is reported as unknown and is never automatically replayed (R9.3).
- Resources are released on every path (RAII `Charge::Drop` + handle-slot drop; spawned
  worker `catch_unwind`) (R9.4).
- A disconnected browser is NOT reported as a successful cancellation of the underlying
  mutation and triggers no replay; the outcome is whatever the operation actually reported,
  read from the ordinary committed state or the returned receipt. The spec makes no promise
  to reconstruct a result that was lost (NG10).

### 3.9 Identity model + execution boundary (findings 2 & 8; R12/R13/PB1-3)
- **Four roles kept distinct (PB3)** on every operation: execution owner/container/UID
  (§3.12); requesting principal/session/group/capabilities (interceptor-resolved, re-admitted
  via `readmit_call`); designated responder (chat_response turn); private-state owner (the
  asking human).
- **UID mapping MEASURED, not asserted (R13.5/PB1)**: T1 records the real idmap from the live
  container (`incus config show <chatbot> --expanded`, `/proc/<init>/uid_map`). Revision 1's
  assumption that container-0 maps to a specific unprivileged host range is WITHDRAWN until
  measured.
- **WG peer-exclusion (R12)**: T2 locates the existing enforcement point or, if absent, T17
  specifies a concrete enforcement task; peer provisioning is separated from application
  authentication so the legitimate chatbot application identity is still admitted (R12.3/.4).
  No keys/peers changed (NG1).

### 3.10 Recursion / lock-order safety (R10)
Acyclic order for a browser→FUSE op:
```
interceptor (no engine lock) → FUSE executor opens caller mount
  → open_handle: take slot (atomic), per-object write CAS (EBUSY if held)
    → spawned 3tchedfs-operation thread: run_lungs_call → D-Bus call_as_identity
      → sealed plugin backend (MutationEngine folds result, commits SHM under state_publication)
        → (threetched methods) explicit binary exec in chatbot env
```
- The FUSE callback thread does not hold `state_publication` while awaiting D-Bus; work runs
  on the spawned worker; the FUSE reply is delivered on the fd (no reply starvation).
- A bridge handler servicing a FUSE op MUST NOT re-open the same mount under lock (no
  FUSE→bridge→same-FUSE). Plugins read/write canonical SHM, not the caller's mount.
- Ingress edge (R10.5, §3.3): the inner backend (D-Bus plugin handlers) does not reference or
  call the outer `fuse_executor` ingress entry; the separation is structural (distinct module
  not reachable from the backend call graph), not a bypass flag. This is what makes the order
  above acyclic by construction.
- Same-object contention → `EBUSY` (R10.3). Honor budgets (NF3).

### 3.11 UI-contract migration pipeline (finding 1; R16)
Export (3tched.com) → import (`json_render_chat_canvas_contract.rs`-style embed) → seal
profile → generation validator + response-commit validator both consume the sealed profile
→ Svelte registry implements it → action admission server-side. Tasks T9a–T9f.

### 3.12 Chatbot execution boundary (finding 2; R13) — positive statement
**The admitted chatbot workload executes with effective UID 0 inside the designated chatbot
container through its supported execution mechanism. The bridge's host UID does not establish
that container identity or grant additional authority to the requesting caller.**
- Today the executor (`threetched.rs` command build) runs the child as host `jeremy` via
  `chpst`/configured owner — i.e. NOT in the chatbot container. The migration changes WHERE
  the admitted threetched/UI workload is dispatched: into the designated chatbot container,
  where it runs as that container's root (effective UID 0 in-container), which maps to a
  measured host subuid (never host UID 0).
- The supported entry mechanism MUST be the sanctioned one (not host privilege escalation,
  not ad-hoc `nsenter`, not running other identities' work): T13b determines it from the
  live container/service topology and the identity invariants, and documents execution owner
  (the chatbot container, UID 0 in-container) vs requesting principal (the human) vs
  designated responder vs private-state owner — all distinct.
- Acceptance (R13.5) VERIFIES BOTH facts: (a) container identity — the child is a process of
  the designated chatbot container (namespace/cgroup membership); (b) effective UID — the
  child's in-container effective UID is 0 (`/proc/<pid>/status`), while its host-visible UID
  is the measured unprivileged subuid. The bridge's own host UID is irrelevant to the
  caller's authority (PB3/R13.2).
- Chatbot-unavailable (R13.4): an operation needing the chatbot execution environment fails
  closed with a diagnostic; it is not silently run under another identity. (This mirrors the
  OD-90 / SIGNALS observation that when the chatbot's mount lane is absent it cannot act —
  treated as fail-closed, not a reason to route the human through an operator mount.) The
  operation reports failure/unknown accurately and cleans up; no recovery (NG10).

### 3.13 Caller→mount lifecycle (finding 3; R11)
Replaces revision 1's assumed "open caller-scoped `Lungs::Credential`".
- **Lookup (R11.1)**: verified caller (OIA1/SID1 → principal/session) → identity→container→
  mount resolved authoritatively from the identity subsystem / Cozo identity records, not
  from the request body.
- **Ownership + provisioning (R11.2)**: the identity/provisioning owner owns the mount; the
  external host provisioner adds a blank leaf once at creation; login/start/stop do NOT add/
  remove Btrfs members. The executor OPENS an existing mount (`open_for_mode`); it does NOT
  create or rebind one.
- **Binding verification (R11.3)**: beyond `MountIdentity::Credential` mode, cross-check the
  mount's `.3tchedfs.json` `instance_uuid` + declared identity against the verified caller,
  so a mount cannot serve the wrong caller. (`open_for_mode` already checks the mode;
  binding-to-caller is the added check.)
- **Refresh/revocation/concurrency/replacement (R11.4)**: `Lungs::Credential` is daemon
  config (a credential FILE); do NOT swap a shared credential file per request. A revoked or
  replaced mount fails closed (`verify()` re-pin detects an instance/catalog change →
  "mount changed; do not retry automatically"). Concurrent requests share the mount daemon;
  per-object write CAS serializes writers (`EBUSY`).
- **Absent mount (R11.5)**: fail closed with a diagnostic; no operator-mount fallback, no
  implicit chatbot impersonation, no on-demand membership.
- **Admission preserved (R11.6)**: OIA1/SID1 constraints hold end to end; the mount does not
  replace door admission.
- **Open item U1/Q2**: whether a supported per-caller mount exists / can be opened for an
  ordinary human without login-time membership creation. If not, R1/R2 for non-chatbot
  callers is blocked until the provisioning owner supplies the mechanism.

### 3.14 (merged into 3.7)

### 3.15 Interceptor minting reconciliation (finding 8; R15)
Tracked SEPARATELY (not a prerequisite): no requirement here depends on removing server-side
minting to function — discovery, generation, commit, and the executor all work under the
current anchored-session model. If the reviewer deems it a prerequisite, the precise
dependency and missing authorization must be stated. Any implementation is identity work
requiring explicit authorization (NG1); T15 is the separate task.

### 3.15b WireGuard peer-exclusion enforcement (finding 6/R12; defines T17)
T2 investigates whether the "chatbot WG-server-key identity is not provisioned/admitted as a
peer" policy is enforced in source. Two outcomes, resolving the previously-dangling T17:
- If T2 CITES an existing enforcement point (function + how it is exercised), T17 is CLOSED
  with that citation + acceptance evidence, and no new task is created.
- If T2 finds NO enforcement, T17 is a CONDITIONAL task: specify the enforcement that
  separates network-peer provisioning from application authentication (so "not a peer" does
  not reject the legitimate chatbot application identity), gated on explicit identity
  authorization (NG1 — no keys/peers changed). T17's acceptance: the chatbot identity is
  refused as a WG peer while still admitted for application operations (R12.4).
T17 is a defined node in the dependency graph (tasks.md), not a dangling reference. It may
run independently of unrelated implementation; this is a scheduling decision, not a waiver
of R12. Full-spec completion requires the R12 acceptance evidence, from existing enforcement
or an explicitly authorized implementation. Missing required evidence or authorization keeps
R12/T17 explicitly blocked; it does not authorize an identity change.
T2 source finding (2026-10-23): inspected `WireGuardOwner::new` and
`WireGuardOwner::dispatch_wireguard("add_peer")` in
`crates/op-plugins/src/state_plugins/decoy_network/wireguard_owner.rs`, plus
`run_decoy`/`DecoyNativeService` admission in `crates/op-grpc-bridge/src/decoy_network.rs`.
The owner is constrained to an explicit decoy interface allowlist; `add_peer` validates the
interface and peer key, then applies the peer to the kernel. The inspected peer-provisioning
path does not reject a peer whose key is the configured chatbot WG-server key. Decoy service
authentication is a distinct application-admission path and is not peer-exclusion evidence.
No key, peer, topology, or identity policy was changed. T2 therefore records a source gap and
T17 remains conditional/blocked pending explicit identity-change authorization; R12 acceptance
evidence remains required at T21. This is source inspection, not runtime acceptance evidence.

### 3.16 FUSE handle lifecycle (finding 5; R20) — provenance first
Generic R9 cleanup is not sufficient; this is dedicated handle-lifecycle work on the
established 256-slot FUSE engine (`fuse_fs.rs` `open_handle`/`slots.take`, `fuse_buffers.rs`
`Budget`/`Charge`/`HANDLE_LIMIT=256`).
- **Provenance before diagnosis (R20.1)**: compare the DEPLOYED binary's provenance with
  source before diagnosing the reported handle symptom; the symptom may be deployed/source
  drift, not a current-source bug. Do NOT assume the illustrative "flush-before-removal" bug
  exists without evidence (systematic-debugging: root-cause before fixes).
- **Cases with acceptance (R20.2)**: (a) sequential reuse past 256 directory opens (slots
  reused, not exhausted); (b) controlled saturation to `HANDLE_LIMIT` then drain (257th →
  `ENOMEM`; release drains; reuse resumes); (c) mixed file/directory slot occupancy; (d)
  failed opens leak no slot/charge; (e) client exit mid-op releases handles/charges on
  teardown; (f) duplicate releases are idempotent (no double subtract of a slot/charge);
  (g) daemon teardown with in-flight ops — charge ownership moves with the spawned job and
  is released exactly once.
- **Bounds + RAII preserved (R20.3/R20.4)**: keep `HANDLE_LIMIT`/byte budgets and the RAII
  `Charge::Drop`+slot-drop accounting; do NOT grow the table to conceal a leak and do NOT
  evict active handles. The `Buffer`/charge moved into a spawned operation (per
  `fuse_buffers.rs` design intent) is released once when that job completes/fails, even after
  RELEASE removes the handle-table entry. Task T19.

---

## 4. Cutover, obsolete-path removal, dependency graph (R17/R19)

Order (details in tasks.md; implementation dependencies enforce R17.2):
1. Investigation/decisions: T1 (UID), T1b (exec env), T2 (WG), T3/T3b (threetched coverage +
   caller inventory), T4a–T4e decisions (mount lifecycle), reviewer gate T0.
2. FUSE executor + resolved caller mount (T4f, T5, T6) — additive; no parity tests or
   behavioral verification before deployment.
3. Discovery + schema provenance (T7, T7b); UI contract sealed (T9a–T9f).
4. Execution boundary (T13b) + responder handoff (T11, T11b) + control-plane selection
   (T11c) — prerequisites of whole-UI generation.
5. Assemble whole-UI generate→commit→render (T11) and layout (T12).
6. Implement the clean cutover: remove HTTP/WS application callers (T13), fixed-dashboard
   fallback (T14), and terminology to be removed (T18). These changes MUST be in the build
   that is deployed; no old/new dual path, shim, or test-pending rollback route is retained.
7. Build the cutover, then deploy through the separately authorized release path. Owner-run
   reseal is required for sealed schema changes. Build success is not acceptance evidence or
   deployment authorization.
8. Only after deployment and an explicit owner request, run T21 acceptance, parity,
   behavioral, and smoke checks against the deployed target. Run formatting, Clippy, and
   other source-based checks against only the source revision that produced that deployment;
   do not run any of these checks earlier. If a check finds a defect and source changes,
   build and redeploy the fix before rechecking. Report implemented/built/deployed/tested
   separately; unrequested checks remain not run.
T15 interceptor minting remains separately tracked.

Hard ordering: T13/T14/T18 cutover removal MUST land before the release build and deployment;
there is no pre-removal parity gate. Acceptance/parity tests and behavioral verification
are deferred until after deployment and explicit owner request (R19.6). Whole-UI generation
(T11/T12) still cannot precede T9 (sealed Svelte contract), T11b (responder handoff), T13b
(execution boundary), or T4f (mount resolution).

Independent sequencing is not full-spec completion: T17 (R12) and T19 (R20) may run alongside
unrelated implementation, but their required acceptance evidence remains part of the complete
migration (R19.5). Collect that evidence only under T21 after deployment and explicit owner
request. If required work, evidence, or authorization is outstanding, report that requirement
as blocked rather than declaring the full migration complete. T15 remains separate; this
clarification authorizes no tests, identity changes, or recovery.

Deployment/reseal: `./deploy.sh [--system]` + owner-run reseal only. The sealed Svelte profile
(R16) and any schema changes require an owner-run reseal to take effect.

The application acceptance stage is not promised to be recoverable (NG10): if a post-deploy
check exposes a code defect, correct the source, rebuild and redeploy before re-verification.

---

- G1 (U1/Q2): per-caller mount provisioning for ordinary humans is unverified (§3.13).
- G2 (R12/U2): WG peer-exclusion enforcement point not yet located (T2).
- G3 (R18/U3): bounded duplicate-authority retirement and fresh isolated seal are
  observed: threetched hash `cec4867a85ca66c33d1b1c37f1bc0bc11296584b755ce569aeb5f9c3d4db94b1`,
  148 methods, no sibling tched_router. Existing old callers/fixtures and missing
  producer operation semantics remain explicit gaps. No renamed/copied catalog,
  broader cutover, provider generation or deployment; runtime remains unaccepted.
- G4 (R13): the supported mechanism to enter the chatbot execution environment is not yet
  determined from live topology (T13b); the design constrains it (no host escalation) but
  does not yet name it.
- G5: `ChatMode::UiRender` exists but is not wired to a FUSE generation path; the assembly is
  new wiring, not an existing flow.
- G6 (R16): the interactive Svelte profile does not exist yet; the static/React profiles must
  be replaced, which is an owner-run reseal.
---

## 6. Design self-check (agent-self-evaluation)
- Accuracy: every as-is claim cited to source read this session; the revision-1 overclaims
  (threetched mechanism "missing", `describe()` returns full schema/hash, container-0 UID
  mapping) are explicitly withdrawn and corrected. Runtime/deploy state not asserted.
- Completeness: all 9 findings mapped to design sections + tasks (requirements §7); the four
  PB3 roles, the mount lifecycle, the responder handoff, and the UI contract are each
  addressed concretely.
- Known weak spots surfaced, not hidden: U1 (per-caller mount), G4 (exec-env mechanism), G3
  (threetched coverage) — each gates a dependent requirement and is flagged.
