# Requirements — Identity-bound FUSE Application Migration

Status: SPECIFICATION ONLY. This document authorizes no implementation, identity/grant/key
change, mutating operation, build, test, reseal, deploy, or service restart. Revision 2
addressed the architecture correction + 9 findings; revision 3 removed recovery/correlation
scope (NG10); revision 4 renamed the spec and applied six review fixes plus three small
corrections; revision 5 sequences clean-cutover removal and build before deployment, with
acceptance tests and verification deferred until after deployment and explicit owner request.
The rename is NOT scope expansion: the spec already covers chat, generation jobs,
model/provider/config operations, caller-bound mounts, chatbot execution, responder handoff,
generated Svelte UI, and FUSE handle-lifecycle work.

Spec id: `identity-bound-fuse-application-migration`
Backend repo: `/srv/git/odbus-verified` (this repo)
UI repo: `/srv/git/3tched.com` (github.com/repr0bated/3tched.com; not in this repo)

Related prior specs/signals: `docs/specs/od-100-model-selection/*` (model selection),
OD-90 (chat_response / canvas), OD-46/OD-47 (mode isolation is not yet an execution
boundary). This spec depends on OD-100 selection and extends OD-90's canvas commit into
a full generate→commit→render→act workflow.

Revision history: rev 2 incorporated the owner architecture correction (the threetched
seal→type→D-Bus→FUSE→binary-execution mechanism is ESTABLISHED, not missing) and nine
findings; rev 3 removed recovery scope (NG10); rev 4 renamed the spec and applied six
review fixes (R13 positive-root-execution, R8/responder input delivery, ingress routing,
FUSE handle-lifecycle R20/T19, T17 definition) plus three corrections (R18 heading,
WebSocket generation label, T0 stale ref); rev 5 requires the clean cutover in the
pre-deployment build and defers all acceptance tests/verification until after deployment and
explicit owner request (R19.6). See `design.md §0` for the revision log; the
finding→requirement/design/task map is at the end of this file (§7).

---

## 0. Terminology (corrected)

This spec uses **threetched** for the external executable and the schema/execution
mechanism built from it. The word "native" is avoided as a synonym; where source uses
`native_*` identifiers (e.g. `NativeGenerationAdmission`, `current_generation_selection`,
the A2A client), they are named verbatim only in source citations and are scheduled for a
scoped rename/removal (R14/T18). "Threetched mechanism" means the established pipeline:

1. Refresh threetched's schema from the verified binary at seal time
   (`threetched.rs::prepare_schema_capture`/`capture`,
   `ThreetchedSurface::capture_with_config`, manifest+sha256 verified).
2. Seal the resulting typed plugin methods AND their executable command bindings into
   the same canonical `PluginSchema`/blob (`op-blob`; R18.9–R18.12).
3. Generate the D-Bus surface from those typed methods (schema-derived, `grpc-expert`).
4. Project the sealed methods through the identity-bound FUSE namespace
   (`tched_fs` protocol + `threetched-fs` mount; `plugins/<p>/operations/<m>/call`).
5. Execute the threetched binary explicitly when an admitted method runs
   (`threetched.rs:~322-345` builds the `Command`; runs via `chpst -u jeremy:jeremy` when
   the bridge is root, else directly as the configured service owner).

This mechanism EXISTS. This spec does NOT introduce a separate D-Bus service, a duplicate
schema, or a replacement binary-execution framework. The migration MOVES the remaining
HTTP/WebSocket application callers ONTO this mechanism.

Other glossary:
- **Control-plane chatbot** — the singleton application identity whose container *is* the
  chatbot; sole root-authority application identity; its container runs continuously.
- **Door** — TLS gRPC `:8090` in `op-grpc-bridge` (`operation.v1.PluginService/CallMethod`
  + interceptor); browsers reach it via op-web's same-origin gRPC-Web connector
  (`crates/op-web/src/door_proxy.rs`).
- **Mount / lungs** — an identity-bound 3tchedFS mount. A `TypedFuse` mount is configured
  as a daemon with a `Lungs` mode: `Socket { path, session }` (host-session) or
  `Credential { bus_address, credential: PathBuf }` (a **credential-file path baked into
  the mount daemon** — NOT a per-request identity argument). `MountedNamespace::open`/
  `open_for_mode` OPEN and VERIFY an existing mount; they do not create or rebind one.
- **Sealed blob / catalog** — `/dev/shm/opdbus/plugin-blobs/<plugin_id>.<schema_hash16>.blob`;
  `op-blob` is sole writer. A plugin exists ⟺ its blob is in the catalog. Catalog
  generation/hash live in the mount's `view.json`
  (`source_catalog_hash`, `source_catalog_generation`); the per-plugin object hash lives in
  `view["objects"][plugin]`. A plugin's full typed schema is readable at
  `plugins/<enc>/schema.json` via `MountedNamespace::schema(plugin)`.
- **PluginSchema** — single source of truth per plugin (`crates/op-plugins/src/state_plugins/
  <plugin>.rs`). `tched_fs` owns the filesystem protocol; `threetched` alone owns the
  verified threetched/zeroclaw binary application contract. `tched_router` is NOT a
  sibling authority for that binary. Unrelated plugins such as `json_render` retain
  their own contracts.
- **Canvas / UI spec** — a model-authored, validated declarative json-render spec (flat
  `{root, elements}`), data not code. Committed via `chat_response.respond_to_user`.
- **Designated responder** — the agent principal the bridge designates to answer a
  specific bridge-issued `turn_id` (`chat_response_turns.rs`: the bridge is the only
  `turn_id` issuer; a response is admitted only from the designated responder).

---

## 1. Current-state findings this spec targets (evidence; full cites in design.md §2)

