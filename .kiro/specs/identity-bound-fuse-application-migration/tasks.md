# Tasks — Identity-bound FUSE Application Migration

Status: SPECIFICATION ONLY (revision 5). Dependency-ordered implementation tasks.
Nothing implemented/built/deployed/tested. Repo policy: implement → build → deploy; tests,
Clippy, fmt, `/verify`, parity and behavioral verification run only after deployment and
only on explicit owner request. Deployment is `./deploy.sh [--system]` (btrfs send/receive);
reseal (`opblob seal-shm`+`persist`) is owner-run. Build/deploy authorization remains
separate; approval of this specification does not authorize either.
Spec id: `identity-bound-fuse-application-migration`.

Legend: `[B]` backend `/srv/git/odbus-verified`, `[U]` UI `/srv/git/3tched.com`. Each task:
files/symbols · contract · acceptance evidence. Maintain per task when work begins:
`implemented: no | built: no | deployed: no | tested: no`.

Reuse the ESTABLISHED threetched mechanism (seal→type→D-Bus→FUSE→binary execution). Do NOT
build a second D-Bus service, duplicate schema, or replacement execution framework (NG2).

Revision 4 added T17 (WG peer-exclusion, conditional), T19 (FUSE handle lifecycle);
strengthened T6 (ingress routing boundary), T11b (responder input-delivery contract), and
T13b (positive root-execution). Revision 5 sequences clean-cutover removal before the
required build/deployment and defers acceptance tests/verification until post-deployment
and explicit owner request (R19.6). Rename remains not scope expansion.

---

## Phase 0 — Investigation / decisions (no code changes)

### T0. Reviewer gate (satisfied for Phase 0; implementation prerequisites remain)
Review disposition (2026-10-23, Codex / OMP): revision 4's six prior findings are closed at
specification level; the reviewer gate is satisfied for READ-ONLY Phase 0 investigation.
Use the existing door/bridge-side adapter design (Q1); resolve U1/Q2, execution-entry and
coverage prerequisites before dependent implementation. Q3/minting remains separate.
This review is not deployment, test, or identity-change authorization. The owner has directed
that implementation happen in OMP; follow R19.6: clean cutover → build → authorized deploy
→ acceptance/tests only after deployment and explicit owner request.

### T1. [B] MEASURE the real chatbot UID map + host privilege boundary (R13.5, PB1, finding 2)
Files: new `docs/src/architecture/identity-privilege-boundary.md` (docs only).
Contract: from READ-ONLY `incus config show <chatbot> --expanded`, the container
`/proc/<init>/uid_map`/`gid_map`, and identity-storage invariants, record the ACTUAL
container-UID-0 → host-subuid mapping and the host privilege boundary. Explicitly withdraw
any unmeasured container-0 = host-range claim.
Evidence: exact idmap ranges recorded; no asserted mapping without measurement. Read-only.

### T1b. [B] Determine the supported chatbot EXECUTION-ENVIRONMENT entry mechanism (R13.1-.4, finding 2)
Files: investigation across `threetched.rs` executor, the chatbot container/service
topology, identity invariants. Docs output appended to T1's doc.
Contract: determine the SUPPORTED mechanism by which an admitted threetched/UI method's
binary runs inside the always-running chatbot execution environment (its identity) — not as
bridge root, not merely host `jeremy`, not via host escalation/nsenter hacks, not under
another identity. Define chatbot-unavailable = fail closed with an accurate failure/unknown
diagnostic and resource cleanup (no recovery mechanism, NG10).
Evidence: named supported mechanism + the four-role (exec owner / requesting principal /
designated responder / state owner) separation documented. Read-only.

### T2. [B] Locate or specify WG peer-exclusion enforcement (R12, finding 8)
Files: identity subsystem + `interceptor.rs` session derivation/anchor.
Contract: CITE the existing enforcement (function + acceptance evidence) OR, if absent,
record a precise gap for T17. Separate network peer provisioning from application
authentication so the legitimate chatbot identity is not rejected.
Evidence: either a cited enforcement point + how it is tested, or a documented gap. No key/
peer/topology change (NG1).

### T3. [B] threetched coverage and sealed executable command bindings (R18, design §3.1.1, finding 6)
Files: `threetched.rs`, `threetched-surface.rs`, canonical `op-blob` serialization,
mounted schema projection and production `main.rs`/`fuse_process.rs` lookup. Actual
sealed-catalog/binary inspection remains separately authorized, not run by this update.
Contract: inventory captured {command, config schema, plugin.*, skills.*, model_routes,
native_config, MCP}; distinguish semantic contracts from CLI-token/process receipts.
Trace every exposed method to exactly one captured executable binding in the same
canonical schema/blob (R18.9–R18.12); preserve the existing capture/projection mechanism.
Evidence: source-backed coverage/binding trace in design §3.1.1; distinguish source,
binary, seal and runtime evidence. No mock, new catalog or semantic emitter.

**Priority owner correction (2026-10-24, R18.13): ONE binary application authority.**
`tched_router` must not remain a separately sealed application method/config catalog
or a target for future HTTP/WS caller cutover. The former authored schema,
registration, generation catalog and direct export/clone paths have been retired.
Corrected targeted opblob build and isolated full-registry seal completed2026-10-24:
71 members, no tched_router, threetched hash
`cec4867a85ca66c33d1b1c37f1bc0bc11296584b755ce569aeb5f9c3d4db94b1`.
No persist/live activation or deployment was needed or performed.