- C1. The application reaches threetched over two direct transports, NOT the FUSE
  mechanism: (a) control-plane **chat** uses loopback HTTP + A2A `message/send`
  (`http://127.0.0.1:8084`, `tched_router_runtime.rs::chat_under_guard`,
  `control_plane_routing.rs`); (b) **UI generation** uses the stateless-generation
  **WebSocket** adapter — `json_render_generation_dispatch.rs:~157`
  `for_agent_alias("dashboard")` → `complete_under_guard`, which drives
  `tched_router_generation_native.rs` ("Bounded adapter for the pinned binary's stateless
  generation WebSocket", `tokio_tungstenite connect_async_with_config`). Both transports are
  the migration target — to be moved onto the established threetched FUSE mechanism, NOT onto
  a new mechanism.
- C2. `chat_response.respond_to_user` is a **commit of already-authored content**
  (`chat_response_dispatch.rs`), separate from generation. Owner expects it to be the
  response boundary associated with invoking threetched. The commit caller must be the
  **designated responder** (`chat_response_dispatch.rs:541` claims by `caller.principal_id`;
  `chat_response_turns.rs` requires the designated responder) — so the human who asks and
  the chatbot who answers are different principals (R10/R8).
- C3. The interceptor **mints genesis server-side** on first arrival
  (`interceptor.rs::anchor_verified_human_session` → `mint_and_store_genesis`), contrary to
  the settled "decoy mints OIA1/SID1; interceptor anchors" model (R12/R15).
- C4. There is **no browser→FUSE adapter**; browsers reach plugins only through the door
  (`door_proxy.rs`). A FUSE-only application path needs a net-new authenticated adapter and
  a resolved caller→mount lifecycle (R1/R11).
- C5. The backend canvas profiles **structurally reject the requested whole interactive
  UI**: `json_render_chat_canvas.rs:7-9` admits "no actions, bindings or state: a chat
  canvas is a static composition", and `:149-156` rejects host controls
  (`antigravityChatContainer`, `tchedRouterPicker`, `chatMessage`) and any component events;
  `json_render_canvas_profile.rs:166-184` validates an older React `setState`-only profile
  of exactly 8 components excluding the picker/chat host components;
  `chat_response_dispatch.rs:159-167` validates canvases with `PinnedCanvasValidator`. A new
  authoritative Svelte whole-application contract is required (R4/R16) — not a loosened
  validator.
- C6. The threetched executor (`threetched.rs:~322-345`) runs the binary as
  `jeremy:jeremy` (via `chpst` when root, else as the configured owner). A root bridge does
  NOT make that child execute as root in the chatbot container; the designated chatbot
  execution environment is not yet the execution target (R13).

### 1.1 Withdrawn finding (revision 1 error)
WITHDRAWN: revision 1 implied the threetched integration mechanism was missing and
proposed reaching threetched "only inside the sealed plugin backend" as if that were new.
The seal→type→D-Bus→FUSE→execute mechanism is ESTABLISHED (see §0). The real gap is that
the HTTP/A2A callers have not been moved onto it. All requirements below are rephrased
accordingly.

---

## 2. Scope

### 2.1 In scope
- Move the remaining HTTP/WebSocket application callers (UI generation, chat,
  model/provider/config operations, generation jobs) onto the established identity-bound
  threetched FUSE mechanism.
- Add an authenticated **browser→FUSE adapter** with a resolved caller→mount lifecycle.
- Define one authoritative **Svelte whole-application UI contract** (generated canvas +
  chatbot overlay + model-selection overlay + their actions), sealed as a profile, and
  carry it through generation-validator, response-commit-validator, and the Svelte
  registry.
- Establish the **chatbot execution boundary**: an admitted chatbot workload executes with
  effective UID 0 inside the designated chatbot container through its supported execution
  mechanism, distinct from the requesting principal and from the host identity of the bridge
  process.
- Define the **human→chatbot response handoff** (turn reservation, bound turn delivery,
  responder-side generation+commit, human retrieval).
- Route UI-generation model selection through the control-plane chatbot's authoritative
  selection.
- Make discovery + schema provenance concrete using existing mounted schema access.
- Perform the requested FUSE handle-lifecycle investigation and work (slot reuse,
  saturation/drain, mixed occupancy, failed opens, client exit, duplicate releases,
  teardown, in-flight charge ownership), preserving the 256-slot/byte bounds and RAII
  accounting.
- Remove obsolete HTTP/A2A/WS application paths and any fixed-dashboard fallback.

### 2.2 Out of scope (non-goals)
- NG1. Changing identities, grants, capabilities, keys, or WireGuard transport topology;
  no new WireGuard peer/service; no key rotation; no private-key handling.
- NG2. Introducing a second D-Bus service, a duplicate/parallel schema, a replacement
  binary-execution framework, or a second control plane. `op-blob` remains sole blob
  writer; D-Bus remains the control plane.
- NG3. Re-introducing React/JSX/Next.js, Antigravity rendering, the retired `op-gemma`
  crate, a second renderer, pairing flow, `op-web/ui/`, or `zeroclaw-gui`.
- NG4. Deployment/reseal. Build/preparation only; deployment is `./deploy.sh [--system]`
  (btrfs send/receive); reseal (`opblob seal-shm`+`persist`) is owner-run.
- NG5. Replaying the previous chat operation of unknown outcome.
- NG6. Plugin *visibility* implying permission to *invoke*.
- NG7. Globally elevating host services or other identities, deleting host accounts,
  removing required services, weakening isolation, or bypassing application admission.
- NG8. Adding `native = "threetched"` (or any alias) as a TOML/config alias — that would
  need a consumer and would preserve the ambiguous name instead of completing the rename.
- NG9. Shared credential-file swapping, arbitrary mount paths, operator-mount fallback,
  implicit chatbot impersonation, or new Btrfs storage membership on login.
- NG10. **Recovery is out of scope.** This migration SHALL NOT add requirements, tasks,
  acceptance criteria, or architectural promises for recovery, durable cross-stage workflow
  correlation, lost-result reconstruction, or "recoverable" outcomes; it SHALL NOT present
  recovery as an existing end-to-end capability, and building recovery SHALL NOT be a
  migration prerequisite. Retained safety properties (ordinary per-operation receipts,
  accurate failure/unknown-outcome reporting, resource cleanup, and no automatic replay of
  uncertain mutations) do NOT require implementing recovery and are kept (R9).

### 2.3 Privilege boundaries (constraints)
- PB1. The chatbot container is the chatbot identity and runs as root *inside its
  container*. Container UID 0 is **not** host UID 0 and is **not** granted to browser or
  container callers. The actual UID mapping MUST be MEASURED from the live container (R13),
  not asserted; revision 1's assumption that container-0 maps to a particular unprivileged
  host range is WITHDRAWN until measured.
- PB2. The chatbot identity is bound to the WireGuard **server** keypair (not a PSK, not a
  separate peer). Binding preserved; "not provisioned/admitted as a peer" must be enforced
  (R12) while not rejecting the legitimate chatbot identity (R12.4).
- PB3. Four distinct roles MUST be kept separate on every operation:
  (a) **execution owner/container/UID** (who runs the threetched child / FUSE daemon);
  (b) **requesting principal/session/group/capabilities** (who asked);
  (c) **designated responder** (who may answer a bridge-issued turn);
  (d) **owner of private conversation and UI state** (whose scope the result lands in).
  Running the bridge as root MUST NOT collapse these.

---

## 3. Functional requirements (numbered, testable)

### R1 — Authenticated browser-to-FUSE adapter
The browser SHALL perform plugin discovery, schema reads, UI generation, and plugin
actions through an authenticated adapter that executes against the caller's identity-bound
mount via the established mounted invocation convention, not through a direct application
HTTP/A2A/WebSocket path.

Acceptance:
- R1.1 Each request SHALL carry exactly one OIA1 or SID1 as binary metadata (unchanged
  door contract) and the caller identity SHALL be resolved server-side before any FUSE
  access.
- R1.2 The adapter SHALL execute against the caller's authorized mount (not an operator
  host-session mount, not an implicitly-selected chatbot identity).
- R1.3 Missing/invalid credential → UNAUTHENTICATED; no mount opened.
- R1.4 Model output or page input SHALL NOT address an arbitrary host path or mount; only
  the caller's authorized namespace is reachable (`MountedNamespace` rejects a path whose
  plugin/method is absent from the pinned catalog).
- R1.5 The same-fd write→fsync→seek→read receipt convention SHALL be preserved; no
  automatic retry after an uncertain outcome.

### R2 — Move HTTP/WS callers onto the established threetched FUSE mechanism
The application paths that generate UI, run chat, and perform model/provider/config/
generation-job operations SHALL use the established identity-bound threetched FUSE
mechanism (§0), not direct HTTP/A2A/WebSocket.

Acceptance:
- R2.1 No application dispatch path SHALL call `http://127.0.0.1:8084` A2A `message/send`
  or a WebSocket to threetched for generation/chat/model/config/generation-jobs.
- R2.2 The established mechanism SHALL be used unchanged in shape: admitted method → FUSE
  call → D-Bus dispatch → sealed `threetched` binary application backend → explicit
  binary execution. A sibling `tched_router` application catalog is not this target.
- R2.3 A FUSE facade that internally re-opens HTTP/WS to threetched SHALL be
  non-conformant.
- R2.4 Direct threetched HTTP/binary access MAY remain ONLY as an explicitly
  infrastructure/maintenance surface (e.g. binary health), never reachable from the browser
  and never the application generation/chat path; each such retained caller MUST be
  justified as non-application maintenance (R17).
- R2.5 Canonical admission (interceptor identity checks, sealed capabilities,
  `PluginService/CallMethod`) remains the admission path.

### R3 — Plugin detection through the authorized FUSE namespace
The UI SHALL detect plugins by enumerating the caller's authorized mounted namespace and
load each plugin's sealed-blob schema to generate interfaces and bind actions.

Acceptance:
- R3.1 Discovery SHALL use the pinned mounted catalog (`view.json` objects + per-operation
  `metadata.json`), verified by the `MountedNamespace` pin.
- R3.2 Render schemas SHALL come from the sealed blob via `MountedNamespace::schema(plugin)`
  (reads `plugins/<enc>/schema.json`); no parallel `schemas/` dir, client-invented
  definition, handwritten duplicate, or registry fallback.
- R3.3 Visibility ≠ permission (NG6): invocation still requires method declaration,
  matching plugin version, argument validation, verified identity, and the exact grant.
- R3.4 On catalog generation/hash change (reseal), discovery SHALL reflect the new pinned
  catalog and SHALL invalidate UI specs pinned to a superseded catalog/plugin contract
  (R7).
- R3.5 A failed live-value read (EIO on `value.json`) SHALL be surfaced as "state
  unavailable" while schema/metadata discovery still succeeds; the plugin SHALL NOT be
  treated as absent (missing state ≠ missing schema).
- R3.6 threetched's binary-derived surface SHALL be discovered via its sealed schema; a
  coverage inventory SHALL distinguish semantic contracts from generic CLI envelopes (R11
  below is renumbered R18 for threetched coverage — see §3 continuation).

### R4 — Whole-UI model generation as a validated Svelte declarative spec
The model SHALL generate the whole application UI as a validated declarative json-render
spec rendered only by the trusted Svelte registry.

Acceptance:
- R4.1 Generated output SHALL validate against the manifest-pinned renderer profile before
  use, and the validating catalog/renderer pin SHALL be recorded with the committed spec.
- R4.2 Every visible component and action SHALL be declared in the active UI catalog and
  implemented in the trusted Svelte registry; an undeclared component/action SHALL fail
  validation.
- R4.3 Renderer is Svelte/SvelteKit + Vite + `@json-render/svelte` only.
- R4.4 A generated component/mode/action SHALL NOT grant permission; server-side
  session+group+capability ACLs are enforced on every action incl. streamed/reconnect.
- R4.5 Connection/loading/error bootstrap SHALL be distinct from the generated application
  and SHALL NOT be a fixed fallback dashboard substituting for generation.

### R5 — Layout: canvas workspace + collapsible overlay panels
The generated canvas is the main workspace; chatbot and model-selection panels are
collapsible overlays ABOVE the canvas.

Acceptance:
- R5.1 Panels overlay the canvas (stacking above it), not a split dashboard.
- R5.2 Panel components/actions belong to the active catalog/registry (R4.2).
- R5.3 The contract SHALL define stacking order, keyboard access (open/close/esc/focus-
  trap), visible focus, `aria-live` status, and responsive behavior.
- R5.4 Conversation and model-selection state are preserved while collapsed and across
  canvas re-render.

### R6 — One authoritative model selection (control-plane)
UI generation SHALL use the control-plane chatbot's authoritative selected provider/model;
no independent UI-generation model, hidden fallback, stale dashboard selection, or second
routing authority.

Acceptance:
- R6.1 The generation path SHALL read the control-plane selection authoritatively and PIN
  it for the duration of each generation. The current
  `json_render_generation_dispatch.rs` use of `for_agent_alias("dashboard")` +
  `current_generation_selection` SHALL be replaced by the control-plane selection read
  (same enforcement `control_plane_routing::complete` applies: output provider/model must
  equal the observed selection).
- R6.2 No Gemma assumption, hidden fallback, or second provider state.
- R6.3 A control-plane selection change SHALL affect subsequent generations; an in-flight
  generation keeps its pinned selection; a selection change or revocation concurrent with
  a generation SHALL be resolved deterministically (fail the in-flight admission re-check,
  per the existing `check_current` pattern, rather than silently mixing selections).
- R6.4 Who MAY change the selection is governed by existing identity/group permissions
  (OD-100), unchanged.

### R7 — Catalog / schema-pin lifecycle and stale specs
Generated UI specs SHALL stay consistent with the sealed catalog AND the target plugin
contracts they bound to.

Acceptance:
- R7.1 A committed canvas SHALL carry the catalog/renderer pin it validated against
  (R4.1).
- R7.2 On reseal (catalog generation/hash change), a stored spec SHALL be revalidated
  against BOTH the current renderer profile AND the current target-plugin method contracts
  before re-render, and regenerated if revalidation fails. Renderer-only revalidation is
  insufficient: a changed/removed target plugin method SHALL invalidate the bindings that
  reference it (R16.5).
- R7.3 Bindings to a removed plugin/method SHALL fail validation rather than render dead
  controls.