Bounded source/catalog removal is implemented, not a renamed/copied method table.
Canonical reader evidence observes 148 matching MethodDecl/command bindings:
143 callable leaves and five runnable optional parents; stateless-generate is
present with its captured argv/CLI token constraints and process-output receipt.
Full inventory/provenance/binding evidence is retained with implementation-result.md.
Old bridge/Chat/jobs/config callers and meaningful legacy success fixtures remain
owner-held migration gaps; [INFERENCE] affected test targets will not compile until
migrated. No tests were run and no runtime/provider semantics are claimed from seal.
STOP for owner/director review; no broader HTTP/WS/UI cutover or further source work.


Owner U3-first correction (2026-10-23): preserve the already-implemented
`prepare_schema_capture` → verified full `config schema` + recursive CLI capture
→ snapshot/example → canonical blob chain. Runtime remains sealed-catalog-only.
No new threetched method/config catalog, schemars semantic substitute or call-file
envelope is a T3 remedy. Provider-only coverage must come from the real binary's
CLI through this same refreshed capture; `agent --message` is not a substitute.
The vendor CLI candidate is source only: current verified-binary and sealed
availability are not established, and no build/seal/runtime check is authorized.
The documentation-only checkpoint was not accepted: an applied patch0004 was missing
from `source.pin`, build verification and artifact provenance. This producer-input
gap is now corrected in source with `PATCH_APPLICATION_GENERATION_CLI_SHA256` and
`patches.application_generation_cli_sha256`, strict whole-package preparation and
complete seal/runtime manifest digest requirements. Apply/regeneration helpers and
source documentation are updated; the stale inventory pin is refreshed without
changing the already-complete inventory or original base manifest. Rejection tests
are authored, not run. Capture/catalog implementation and patch0004 bytes are unchanged.
Only static SHA256 calculation was executed for this correction; no script, check,
build, artifact generation, seal or deployment. All broader source changes/status notes
below remain **HELD**, not U3 acceptance. Return this producer-provenance checkpoint
for director review; verified-binary/sealed semantic availability is still unknown.
Follow-up U3-first correction: director found omitted older required fields could still
inherit environment values. Build/apply/regeneration/golden now clear and immediately
require their complete actually consumed source.pin sets, not only patch0004. The
behavioral fixture exports legitimate values and deletes each required key separately
(11 build, 7 apply, 5 regeneration cases), plus changed-patch rejection per entry.
Fixtures invoke real entries and assert rejection diagnostics/no created work paths;
they do not inspect source text and are NOT RUN. No unused metadata requirement,
new helper/catalog, pin-byte change, digest calculation or command execution. Broad
source remains held pending director review of this follow-up, not execution acceptance.
Owner feature-method clarification: each recursively discovered binary CLI leaf is
already mapped by `schema_from_capture` to an actual `PluginSchema.methods`/`MethodDecl`
with captured help-derived token argument constraints and the existing generated
projection. The `{stdout,stderr,exit}` return is the process receipt, not semantic
feature output. A focused fixture now covers representative config/widgets/skills
methods and provenance labels; authored, NOT RUN. Capture metadata explicitly labels
the distinction; direct config schema capture and method schema shapes are unchanged.
No new semantic emitter, parallel catalog or envelope is required or added. Static
source does not establish the actual verified binary's complete feature set, any
additional result semantics, or fresh-seal `stateless-generate` availability; no
binary execution/seal was performed. This is a source mapping checkpoint, not U3
binary/seal/runtime acceptance. Broader source remains held.

Owner command-sealing requirement (2026-10-23; R18.9–R18.12, design §3.1.1):
commands for methods MUST be sealed WITH the schema. For each exposed binary-backed
threetched method, require matching method ID, exact captured command/subcommand argv
prefix, argument encoding/constraints and executable provenance in one canonical
sealed document. A shared sealed snapshot keyed by method is allowed; do not duplicate
it merely to put argv inside `MethodDecl`. No independently mutable command authority.

T3 binding acceptance criteria (authored only, NOT RUN):
- T3.a At each seal, full verified-binary config schema and recursive CLI contracts
  produce one captured callable binding per exposed method; no orphan or ambiguous
  binding. Record both method declaration and binding locations in the canonical blob.
- T3.b Nested feature invocation uses its sealed multi-token argv prefix, not a split
  method name. Captured aliases, literal values, positional order and token encoding
  survive projection/dispatch; no shell interpolation, ambient alias/config resolution
  or handwritten table. Ordinary plugins are not required to invoke a binary.
- T3.c Consumers check declaration ID/args against captured ID/argv/contract/provenance
  before invocation. Unknown/missing/duplicate bindings, contract/argv/provenance
  disagreement and invalid arguments fail closed before effects. Presence in
  `example.surface` alone is not evidence that every consistency check exists.
- T3.d Runtime selects only the pinned sealed binding, validates its args and verifies
  the executable against sealed identity before direct invocation. Binary/catalog
  changes cannot silently retarget, rediscover or retry; preserve identity, exact-grant
  and guest-root boundaries. Parsing retained sealed help is not live rediscovery.

Current source retains `schema.example.surface.commands[method].argv` and
runtime binary/provenance at `schema.example` under one canonical schema hash.
The bounded U3 source correction now supplies the complete outer MethodDecl map
to runtime preparation and rejects missing/orphaned, duplicate/unordered or
ID/name/args/returns-mismatched bindings before binary preparation. Existing
snapshot help/argv/contract and executable provenance checks remain unchanged.
Fixtures for mismatch rejection and recursive leaves plus runnable optional
parents surviving canonical sealing are authored, NOT RUN.