### R8 — Response lifecycle (generate → commit → render → fail/cancel)
Generation, response commit, rendering, failure, and cancellation SHALL be one complete
workflow, with `respond_to_user` as the response boundary associated with invoking
threetched, committed by the designated responder.

Acceptance:
- R8.1 Workflow: (a) the bridge reserves a turn for the verified human and records the
  human request; (b) the chatbot (designated responder) receives the bound `turn_id`;
  (c) generation runs in the chatbot execution environment via the threetched FUSE
  mechanism using the pinned control-plane selection; (d) the chatbot commits via
  `respond_to_user`, which validates the canvas against the pinned Svelte profile, claims
  the open turn as the designated responder, commits into the asking human's scope,
  publishes SHM, extends the turn-hash chain, and emits `response_committed`; (e) the human
  reads the committed result (`get_conversation`/`get_turn_response`) from authoritative
  state in the ordinary course; (f) on failure, the operation reports failure or an unknown
  outcome accurately and cleans up its resources (R9). Reconstructing a lost result is out
  of scope (NG10).
- R8.2 Generation SHALL NOT remain on the HTTP bypass with a commit merely appended (C1/C2).
- R8.3 Turn ownership/conversation isolation preserved: a response is admitted only for an
  open, unexpired turn whose designated responder is the verified caller, committing into
  the asking human's scope. Turn-ownership checks SHALL NOT be weakened and the human SHALL
  NOT impersonate the chatbot (R10).
- R8.4 Authoritative state published to canonical SHM before notifications; subscriptions
  derive from committed SHM, not an in-memory payload or generator buffer.
- R8.5 Each committed response produces a receipt (response_id, conversation_id,
  turn_index, turn_hash, revision, canvas pin) and a content-free `response_committed`
  signal.
- R8.6 Uncertain generation/commit outcome → no automatic retry; the operation reports the
  outcome as unknown with its terminal diagnostic. A durable committed turn is already
  answered (the existing `chat_response` claim/commit semantics); the spec does not add any
  cross-stage recovery beyond that ordinary commit result (NG10).
- R8.7 Responder input delivery: the spec SHALL name the authorized contract that delivers
  the human's prompt content and the necessary context to the designated-responder chatbot
  for the bound `turn_id`, and the initial-page generation trigger. `list_pending_turns`
  returns only metadata (`PendingTurnInfo`: turn_id/conversation_id/turn_index/owner_key/
  owner_principal_id/opened_at — no prompt), and `get_conversation`/`get_turn_response` read
  the CALLER's own principal scope, so neither currently delivers the asked prompt to the
  responder. The contract SHALL deliver only the specific pending turn's prompt + minimal
  context under the responder's `chat_response.respond` grant for that designated turn; it
  SHALL NOT grant the responder broad read access to humans' conversations and SHALL NOT add
  recovery (NG10). The method/capability/payload are specified in design §3.7 and T11b.

### R9 — Receipts, resource cleanup, honest failure/unknown-outcome, no auto-replay
All FUSE-mediated operations SHALL preserve ordinary per-operation receipts, clean up their
resources, report failure and unknown outcomes accurately, and never automatically replay an
uncertain mutation. These are safety properties and do NOT require implementing recovery
(NG10).

Acceptance:
- R9.1 Each operation returns its terminal receipt/diagnostic on the same descriptor,
  including on denial.
- R9.2 Honor FUSE engine limits: `HANDLE_LIMIT`=256, byte budgets (`INPUT_LIMIT`=1 MiB,
  `DOCUMENT_LIMIT`=16 MiB, `TOTAL_LIMIT`=64 MiB); exceed → `ENOMEM`/`EFBIG`; never raise
  limits or abort the connection to relieve pressure.
- R9.3 An uncertain-outcome mutation SHALL NOT be automatically replayed; the operation
  reports the outcome as unknown with its terminal diagnostic. The spec adds no
  recovery/re-drive mechanism (NG10).
- R9.4 Resources SHALL be cleaned up on every path: handle slots and byte-budget charges are
  released (the existing RAII `Charge::Drop` + slot drop), and a spawned operation thread
  does not leak on failure/panic (existing `catch_unwind`). A requesting RPC's cancellation
  or a disconnected browser SHALL NOT be reported as a successful cancellation of the
  underlying mutation and SHALL NOT trigger a replay; the outcome is whatever the operation
  actually reported (success/failure/unknown), read from the ordinary committed state or the
  returned receipt — the spec does NOT promise to reconstruct a result that was lost
  (NG10).

### R10 — No FUSE/bridge recursion, lock inversion, deadlock; responder integrity
The browser→FUSE→bridge path SHALL NOT create FUSE→bridge→same-FUSE re-entry, lock
inversion, or deadlock, and SHALL NOT weaken responder/turn ownership to make the workflow
pass.

Acceptance:
- R10.1 A bridge handler servicing a FUSE-originated operation SHALL NOT re-enter the same
  mount while holding the engine/state-publication lock.
- R10.2 The design SHALL document an acyclic lock order (handle slot, per-object write CAS,
  `state_publication`, D-Bus call).
- R10.3 Concurrent writers to the same workspace object fail fast (`EBUSY`), not block.
- R10.4 The commit caller SHALL be the designated responder; the human SHALL NOT commit the
  chatbot's answer and the chatbot SHALL NOT be impersonated.
- R10.5 Ingress routing SHALL define an explicit boundary: the outer browser→FUSE adapter is
  an INGRESS-ONLY entry point reached from the gRPC door; the inner backend path is the
  D-Bus-backed sealed-plugin dispatch. Inner FUSE-originated D-Bus calls SHALL NOT re-enter
  the outer ingress adapter. The routing SHALL NOT use a client-controlled bypass flag to
  distinguish ingress from inner calls; the entry point and return path SHALL be named and
  structurally separated (design §3.3/§3.10). "No recursion" alone is not the routing design.

### R11 — Caller-to-mount lifecycle (resolve before executing)
The caller→mount lifecycle SHALL be fully specified before the executor is implemented,
because `Lungs::Credential` is mount-daemon configuration (a credential-file path), not a
per-request identity, and `MountedNamespace::open_for_mode` opens/verifies an existing
mount rather than creating or rebinding one.

Acceptance:
- R11.1 Authoritative lookup: given a verified caller (OIA1/SID1 → principal/session), the
  design SHALL specify how the caller's identity→container→mount is resolved
  authoritatively (not from the request body).
- R11.2 Mount ownership + provisioning lifecycle: the design SHALL name who owns/creates
  the caller's mount and the supported provisioning path; login/start/stop SHALL NOT add or
  remove Btrfs members (identity-storage invariant) and the executor SHALL NOT create or
  rebind mounts itself.
- R11.3 Exact caller binding verification SHALL go beyond checking `MountIdentity::Credential`
  mode: the mount's declared identity (instance UUID, session/mode, and the verified
  caller) SHALL be cross-checked so a mount cannot be bound to the wrong caller.
- R11.4 Credential refresh, revocation, concurrent requests, and mount replacement SHALL be
  defined (no shared credential-file swapping; a revoked/replaced mount fails closed).
- R11.5 Absent-mount behavior SHALL be defined (fail closed with a diagnostic; no operator-
  mount fallback, no implicit chatbot impersonation, no on-demand membership creation).
- R11.6 The original OIA1/SID1 admission constraints SHALL be preserved end to end.

### R12 — WireGuard peer-exclusion enforcement (without rejecting the chatbot)
The "chatbot WG-server-key identity is not provisioned/admitted as a peer" policy SHALL
have a cited enforcement point or an explicit enforcement task.

Acceptance:
- R12.1 The design SHALL either cite the existing enforcement (function + acceptance
  evidence) or specify a concrete enforcement task if it is missing (R15/T17); it SHALL NOT
  claim key choice alone enforces it.
- R12.2 No keys changed; no peer/service created (NG1).
- R12.3 Network peer provisioning SHALL be separated from application authentication, so
  "not a peer" does not reject the legitimate chatbot application identity.
- R12.4 Acceptance evidence SHALL show the legitimate chatbot identity is still admitted for
  application operations while being excluded from peer provisioning.

### R13 — Chatbot execution boundary (implement, not just document)
The admitted chatbot workload executes with effective UID 0 inside the designated chatbot
container through its supported execution mechanism. The bridge's host UID does not
establish that container identity or grant additional authority to the requesting caller.
The four roles of PB3 remain distinct.

Acceptance:
- R13.1 The design SHALL specify the supported mechanism by which the admitted threetched/UI
  workload is dispatched into the designated chatbot container and runs there with effective
  UID 0 (its in-container root identity), replacing the current `threetched.rs` executor
  which runs the child as host `jeremy` via `chpst`/configured owner.
- R13.2 The bridge's host UID SHALL NOT establish the chatbot container identity and SHALL
  NOT grant additional authority to the requesting caller; execution owner/container/UID
  (PB3a) stays distinct from the requesting principal (PB3b).
- R13.3 The requesting principal/session/group/capabilities, the designated responder, and
  the private-state owner SHALL remain distinct and preserved through the operation.
- R13.4 Chatbot-unavailability behavior SHALL be defined (an operation requiring the chatbot
  execution environment fails closed with a diagnostic; it is not silently run under another
  identity). It reports failure/unknown-outcome accurately and cleans up (R9); the spec adds
  no recovery mechanism (NG10).
- R13.5 The actual container UID mapping SHALL be MEASURED and recorded (the host subuid
  range container-UID-0 maps to); no assertion of a particular range without measurement
  (PB1). Acceptance SHALL VERIFY BOTH the child's container identity (it runs inside the
  designated chatbot container) AND its effective UID (0 in-container), e.g. the child's
  `/proc/<pid>/status` effective UID and its container/namespace membership.
- R13.6 No global elevation of host services or other identities (NG7).

### R14 — Terminology rename/removal (scoped)
Ambiguous "native" routing/execution naming SHALL be renamed to "threetched" or the precise
component name.

Acceptance:
- R14.1 A scoped task SHALL enumerate the `native_*` identifiers and rename/remove them,
  preserving exact source identifiers only in citations until renamed.
- R14.2 No `native = "threetched"` config alias is introduced (NG8).

### R15 — Interceptor minting reconciliation (tracked consistently)
The interceptor's server-side genesis minting (C3) reconciliation SHALL be tracked
consistently: it is NOT simultaneously mandatory-completion and optional.

Acceptance:
- R15.1 The design SHALL state whether anchor-only reconciliation is a demonstrated
  PREREQUISITE of this migration or a SEPARATE identity task. Default: SEPARATE, because no
  requirement here depends on removing server-side minting to function.
- R15.2 IF it is a prerequisite, the design SHALL identify the precise dependency and the
  missing authorization; otherwise it SHALL be tracked as a separate identity task not
  gating this migration.
- R15.3 No key/identity rotation in this spec (NG1).

### R16 — Authoritative Svelte whole-application UI contract (sealed profile)
There SHALL be one authoritative Svelte whole-application UI contract covering the
generated canvas, chatbot overlay, model-selection overlay, and their actions, replacing
the static/React chat-canvas profiles that reject interactive UI (C5).

Acceptance:
- R16.1 The contract SHALL admit the interactive application: host/overlay components (a
  chatbot panel, a model-selection panel), component actions/events, bindings, and state —
  the exact things `json_render_chat_canvas.rs`/`json_render_canvas_profile.rs` currently
  reject — declared as first-class, trusted, catalog-declared components, not smuggled past
  a loosened validator.
- R16.2 The contract SHALL be authored once in the UI repo, exported, imported into the
  backend, and sealed as a profile (mirroring the existing chat-canvas export/import/seal
  path, `json_render_chat_canvas_contract.rs`), so there is one source of truth.
- R16.3 The generation validator and the response-commit validator
  (`PinnedCanvasValidator` call sites in `json_render_generation_dispatch.rs` and
  `chat_response_dispatch.rs:159-167`) SHALL both validate against this one sealed Svelte
  profile.
- R16.4 The Svelte registry (`chatRegistry`) SHALL implement exactly the contract's
  components/actions; an action absent from the registry SHALL fail (R4.2). Action
  admission SHALL enforce server-side ACLs (R4.4).
- R16.5 Generated action bindings SHALL be pinned to the TARGET PLUGIN CONTRACTS as well
  as the renderer profile; a changed/removed target method invalidates its bindings (R7.2).