The owner separately authorized fresh producer/targeted opblob builds and a NEW
isolated supported `seal-plugins` catalog for this U3 checkpoint, not live SHM
replacement, deployment or provider generation. Patch0004 application failures
were corrected against the exact pinned post-series source; required lock
reconciliation/build/seal evidence is recorded in the current implementation
receipt. No broader task or runtime acceptance follows from artifact sealing.
Generic stdout/stderr/exit remains process transport. Tests/checks remain deferred
until deployment AND explicit owner request (R19.6); recovery/broad migration held.


### T3b. [B] Finalize the source-backed application caller inventory (R17.1, finding 7)
Files: grep `TchedRouterRuntimeClient`, `for_agent_alias`, `complete_under_guard`,
`chat_under_guard`, `:8084`, `message/send`, gateway methods, `is_generation_method`, model-
route methods across `crates/op-grpc-bridge`.
Contract: every application caller of the HTTP/WS threetched path and model/provider/config/
generation-job ops is listed and assigned to a migration task (T11/T13) or a justified
non-application maintenance task (R2.4). Supersede design §2.0's provisional table.
Evidence: complete caller→disposition table; no unassigned caller. Read-only.

### T4a–T4e. [B] Caller→mount lifecycle DESIGN (R11, finding 3) — before the executor
Files: investigation across identity subsystem / Cozo identity records, `threetched-fs`
`mounted.rs`/`namespace.rs`/`main.rs`, mount provisioning helpers.
Contract (decide and document each before T4f/T5):
- T4a Authoritative identity→container→mount lookup (not from request body) (R11.1).
- T4b Mount ownership + supported provisioning lifecycle; no add/remove Btrfs members on
  login; executor opens, never creates/rebinds (R11.2, NG9).
- T4c Exact caller-binding verification beyond `MountIdentity::Credential` mode: cross-check
  `.3tchedfs.json` instance_uuid + declared identity against the verified caller (R11.3).
- T4d Credential refresh, revocation, concurrent requests, mount replacement — no shared
  credential-file swapping; revoked/replaced mount fails closed (R11.4).
- T4e Absent-mount behavior = fail closed, no operator-mount fallback, no chatbot
  impersonation (R11.5); OIA1/SID1 constraints preserved (R11.6).
Evidence: a written lifecycle design answering R11.1–.6; explicit resolution of U1/Q2 with
the provisioning owner (whether a per-caller mount is openable without login membership). If
U1 is unresolved, T5+ for non-chatbot callers is BLOCKED.

---

## Phase 1 — FUSE executor behind the existing door (additive)

### T4f. [B] Implement the caller→mount resolver per the T4a–T4e design (R11)
Files: new `crates/op-grpc-bridge/src/fuse_executor.rs` (resolver part).
Contract: resolve + open+verify the caller's existing mount under the verified interceptor
context; enforce binding verification (T4c); fail closed on absent/revoked/wrong-caller.
Evidence (owner-gated): valid caller → correct mount; wrong-caller mount → rejected; absent
→ fail closed; no operator-mount/chatbot fallback.

### T5. [B] FUSE executor: run one mounted operation + map receipts to gRPC (R1, R9)
Files: `fuse_executor.rs`; reuse `MountedNamespace::open_for_mode`/`schema`/`describe`/
`invoke`/`call`.
Contract: enforce the `tched_fs` invocation convention (one `O_RDWR` fd, write args, fsync,
seek, read receipt); translate diagnostics → gRPC `Status` + envelope `MutationError`;
honor `--confirm-side-effects`; no auto-retry; honor `HANDLE_LIMIT`/budgets.
Evidence (owner-gated): read op returns receipt; denied → PERMISSION_DENIED + diagnostic;
uncertain → terminal diagnostic, no retry; 257th handle → ENOMEM.

### T6. [B] Wire the executor into door dispatch for mounted plugin operations + ingress boundary (R2.5, R10, R10.5)
Files: `grpc_server.rs`/`plugin_method_routes.rs`/`mutation_engine.rs` dispatch; new
`fuse_executor.rs` as the ingress-only entry; gate so the old path still works until T13.
Contract: admission (interceptor grant + method decl + version + arg validation) stays the
gate; executor runs under the caller; assert the acyclic lock order (design §3.10).
Ingress routing boundary (R10.5): `fuse_executor` is the ONLY door-reached ingress entry;
the inner backend (D-Bus plugin handlers, `run_lungs_call`, sealed plugin dispatch) SHALL NOT
reference or call the ingress entry — structural separation, NOT a client-controlled bypass
flag and NOT merely a runtime "am I re-entrant" check. Name the ingress entry point and the
backend return path; inner FUSE-originated D-Bus calls must not re-enter the outer adapter.
T6 baseline: derive expected behavior from existing source/contracts before cutover without
running parity tests or invoking a pre-cutover test path. Acceptance checks (including parity,
lock-order, and EBUSY verification) are deferred to T21 after deployment and explicit owner
request (R19.6). Do not make them a prerequisite for T13 cutover; check the deployed
clean-cutover implementation only in T21.

---

## Phase 2 — Discovery + schema provenance + sealed Svelte UI contract