- R16.6 Fixed host controls SHALL NOT remain outside the model-generated application
  (overlays are inside the generated application's catalog/registry, R5.2); bootstrap
  chrome (R4.5) is the only fixed, non-generated surface.

### R17 — Complete migration inventory and dependencies
The migration SHALL cover all affected application callers with a source-backed inventory
and an explicit dependency graph.

Acceptance:
- R17.1 A source-backed caller inventory SHALL list every application caller of the HTTP/WS
  threetched path and the model/provider/config/generation-job operations — not merely
  `tched_router.Chat`. Each caller is assigned to a migration task or a justified
  non-application maintenance task (R2.4).
- R17.2 The dependency graph SHALL enforce: whole-UI generation cannot precede (a) the
  sealed interactive Svelte UI contract (R16), (b) correct responder handoff (R8/R10), (c)
  the execution boundary (R13), and (d) the caller→mount lifecycle (R11).
- R17.3 Cross-stage workflow correlation and recovery are OUT OF SCOPE (NG10). The spec
  SHALL NOT define durable correlation ids for recovery, a per-stage recovery authority, or
  lost-result reconstruction. Each operation independently returns its ordinary receipt and
  reports success/failure/unknown accurately (R9); a disconnected browser is not reported as
  a successful cancellation and does not trigger a replay.

### R18 — threetched contract coverage (no invented semantics)
The spec/implementation SHALL inventory threetched contract coverage and SHALL NOT invent
semantics or hide gaps with mocks.

Acceptance:
- R18.1 ONE sealed `threetched` PluginSchema/blob is the authority for the verified
  threetched/zeroclaw binary's full config schema and captured callable CLI surface.
  `tched_router` SHALL NOT be a separate application method/config schema authority.
  The inventory distinguishes binary semantic contracts from CLI-token/process
  receipts without treating a second handwritten catalog as semantic coverage.
- R18.2 Where a semantic contract is missing, the gap is stated; no mock/fabricated success
  is substituted.
- R18.3 The previously observed ~147 mounted methods are inventory, not proof of complete
  semantic contracts.
- R18.4 Owner U3 clarification: `threetched-surface` SHALL remain a capture/passthrough
  surface. Every real plugin seal SHALL refresh the full `config schema` bytes and
  recursively discovered CLI contracts from the verified threetched binary, bind their
  digests into the complete canonical plugin blob, and fail on capture errors. Runtime
  SHALL consume that sealed catalog only, without ambient rediscovery or another schema
  tree. The existing capture authority is not a missing migration implementation.
- R18.5 Provider-only / `StatelessGeneration` support SHALL become callable through that
  same binary-capture → seal → catalog path. A genuine producer-side CLI implementation
  may supply it; no handwritten or schemars-derived competing threetched method/config
  catalog may supply an invented replacement contract. `agent --message` is not the
  provider-only backend.
- R18.6 A vendor source candidate is not evidence of current executable or sealed-method
  availability. Report source, verified binary, seal and runtime availability separately.
  Invocation envelopes, call-file formats, guest execution and admission callbacks are
  separate U1/T6/T13 work, not U3 capture corrections or U3 acceptance.
- R18.7 Producer provenance SHALL bind every applied vendor patch, including patch0004,
  before build/application and in the emitted artifact manifest. The mandatory
  `PATCH_APPLICATION_GENERATION_CLI_SHA256` source pin SHALL match
  `patches.application_generation_cli_sha256`; missing provenance is not a compatibility
  path. Package preparation validates the full patch/inventory/base set against pins.
  Each source.pin consumer SHALL clear every required input it actually consumes before
  sourcing, then reject missing/empty file values; inherited environment values SHALL NOT
  supply omitted upstream, archive, patch, inventory or base provenance. Unused metadata
  is not made mandatory merely to satisfy a fixture.
  Seal/runtime manifest readers require the complete digest set while retaining their
  manifest/binary bindings. A source-only provenance repair is not a new artifact or seal.
- R18.8 The verified binary is the sole threetched schema producer. Capture SHALL preserve
  its full config-schema bytes and recursively discover its CLI feature commands; every
  discovered callable leaf and runnable optional-command parent SHALL become an actual
  `PluginSchema.methods`/`MethodDecl` using the captured CLI-token argument constraints
  and existing generated projection. The
  `{stdout,stderr,exit}` return is the process receipt, distinct from feature-domain
  result semantics. Do not infer methods from config properties or claim help tokens/
  receipts are domain semantic types. Static source establishes the mapping path; actual
  verified-binary feature availability and any additional producer-declared result
  semantics remain unestablished. No new emitter or invented contract is required.
- R18.9 Commands SHALL be sealed WITH the schema. At each seal, the verified binary's
  full config schema and recursive CLI contracts (R18.4) SHALL yield exactly one captured
  callable binding for every exposed binary-backed threetched `MethodDecl`: matching
  method ID, exact command/subcommand argv prefix, captured argument encoding/constraints,
  and executable identity/provenance in the SAME canonical sealed `PluginSchema`/blob.
  Command data is an executable binding, not merely descriptive help or configuration.
  A shared sealed snapshot keyed by method ID is permitted; a dedicated `MethodDecl`
  field is not mandated. No separately mutable command authority or second catalog is
  permitted. This requirement does not make unrelated plugins execute shell commands.
- R18.10 Runtime SHALL select the admitted method's bound sealed mapping and args
  contract, check their agreement, validate caller arguments, verify the executable
  against sealed identity/provenance, and invoke it directly. The argv prefix comes
  from that binding; caller values are encoded only by the captured sealed contract.
  No command text/shell interpolation, command inference from the method name,
  handwritten command table, ambient config/alias lookup, or runtime help/schema
  rediscovery may supply or override the binding. Parsing retained sealed evidence is
  not rediscovery. Preserve existing identity, exact-grant and guest-root boundaries.
- R18.11 Missing, unknown, ambiguous or mismatched method-command bindings SHALL fail
  closed before invocation. Consumers SHALL require agreement between the exposed
  method ID/args contract and its captured argv/contract/provenance in the same pinned
  document; presence of an `example` or descriptive help alone is not that check.
  A binary replacement or catalog change SHALL NOT silently retarget an admitted call
  or trigger recapture/retry against a new mapping.
- R18.12 Acceptance criteria (T3, design §3.1.1; not permission to run checks):
  (a) a nested feature method executes its captured multi-token command/subcommand
  prefix, not a method-name split; (b) captured option aliases, literal token values,
  positional order and encoding survive sealing and dispatch without renaming or shell
  expansion; (c) unknown methods, absent/duplicate bindings and mismatched method IDs,
  args contracts, argv or executable provenance are rejected before binary invocation;
  (d) binary/catalog changes preserve pinned authority or reject the call before
  invocation, never resolve an ambient replacement. `{stdout,stderr,exit}` remains
  runner transport, not an invented semantic emitter/result contract. Verification
  and tests remain deferred until after deployment AND explicit owner request (R19.6);
  recovery and broader migration are outside this U3 docs-only update.
- R18.13 Owner unified-authority correction (2026-10-24): application router/chat/
  model/config/generation operations belonging to the binary SHALL originate in its
  verified CLI capture and be sealed in that ONE threetched schema/blob. A separately
  registered `tched_router` catalog (including its former 38 authored methods) is
  nonconformant even if both blobs are sealed and some config fields import the same
  binary schema. Do not alias/rename/copy that catalog into threetched or suppress
  methods solely to claim completion. Existing callers remain explicit migration
  gaps until they target actual captured binary operations; missing producer
  functionality is an explicit gap, not permission for invented semantics.
  Only genuinely non-application operational maintenance may remain separate,
  justified by real purpose and authority; application APIs cannot be relabeled
  maintenance. Any candidate must satisfy this single-authority invariant; a sibling
  application authority may not be accepted/published/staged as completed U3.
  No deployment is authorized.

### R19 — Clean cutover and obsolete-path removal
The migration SHALL be a clean cutover with obsolete code removed, not a shim.

Acceptance:
- R19.1 On completion, no live application path generates UI / runs chat / does
  model/config/generation-job ops through direct threetched HTTP/A2A/WS.
- R19.2 The fixed-dashboard fallback is removed (bootstrap chrome retained).
- R19.3 Deprecated UI artifacts are not reintroduced (NG3).
- R19.4 Status SHALL be reported as implemented/built/deployed/tested separately; no
  partial transport swap or scaffold presented as complete.
- R19.5 Full-spec completion SHALL include satisfaction of R12 and R20 with their required
  acceptance evidence. Independent sequencing of T17/T19 SHALL NOT waive either requirement.
  Unresolved required work, acceptance evidence, or authorization SHALL remain explicitly
  blocked, not reported as complete; no identity-change or test authorization is implied.
- R19.6 Cutover implementation (including obsolete-path/fallback removal) and the required
  build SHALL precede deployment. Acceptance tests, parity checks, smoke tests, formatting,
  Clippy, and other behavioral verification SHALL NOT run before deployment and SHALL run
  only after explicit owner request. Runtime/acceptance checks exercise the deployed target;
  source-based checks apply only to the source revision that produced that deployment. Source
  investigation/design and the required build/deployment preparation remain permitted; build
  success is not acceptance verification or deployment authorization. After a post-deployment
  check fails and code changes, rebuild and redeploy before rechecking.

### R20 — FUSE handle lifecycle (the requested handle work)
The migration SHALL include the requested FUSE handle-lifecycle investigation and work;
generic resource cleanup (R9) is NOT sufficient on its own. The 256-slot / byte bounds and
the existing RAII accounting SHALL be preserved.

Acceptance:
- R20.1 Provenance first: before diagnosing the reported symptom, the deployed binary's
  provenance SHALL be compared with source (the reported behavior may be a deployed/source
  drift, not a current-source bug). The illustrative "flush-before-removal" bug SHALL NOT be
  assumed to exist without evidence.
- R20.2 The lifecycle work SHALL cover, with acceptance criteria, each case:
  (a) sequential reuse beyond 256 directory opens (open/release repeatedly past the slot
  count — slots are reused, not exhausted); (b) controlled saturation to `HANDLE_LIMIT` and
  drain (257th → `ENOMEM`, then release drains and reuse resumes); (c) mixed file/directory
  occupancy of the slot table; (d) failed opens (no slot/charge leak on the error path);
  (e) client exit mid-operation (handles/charges released on teardown); (f) duplicate
  releases (idempotent, no double-free of a slot/charge); (g) daemon teardown with in-flight
  operations (charge ownership moves with the spawned job, released exactly once).
- R20.3 The 256-slot `HANDLE_LIMIT` and byte budgets (`INPUT_LIMIT`/`DOCUMENT_LIMIT`/
  `TOTAL_LIMIT`) and the RAII `Charge`/slot accounting SHALL be preserved; the table SHALL
  NOT be grown to conceal a leak, and active handles SHALL NOT be evicted.
- R20.4 In-flight charge ownership SHALL be explicit: a `Buffer`/charge moved into a spawned
  operation is released exactly once when that job completes/fails, even after RELEASE
  removes the handle-table entry (the existing design intent in `fuse_buffers.rs`).

---

## 4. Non-functional requirements
- NF1. Security: default-deny, server-side authorization on every operation incl.
  streamed/reconnect. No secrets (WG private key, credentials) in specs/logs/UI/payloads/
  fixtures.
- NF2. Accessibility: WCAG 2.2 AA for generated UI + overlays (keyboard, focus, aria-live,
  contrast).
- NF3. Memory/resource: honor the 256-slot table + byte budgets; measure before changing
  allocation behavior; no connection abort / `max_background` tuning to mask backpressure.
- NF4. Reliability: no auto-retry after uncertain FUSE outcomes; the operation reports the
  unknown outcome with its diagnostic and cleans up. (Recovery is out of scope, NG10.)
- NF5. One door, one credential on the UI side; no second transport, per-plugin stubs, or
  direct HTTP to op-web for chat.

---

## 5. Acceptance evidence matrix

| Req | Observable acceptance evidence (gathered after deployment and explicit owner request; runtime checks target the deployment) |
|-----|-------------------------------------------------------------------|
| R1  | FUSE op succeeds with SID1/OIA1; no credential → UNAUTHENTICATED, no mount opened. |
| R2  | grep: no application call to `:8084`/A2A/ws for gen/chat/model/config/jobs; flows go FUSE→D-Bus→sealed threetched→binary exec. |
| R3  | Discovery lists the caller's authorized plugins; render schema = `schema(plugin)` from the blob; visible-but-ungranted method denied on invoke. |
| R3.5| value.json EIO renders schema-derived UI with unavailable marker; plugin not dropped. |
| R4/R16 | Canvas with an interactive overlay/action is ACCEPTED; a component/action absent from the Svelte registry is REJECTED; both validators use one sealed Svelte profile. |
| R5  | Panels overlay canvas; keyboard open/close/esc/focus-trap; collapse preserves conversation + selection. |
| R6  | Generation uses control-plane selection (not "dashboard" alias); change → next gen changes; revocation fails the in-flight re-check. |
| R7  | Reseal/hash change → stored spec fails renderer+target revalidation and is regenerated; removed-method binding fails validation. |
| R8/R10| One generate→commit turn: human reserves/records request, chatbot (designated responder) commits into human scope; human reads the committed result in the ordinary course; human cannot commit the chatbot's answer. |
| R8.7 | A named input-delivery contract hands the designated responder ONLY the bound turn's prompt + minimal context under `chat_response.respond` for that turn; the responder gets no broad read of humans' conversations; initial-page generation trigger named. |
| R10.5 | Named ingress-only adapter entry point + backend return path; inner FUSE-originated D-Bus calls do not re-enter the ingress adapter; no client-controlled bypass flag. |
| R9  | 257th handle → ENOMEM; uncertain outcome → terminal diagnostic reported as unknown, no auto-retry; slots/budget charges released on every path; disconnect is not reported as a successful cancellation and triggers no replay. |
| R11 | Caller→mount resolved authoritatively; wrong-caller mount rejected; absent mount fails closed; revoked mount fails closed; no operator-mount fallback. |
| R12 | Cited/enforced peer-exclusion with evidence; chatbot still admitted for app ops; no key/peer change. |
| R13 | Admitted chatbot workload runs with effective UID 0 INSIDE the designated chatbot container (verify both container membership and in-container UID 0), not as host-jeremy-from-bridge; measured host subuid map recorded; chatbot-unavailable fails closed with a diagnostic (no recovery). |
| R14 | `native_*` identifiers renamed/removed; no `native="threetched"` alias. |
| R15 | Interceptor-minting reconciliation tracked as a single, consistently-classified task. |
| R17 | Source-backed caller inventory complete; dependency graph enforces R17.2; no cross-stage correlation/recovery promised (NG10). |
| R18 | Coverage inventory plus R18.9–R18.12 / design §3.1.1 / T3.a–d: each exposed method has one matching argv/encoding/args/provenance binding in the SAME canonical sealed schema; nested feature and alias/token preservation, unknown/missing/mismatch rejection and binary/catalog-change pinning require acceptance evidence. Current co-sealing source trace is not a pass for all consumer checks; no mock or semantic emitter. Execution remains deferred by R19.6. |
| R19 | No live direct-threetched app path; fallback removed; full completion includes R12/R20 acceptance evidence; unresolved work/authorization remains blocked; status reported by stage. |
| R20 | Provenance compared before diagnosis; each handle-lifecycle case (a)-(g) has passing acceptance; 256-slot/byte bounds and RAII accounting preserved; table not grown, active handles not evicted. |

---

## 6. Open questions for reviewer (only those not answerable from repo evidence)
- Q1. Browser→FUSE adapter placement: op-web same-origin forward to a bridge FUSE-executor
  service, vs a new bridge-side gRPC service. Design proposes the former (one door/one
  credential). Confirm.
- Q2 (reframed per finding 3). Per-caller mount lifecycle: who provisions and owns an
  ordinary human caller's identity-bound mount, and is there a supported path to resolve
  and open it under the caller without creating Btrfs membership on login? If no per-caller
  mount exists, the executor cannot run ordinary-caller operations and R11 blocks R1/R2 for
  non-chatbot callers — reviewer must confirm the provisioning owner's supported mechanism.
- Q3 (resolved per finding 8). Interceptor-minting reconciliation is tracked as a SEPARATE
  identity task (R15.1 default), NOT a prerequisite of this migration, because no
  requirement depends on removing server-side minting to function. If the reviewer deems it
  a prerequisite, they must state the precise dependency and the missing authorization.

---

## 7. Finding → requirement/design/task mapping

| Owner/reviewer item | Requirements | Design § | Tasks |
|---|---|---|---|
| Architecture correction (threetched mechanism established) | §0, R2, NG2, 1.1 withdrawn | §0, §2.0, §3.1 | T3, T5, T6, T11 |
| F1 Backend UI contract + Svelte registry together | R4, R16, R7.2 | §3.5, §3.11 | T9a–T9f |
| F2 Chatbot execution boundary (not just UID docs) | R13, PB1, PB3 | §3.9, §3.12 | T1, T1b, T13b |
| F3 Caller→mount lifecycle before executor | R11, NG9 | §3.3, §3.13 | T4a–T4e (replaces old T4) |
| F4 Human→chatbot response handoff + input delivery | R8, R8.7, R10, PB3 | §3.7 | T11, T11b |
| F5 UI-generation model-selection path | R6 | §3.6 | T11c |
| F6 Discovery + schema provenance concrete | R3.2, R7.2, R16.5 | §3.4 | T7, T7b |
| F7 Complete migration inventory + deps (correlation/recovery removed, NG10) | R17 | §2.0, §4 | T3b, T13, T17-graph |
| F8 Identity policies w/o scope expansion (WG + minting) | R12, R15 | §3.9, §3.15 | T2, T17, T15 |
| F9 Terminology + preload compliance | R14, (preload) | §0, §1 | T18, (done) |
| Recovery out of scope (owner, rev 3) | NG10, R8.6, R9, R13.4, R17.3 | §3.7, §3.8, §3.12 | T1b, T5, T11, T13b |
| rev4-1 Rename (Identity-bound FUSE Application Migration) | header, spec id | §0 | (dir+id updated) |
| rev4-2 Positive root execution (UID 0 in chatbot container) | R13, PB1 | §3.12 | T13b |
| rev4-3 Responder input delivery contract | R8.7 | §3.7 | T11b |
| rev4-4 Ingress-only adapter vs inner backend boundary | R10.5 | §3.3, §3.10 | T6 |
| rev4-5 FUSE handle-lifecycle work | R20, NF3 | §3.16 | T19 |
| rev4-6 Define T17 (WG peer-exclusion) | R12, R15 | §3.9, §3.15 | T17 |
| rev4-fix R18 heading / WebSocket gen label / T0 stale ref | R18, C1 | §2.0, §2.1 | T0 |
| Completion-rule clarification (owner-authorized) | R19.5, R12, R20 | §3.15b, §4 | T0, T17, T19, hard ordering and non-completion criteria |
| rev5 deployment-first verification order | R19.6 | §4 | T20, T21, hard ordering and non-completion criteria |
| Commands sealed WITH schema (owner, U3) | R18.9–R18.12, U3, §5 R18 | §3.1.1, G3 | T3.a–d; criteria only, deferred under R19.6 |

---

## 8. Genuinely unresolved prerequisites (after exhausting repo evidence)
- U1 (blocks R1/R2 for non-chatbot callers): whether a supported per-caller identity-bound
  mount exists/can be opened without login-time Btrfs membership creation (Q2). The
  identity-storage invariant forbids add/remove on login; the provisioning owner must
  confirm the resolve-and-open path.
- U2 (R12): the exact existing peer-exclusion enforcement point is not yet located in
  source (T2 investigation); if absent, R15/T17 adds enforcement — but that is identity
  work requiring explicit authorization (NG1).
- U3 (R18): one verified binary → full config schema plus recursive callable CLI
  contracts → ONE canonical `threetched` PluginSchema/blob. Same-blob argv/args/
  provenance and runtime agreement remain required (R18.9–R18.12).
  The competing tched_router schema/registration and direct exports/clones were
  retired using the existing retired-ID convention. Corrected opblob built and
  isolated seal completed on 2026-10-24: 71 catalog members, no tched_router,
  threetched schema hash `cec4867a85ca66c33d1b1c37f1bc0bc11296584b755ce569aeb5f9c3d4db94b1`.
  Canonical blob extraction observes 148 matching MethodDecl/command bindings:
  143 callable leaves plus five runnable optional parents, including stateless-generate.
  Full config capture has 83 root properties and 346 definitions.
  Existing bridge/application callers and their legacy tests remain migration gaps;
  no broader HTTP/WS switch, deployment or provider invocation occurred. Artifact
  evidence is recorded in implementation-result.md; runtime execution and richer
  feature semantics are not established by seal evidence. STOP for owner review.

Revision 5 records the owner-directed execution order. Implementation belongs in OMP; this
document records scope and gates but does not authorize deployment or pre-deployment tests.