### T7. [B] Discovery returns the caller's authorized catalog (R3.1-.3, finding 6)
Files: `fuse_executor.rs` + `MountedNamespace::describe()` (descriptors) and `schema(plugin)`
(full sealed schema). Schemas from sealed blobs only; no `schemas/` dir / registry fallback.
Contract: list `plugins/<enc>/operations/<enc>/call` + `metadata.json`, and expose per-plugin
`schema(plugin)` + the `view.json` catalog generation/hash + object hash; visibility ≠
permission.
Evidence (owner-gated): discovery lists the caller's authorized plugins; render schema =
`schema(plugin)`; a visible-but-ungranted method is denied on invoke.

### T7b. [B] Define the discovery method as a plugin-owned typed method (R3.2, finding 6, NG2)
Files: `tched_fs.rs` (candidate owner) — add a typed `get_catalog`-style read method.
Contract: name the plugin owner, typed input/output, and read capability; generate its
transport surface through the ESTABLISHED pipeline (schema → seal → D-Bus → FUSE). Not a
client-invented endpoint or bridge-only side channel. Distinguish plugin schema / target-
method contract vs renderer profile vs catalog pins in the output shape.
Evidence (owner-gated): method appears in the sealed catalog + mounted namespace; typed I/O +
capability enforced; requires owner-run reseal to take effect.

Source status (2026-10-23, astra-migration-lead-3): added typed
`tched_fs.get_catalog` under the existing contract-read capability. Its backend
requires the current admitted guest context, reads both pinned mounted views,
compares projected schemas with current sealed authority and returns separate
view/catalog pins, plugin object/schema hashes, complete target-owned schemas,
operation/metadata paths and current exact-grant indications. It does not grant
operation permission or read `value.json`; state is explicitly `not_requested`.
The browser binary panel now discovers through this canonical method and invokes
the existing `CallMethod` with the discovered schema pin. The unbacked
`/api/threetchedfs/describe` and `/invoke` REST helpers were removed.
This establishes source wiring, not sealed availability or T8 live-state acceptance.
Built/sealed/deployed/tested: no.


### T8. [B] "Missing state ≠ missing schema" surfacing (R3.5)
Files: `fuse_executor.rs` result mapping aligned with `fuse_fs.rs` EIO-on-`value.json`.
Contract: value.json EIO → "state unavailable" while schema/metadata discovery succeeds;
plugin not dropped.
Evidence (owner-gated): simulated value.json EIO → schema-derived UI with unavailable
marker; plugin remains listed.

### T9a. [U] Author the interactive Svelte whole-application contract (R16.1, finding 1)
Files: `3tched.com` `src/json-render/catalog/catalog.ts` + components.
Contract: declare the generated canvas + chatbot overlay + model-selection overlay + their
actions/events/bindings/state as first-class trusted catalog components (the interactive
features the current backend profiles reject). Svelte/SvelteKit/Vite/`@json-render/svelte`
only.
Evidence (owner-gated): contract defines interactive overlays + actions; `npm run
type-check`/`lint`/`test`/`build` (owner-gated).

### T9b. [U→B] Export the Svelte contract and import it into the backend (R16.2)
Files: UI export; backend `json_render_chat_canvas_contract.rs`-style embed (new
Svelte-profile contract module).
Contract: authored once in UI, exported, embedded verbatim in the backend (mirror the
existing chat-canvas export/import). One source of truth.
Evidence (owner-gated): embedded export matches the UI export byte-for-byte; provenance
recorded.

### T9c. [B] Seal the new Svelte whole-application profile (R16.2, replaces static/React profiles)
Files: new sealed profile analogous to `json_render_chat_canvas.rs` / `json_render_canvas_
profile.rs`; retire/replace the static chat-canvas + React `setState`-only profiles that
reject interactive UI (C5).
Contract: the sealed profile admits the interactive application (overlays, actions, bindings,
state) as trusted declared components — not a loosened validator. Owner-run reseal to take
effect.
Evidence (owner-gated): a canvas with an overlay/action VALIDATES; one with an unregistered
component/action is REJECTED.

### T9d. [B] Point the generation validator at the sealed Svelte profile (R16.3)
Files: `json_render_generation_dispatch.rs` `decode_generated_canvas`/validator construction.
Contract: generation validates against the one sealed Svelte profile.
Evidence (owner-gated): generated interactive spec accepted; invalid rejected.

### T9e. [B] Point the response-commit validator at the sealed Svelte profile (R16.3)
Files: `chat_response_dispatch.rs:159-167` `validate_canvas` / `PinnedCanvasValidator`.
Contract: commit validates against the same sealed Svelte profile; records the pin.
Evidence (owner-gated): commit of an interactive canvas succeeds + records the pin; invalid
rejected before any claim.

### T9f. [U] Implement the Svelte registry for exactly the contract (R16.4, R4.2)
Files: `3tched.com` `chatCatalog`/`chatRegistry` + Svelte components; action admission.
Contract: registry implements exactly the contract's components/actions; action absent from
registry fails; server-side ACLs on actions (R4.4); run `svelte-autofixer` before finalize.
Evidence (owner-gated): unregistered component/action fails; a11y review (accessibility/web-
design-guidelines); type-check/lint/test/build.

Source status (2026-10-23, Astra lead; not runtime acceptance): UI
`references/application-profile.json` is the authoritative application definition,
consumed by `application-contract.ts` and imported verbatim as
`json_render_application_profile.json`. `export-application-profile.mjs` copies its bytes
without importing missing package build output; the exporter has not been executed.
The new sealed-field default and `ApplicationValidator` replace only generation and
response-commit consumers, retaining unrelated static/gallery contracts. They validate
bounded state, declared events/actions, graph shape and target PluginSchema arguments and
capability declarations against exact target hashes. The Svelte application registry
includes generated overlay/input/select/button controls; `invokePlugin` uses the existing
authenticated door with a target pin, not a new endpoint or hidden ChatPanel/ModelPicker.
Door admission and the owned backend task retain/recheck that target pin; grants remain
backend-owned. Existing fixed page composition, renderer-pin stored-spec lifecycle,
caller-bound FUSE ingress and the designated responder workflow still require their
separate source integration; these changes alone do not establish whole-app cutover.
No build, Svelte autofixer, tests, runtime checks, reseal or deployment was performed.

### T10. [B] Pin lifecycle: renderer AND target-plugin revalidation (R7, R16.5, finding 6)
Files: reuse the sealed Svelte validator + per-plugin `schema(plugin)`; stored-spec
revalidation path.
Contract: committed canvas carries renderer pin; on reseal, revalidate against renderer
profile AND target-plugin method contracts; removed/changed method → binding fails →
regenerate. Renderer-only revalidation insufficient.
Evidence (owner-gated): after a simulated hash change, a stored spec fails
renderer+target revalidation and regenerates; removed-method binding fails.

Source status (2026-10-23, astra-migration-lead-3): generation output now carries
the renderer schema hash and rechecks that renderer before returning. Explicit
stored turn/conversation reads revalidate renderer, target contracts and caller
grants without rewriting historical turn hashes. Browser application admission
reads the caller-mounted catalog, compares the bundled UI authoring profile with
the current sealed profile, checks all target pins and resolved action arguments
and current grants before display. Renderer dispatch now uses explicit validated
application ownership instead of component/event heuristics; application results
are excluded from static-gallery persistence. The single UI-authored profile and
both resolved validators also enforce select option fields.
Private-SHM retained-display/reseal invalidation and designated-responder
application persistence remain part of the continuing T10/T11 integration.
Source partial; built/sealed/deployed/tested: no.


---

## Phase 3 — Model selection, responder handoff, whole-UI generation

### T11c. [B] UI-generation selection = control-plane authoritative (R6, finding 5)
Files: `json_render_generation_dispatch.rs` (`for_agent_alias("dashboard")` +
`current_generation_selection`).
Contract: replace the dashboard-alias selection with the control-plane chatbot's
authoritative selection read; pin per generation; concurrent change/revocation fails the
in-flight `check_current()` re-check. No second model/stale dashboard/hidden fallback.
Evidence (owner-gated): generation uses control-plane selection; change → next gen changes;
revocation fails the in-flight re-check.

### T11b. [B] Responder handoff + input-delivery contract (R8.1, R8.7, R10.4, finding 3/4)
Files: `chat_response_turns.rs` (turn issuer/designated responder), `chat_response_dispatch.rs`
(`commit_human_turn`, `claim`), `chat_response_methods.rs` + `chat_response.rs` (new method
registration), chatbot-side turn read.
Contract — two parts:
1. Handoff: the bridge reserves the turn for the human, records the human request, designates
   the chatbot responder; turn-ownership checks unchanged; the human cannot commit the
   chatbot's answer.
2. Input delivery (R8.7): add a NEW plugin-owned typed method on `chat_response` —
   `get_pending_turn_input { turn_id } -> { prompt, context }` (name/shape finalized here) —
   that delivers the asked prompt + minimal context for exactly the designated turn. It
   requires `chat_response.respond` AND a designated-responder check for that `turn_id` (the
   same designation the commit `claim` enforces). It does NOT widen `chat_response.read`,
   does NOT expose other humans'/conversations' content, and adds no recovery (NG10).
   Projected through the established seal→D-Bus→FUSE pipeline (NG2). Rationale: existing
   `list_pending_turns` returns only `PendingTurnInfo` metadata (no prompt), and
   `get_conversation`/`get_turn_response` read the caller's own scope — neither delivers a
   human's prompt to the responder. Also define the initial-page generation trigger as a
   bridge-opened "initial UI" turn designating the chatbot (same path, no special-case
   bootstrap generator).
Evidence (owner-gated): a human request opens a turn designating the chatbot; the chatbot
reads ONLY that turn's prompt+context via the new method under `chat_response.respond`; a
read of a turn the caller is not the designated responder for is refused; a human attempt to
claim/commit the chatbot's turn is refused; no broadening of conversation read access.

Source status (2026-10-23, Astra lead): the typed `get_pending_turn_input` method and
non-mutating exact-responder registry lookup are implemented. Output is
`{turn_id, prompt, context:{conversation_id, turn_index, prompt_turn_hash, prior_turns,
prior_turns_truncated}}`; the bounded newest eight durable turns supply the immediate
HumanMessage predecessor plus up to seven same-conversation content records, including
prior canvases. Chain/head validation and explicit truncation prevent silent stateless
or complete-history claims.
Owner session, responder grant/seal and Pending/unexpired designation are rechecked after
asynchronous reads. No claim, recovery, broad conversation access or publication is added.
The initial-page trigger remains the bridge-opened initial-UI human turn specified above;
chatbot-side assembly remains T11, not established by this method alone.
Built: no; sealed/deployed: no; tested/verified: no (T21 deferred).

### T11. [B] Assemble the UiRender generate→commit workflow over the FUSE mechanism (R8, R2)
Files: wire `ChatMode::UiRender` + `json_render.generate_spec` + control-plane selection
(T11c) + responder handoff (T11b) + `respond_to_user`; generation runs in the chatbot
execution environment (T13b) via the FUSE/D-Bus/sealed-threetched mechanism — NOT the HTTP
A2A client.
Contract: (1) bridge reserves+records the human turn; (2) chatbot (responder) generates the
whole-UI canvas via the threetched FUSE mechanism using the pinned control-plane selection;
(3) `respond_to_user` validates against the sealed Svelte profile, commits into the human's
scope, publishes SHM, extends the chain, emits `response_committed`, returns the receipt;
(4) human retrieves from committed SHM.
Depends on: T5, T6, T9c-e, T11b, T11c, T13b, T4f. (R17.2 ordering.)
Evidence (owner-gated): one generate→commit turn yields receipt + `response_committed`;
output provider/model = control-plane selection; answer lands in the human's scope; the
human did not gain root/chatbot identity.

### T12. [U] Layout: canvas workspace + collapsible overlay panels (R5)
Files: Svelte shell + overlay components in the registry; runtime store for conversation +
model-selection state.
Contract: canvas is the main workspace; chatbot + model-selection overlay above it (not a
split dashboard); keyboard open/close/esc/focus-trap, visible focus, aria-live, responsive
collapse; state preserved while collapsed + across canvas re-render; bootstrap chrome
distinct from generation (no fixed fallback dashboard).
Depends on: T9f, T11.
Evidence (owner-gated): panels overlay canvas; keyboard + a11y pass; collapse preserves
state; failure shows error+regenerate, not a canned dashboard.

### T13b. [B] Execute the admitted chatbot workload with UID 0 inside the chatbot container (R13)
Files: `threetched.rs` executor dispatch (per T1b's determined supported mechanism).
Contract (positive statement): the admitted chatbot workload executes with effective UID 0
INSIDE the designated chatbot container through its supported execution mechanism. The
bridge's host UID does not establish that container identity or grant additional authority to
the requesting caller. Replaces the current host-`jeremy`-via-`chpst` execution. Exec owner
(chatbot container, in-container UID 0) stays distinct from requesting principal, designated
responder, and private-state owner. Chatbot-unavailable fails closed with an accurate
failure/unknown diagnostic and resource cleanup (no recovery, NG10). No host escalation /
other-identity routing (NG7).
Depends on: T1, T1b.
Evidence (owner-gated): acceptance VERIFIES BOTH — (a) container identity: the child is a
process of the designated chatbot container (namespace/cgroup membership); (b) effective UID:
the child's in-container effective UID is 0 (`/proc/<pid>/status`), with host-visible UID =
the measured unprivileged subuid (T1). Not run as host-jeremy-from-bridge; chatbot-unavailable
→ fail closed with a diagnostic (no recovery promised).

---

## Phase 4 — Clean cutover implementation (before build/deployment)

### T13. [B] Remove application HTTP/A2A/WS threetched callers (R2.1-.4, R19.1, finding 7)
Files: all callers from T3b's inventory — `control_plane_routing.rs`,
`tched_router_runtime.rs` application usage, the Chat arm in `mutation_engine.rs`,
`json_render_generation_dispatch.rs`, model-route methods, generation-jobs callers.
Contract: delete the application dispatch's HTTP/A2A/WS usage for generation/chat/model/
config/generation-jobs; retain only an explicitly infrastructure-scoped threetched health
surface, unreachable from the browser, each retained item justified (R2.4).
Depends on: T11, T11c, and every migrated caller's replacement implemented. The T13/T14/T18
removals are included in the build deployed for cutover; no pre-removal parity/test gate.
Acceptance evidence is collected only after deployment under T21.

### T14. [U] Remove the fixed-dashboard fallback (R4.5, R19.2)
Files: 3tched.com shell; keep connecting/loading/error bootstrap only.
Contract: no canned dashboard substitutes for generation; bootstrap chrome retained. This
clean cutover is included in the deployed build; no fallback dashboard or compatibility route
is kept pending tests.
Post-deployment evidence: generation failure shows error/retry, not a dashboard; fallback
removal is verified under T21 after explicit owner request (R19.6).

### T18. [B] Scoped terminology rename/removal (R14, finding 9)
Files: enumerate `native_*` identifiers (`NativeGenerationAdmission`,
`current_generation_selection`, A2A client naming, "native" routing prose).
Contract: rename/remove to "threetched"/precise component names; preserve source identifiers
only in citations until renamed; NO `native="threetched"` config alias (NG8).
Included in the cutover build before deployment. Post-deployment acceptance evidence (including
the identifier/alias inventory) is collected only under T21 after explicit owner request.

### T19. [B] FUSE handle-lifecycle implementation (R20, finding 5)
Files: `crates/threetched-fs/src/fuse_fs.rs` (`open_handle`, `opendir`/`releasedir`,
`slots.take`, spawned `3tchedfs-operation`), `crates/threetched-fs/src/fuse_buffers.rs`
(`Budget`/`Charge`/`HANDLE_LIMIT=256`). Source investigation first, then only evidence-backed
targeted work.
Contract:
- Preserve the 256-slot/byte limits and RAII ownership; do not grow the table to conceal a
  leak or evict active handles. Do not assert the illustrative flush-before-removal defect.
- Record deployed-binary/source provenance before diagnosing the reported handle symptom.
  Do not execute acceptance cases or probes before deployment; collect their evidence only
  under T21 after explicit owner request.
Depends on T0 and targeted source investigation. Any needed change is included in the cutover
build/deployment; a post-deployment finding needing a fix follows the rebuild, redeploy, then
reverify loop (R19.6). T19 may be implemented independently of UI work; R20 evidence remains
a full-spec completion condition, not a pre-deploy gate.

Source status (2026-10-23, Astra lead): current `fuse_fs.rs` already contains missing-handle
RELEASE/RELEASEDIR success, live inode checks, and removal before operation handling.
Those late existing edits are preserved, not attributed to this lead. Source inspection of
open/opendir/submit/drop and `fuse_buffers.rs` shows local/moved RAII charge ownership and
unchanged256-slot/byte bounds; no additional targeted change is justified by these paths.
No deployed provenance or symptom diagnosis is claimed. R20.1 and cases(a)-(g) remain
unverified/deferred to T21. Built: no; deployed: no; tested: no.

---

## Separate/conditional tracks — T15 separate; T17 satisfies mandatory R12

### T15. [B] Interceptor minting reconciliation (anchor-only) — SEPARATE identity task
Files: `interceptor.rs::anchor_verified_human_session` → remove `mint_and_store_genesis` from
the arrival path (decoy mints OIA1/SID1; interceptor anchors).
Status: SEPARATE, not a prerequisite (R15.1 default) — no task above depends on removing
server-side minting to function. Gated on explicit identity-change authorization (NG1). If
the reviewer reclassifies it as a prerequisite, they must state the precise dependency and
the missing authorization; this task then moves into the gating graph.
Evidence (owner-gated, with identity authorization): arrival with no prior decoy-minted
identity is not minted server-side; anchored sessions still verified; no key/identity
rotation.

### T17. [B] WG peer-exclusion enforcement — CONDITIONAL (R12, defines the former dangling ref)
Status: CONDITIONAL on T2's finding, with an authorization gate. This task now has a
definition and a dependency-graph node (it was previously a dangling reference).
- If T2 CITES existing enforcement: T17 is CLOSED by that citation + its acceptance evidence;
  no code change.
- If T2 finds NO enforcement: T17 specifies and implements the enforcement that separates
  network-peer provisioning from application authentication (so "not a peer" does not reject
  the legitimate chatbot application identity). Files: the identity provisioning/admission
  path + `interceptor.rs` (per T2's located point). Gated on explicit identity-change
  authorization (NG1); no keys changed, no peer/service created.
Independent scheduling only: R12 remains required for full-spec completion (R19.5). Missing
required enforcement, acceptance evidence, or identity-change authorization keeps R12/T17
explicitly blocked; this is not permission to skip R12 or broaden authorization.
Depends on: T2.
Evidence (owner-gated, with identity authorization if implementing): the chatbot
WG-server-key identity is refused as a WG peer while still admitted for application
operations (R12.4); or the cited existing enforcement with its evidence.

---


## Phase 5 — Build and deployment

### T20. [B+U] Build and deploy the complete clean cutover (R19.1–R19.3, R19.6)
Precondition: implementation, including T13/T14/T18 removal and source-driven fixes, is
complete. Build the prepared UI/backend artifacts using the repository's supported
target/release procedure; do not run test suites, Clippy, fmt, parity, smoke, or other
behavioral verification as a pre-deploy gate. Build is the required artifact-production step,
not acceptance evidence.

Deploy only through the separately authorized, supported release process; owner-run
seal/persist is needed for sealed contract changes. A request to implement this migration or
this task list is not deployment authorization. Record implemented/built/deployed separately.
Proceed to T21 only after the cutover deployment is recorded and the owner explicitly requests
post-deployment verification/tests.

## Phase 6 — Post-deployment acceptance and tests (owner-requested only)

### T21. [B+U] Verify the deployed cutover (R1–R20; R19.4–R19.6)
Gate: target deployment is recorded and the owner explicitly requested these checks. Run
acceptance tests, parity checks, smoke/behavior checks, formatting, Clippy, and `/verify` only
now; do not run them earlier. Runtime checks exercise the deployed target; source-based checks
apply only to the source revision that produced that deployment. Collect the acceptance-matrix
evidence here.

Include R12/T17 peer-exclusion evidence without changing identity/key/grants absent separate
explicit authorization, and R20/T19 deployed-binary/source provenance plus lifecycle cases
(a)–(g). Lack of authorization is a blocker, not permission to skip required acceptance.
Report each status as implemented/built/deployed/tested, and mark unrequested checks “not run,”
not passed. If a check requires a code fix, follow T20's build/deploy loop before rechecking.

## Phase 7 — Docs / signals / handoff (continuous)

### T16. [B/U] Update docs + record status (R19.4)
Files: `docs/src/...`; 3tched.com `proto/PROVENANCE` only if the proto is refreshed per its
AGENTS.md; SIGNALS.md; HANDOFF.md; WISHLIST.md OD-## if promoted.
Contract: report implemented/built/deployed/tested separately; mark superseded claims; no
"planned = done."
Evidence: HANDOFF row + dated entry; SIGNALS rows; no scaffold presented as completion.

---

## Dependency graph

```
T0 ─▶ T1 ─▶ T1b ─────────────┐
   ├▶ T2 ─▶ T17 [conditional; R12 acceptance at T21]
   ├▶ T3 ─▶ T3b               │
   ├▶ T19 [source work; R20 acceptance at T21]
   └▶ T4a..T4e (mount design; resolves U1/Q2)
                 │            │
                 ▼            │
               T4f ─▶ T5 ─▶ T6 ─┬─▶ T7 ─▶ T7b ─▶ T8
                                │
   T9a(U) ─▶ T9b ─▶ T9c ─▶ T9d, T9e ; T9f(U) ──────┐
                                │                   │
                                ├─▶ T10             │
                                ▼                   │
                 T11c ; T11b ; T13b ────────────────┤
                                ▼                   │
                               T11 ─▶ T12(U)  ◀──────┘   (needs T9c-f, T11b, T11c, T13b, T4f)
                                ▼
                 T13 + T14 + T18 [clean cutover]
                                ▼
                          T20 [build + deploy]
                                ▼ (deployment recorded)
                 T21 [owner-requested acceptance/tests]
```

Hard ordering rules (R17.2, R19.6):
- Whole-UI generation (T11/T12) cannot precede: the sealed interactive Svelte contract
  (T9c–T9f), responder handoff + input delivery (T11b), execution boundary (T13b), mount
  resolution (T4f).
- T13/T14/T18 removals are part of the clean-cutover artifact and MUST precede its build and
  deployment. No old/new dual path or shim is retained to await tests.
- Do NOT run acceptance tests, parity, smoke, formatting, Clippy, `/verify`, or behavioral
  verification before deployment. Run runtime checks against the deployed target and source-
  based checks against only the source revision that produced that deployment; run all checks
  only after explicit owner request (T21). The required pre-deployment build is not acceptance
  evidence.
- A post-deployment failure requiring a code change loops through rebuild and redeploy before
  any re-verification.
- T15 (minting) remains separate; its explicit identity-change authorization gate is unchanged.
- T17 implementation is conditional on T2; R12 acceptance is collected at T21, not a
  pre-cutover test gate. Missing authorization keeps it blocked.
- T19 may be implemented independently of unrelated UI/application work; do not diagnose the
  reported symptom or run lifecycle probes pre-deploy. Record deployed/source provenance and
  collect R20 acceptance at T21 after explicit owner request.
- Independent sequencing never waives R12/R20 (R19.5). Outstanding required work, evidence,
  or authorization remains explicitly blocked. This rule authorizes no identity changes or
  tests.
- One Cargo process at a time; build as `jeremy` on the shared target; workspace commands
  need an explicit workspace request; JEV owns Cargo routing. No new `.py` files.
- Reuse the established threetched mechanism; no second service/schema/executor (NG2).


---

## Failure reporting and resource cleanup (recovery/correlation OUT OF SCOPE — NG10)
This migration does NOT define cross-stage workflow correlation ids, a per-stage recovery
authority, or lost-result reconstruction. Retained safety properties only:
- Each operation returns its ordinary terminal receipt/diagnostic on its own descriptor,
  including on denial (R9.1).
- An uncertain-outcome mutation is reported as unknown and is never automatically replayed
  (R9.3).
- Handle slots and byte-budget charges are released on every path (RAII `Charge::Drop` +
  slot drop); spawned operation threads do not leak on failure/panic (`catch_unwind`) (R9.4).
- A disconnected browser is NOT reported as a successful cancellation of the underlying
  mutation and triggers no replay; the outcome is whatever the operation actually reported,
  read from the ordinary committed state or the returned receipt. No promise to reconstruct
  a lost result (NG10).

---

## Explicit non-completion criteria (anti-pattern guard)
Migration is NOT complete if:
- any application generation/chat/model/config/generation-job still reaches threetched over
  `:8084`/A2A/ws in the deployed clean-cutover artifact (T13);
- a FUSE facade internally re-opens that HTTP/ws (R2.3);
- the interactive UI is achieved by loosening a validator instead of the sealed Svelte
  contract (T9c);
- fixed host controls remain outside the generated application, or a fixed dashboard
  substitutes for generation (T14/R16.6);
- generated UI renders a component/action absent from the Svelte registry (T9f);
- an ungranted-but-visible method is invokable (T7);
- an uncertain FUSE outcome triggers an automatic retry, or a disconnect is treated as
  cancellation (T5/R9.4);
- a caller's op runs under an operator host-session mount or an implicit chatbot identity
  (T4f);
- the admitted chatbot workload does NOT run with effective UID 0 inside the designated
  chatbot container (e.g. it runs as host-jeremy-from-bridge, or its container identity /
  in-container UID 0 is not verified) (T13b/R13);
- UID mapping is asserted without measurement (T1);
- responder input delivery grants broad access to humans' conversations instead of the
  single designated turn's prompt+context (T11b/R8.7);
- the ingress adapter and inner backend are not structurally separated, or a
  client-controlled bypass flag is used to prevent recursion (T6/R10.5);
- the FUSE slot table is grown to conceal a leak, active handles are evicted, or the
  handle-lifecycle symptom is diagnosed without a deployed-vs-source provenance comparison
  (T19/R20);
- R12 lacks its required enforcement evidence, or required T17 implementation/authorization
  is unresolved, even if unrelated application work has landed;
- R20/T19 handle-lifecycle work or its required post-deployment acceptance evidence remains
  outstanding, even if the generated UI is implemented;
- tests/verification are reported as passed without explicit owner request and completed
  post-deployment checks (R19.6/T21);
- status is reported "done" without the stage-separated implemented/built/deployed/tested
  breakdown (T16).
