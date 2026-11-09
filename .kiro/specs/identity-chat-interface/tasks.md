# Implementation Plan: identity-chat-interface

## Overview

This plan turns `design.md` into incremental TypeScript coding steps inside
`apps/identity-chat/` in the Hackathon_Repository. The design names the
language (TypeScript, SvelteKit, Svelte 5, json-render Svelte, AI SDK, Chat
SDK), so no language choice is open.

Execution order (owner confirmation, 2026-11-09; task numbers below are
stable references, not permission to implement the UI early):

1. Read-only discovery, Baseline_Record, Runtime_Record and
   Legacy_Chat_Inventory with Jev triage.
2. Replace backend and transport first: server-side core, direct FUSE
   Operation_File I/O, Chat SDK intake, AI SDK loop, protobuf Chat_Link and
   the existing-ingress host adapter once its prerequisites are resolved.
   Preserve Tonic's independent backend mux services. Use it on the browser
   path where the recorded existing deployment requires it.
3. Wire model selection into the provider request path. Follow the spec
   where defined; otherwise retain the existing selection mechanism under
   the owner's explicit exception to the clean-slate rule. Do not retain
   old chat transport or inference scaffolding merely to keep selection.
4. Complete the authorized legacy chat cutover, including the
   `ctl_plane_chatbot` coupling cut, under shared-repository handoff rules.
   Keep identity, grants and storage unchanged.
5. Implement the Chat_UI last, using `@ai-sdk/svelte` `Chat` with the custom
   gRPC-Web `ChatTransport`, once its backend contract is available.
6. Perform repository and end-to-end checks under the applicable verification
   and Live_Change gates. An in-process harness is not deployment evidence.

The confirmed browser transport is gRPC-Web, not native browser gRPC or a
default HTTP/SSE chat endpoint. Both Vercel SDKs stay. Internal Fetch objects
used by the Chat SDK do not change that wire contract. An unresolved host
adapter stays blocked; it does not authorize a new listener.

Except for the owner's explicit model-selection exception above, no existing
chat code is imported, copied, wrapped, called or adapted (steering
`chat-clean-slate.md`). `chat_router` and every other
threetched-backed service stay in place; they are recorded as naming
defects, never removed. Identities, grants and storage are never removed or
changed.

Blocked tasks are marked **BLOCKED** with the gating IDs. A blocked task stays
in proposed status and must not be started until every gating ID is
resolved in `design.md` or the Runtime_Record. Tasks marked **PARTIAL** can
implement the stated interim behaviour now; the remainder is gated.

## Tasks

- [ ] 1. Read-only discovery and records
  - All sub-tasks are read-only. No login, start, stop, restart, logout, mount,
    unmount, grant, storage, identity or deployment change (Req 1.6).
  - Every observation is redacted before it is written: Credential_Material is
    replaced by a marker naming only the field (Req 1.9).
  - [~] 1.1 Write the Baseline_Record entries for every pinned interface the Chat_Interface uses
    - Cover `identity_sled` methods, `op-execution-tracker` `ExecutionRecord`,
      `verify_integrity`, `hash_execution`, the Sealed_Method_Schema format
      (`MethodDecl`, `SideEffect`, `required_capability`), the Operation_File
      contract (`threetched-fs` `main.rs`, `mounted.rs`, README),
      `MethodExecutionReceipt`, Door_8090 and the Op_Web_Connector.
    - For each: path relative to `/srv/git/odbus-verified`, symbol, full
      40-character pinned commit, signature, sha256, committed-or-WT flag.
    - Create a Blocker_Record for each symbol absent at the pinned commit or
      with a differing signature, including B-09 (`--guest-kernel` WT only)
      and B-10 (`refresh_credential` output differs in WT).
    - Record B-01 (`mount-3tchedfs` catalog defect) with helper command, error
      text and timestamp, and that the op-dbus source fix exists and is not
      deployed. Do not mount through another path or edit the catalog.
    - Record the pinned `crates/threetched-fs` directory name as a pinned
      naming defect (D-35 naming note).
    - _Requirements: 1.1, 1.7, 1.9, 1.11_
  - [~] 1.2 Write the Runtime_Record entries for Branches and Deployment_Target surfaces
    - For each discovered Branch and each candidate Deployment_Target
      (its Identity_Container and the Host_Dashboard): present or absent for
      the `3tched` view (`/run/mounts/3tchedFS`), the `threetched` view
      (`/run/mounts/threetched`), the Lungs_Socket, Door_8090 and the
      Op_Web_Connector, with exact command, output or error text and UTC
      timestamp to the second. Unobserved items are recorded as absent.
    - Re-observe every owner-brief fact the design relies on (catalog
      generation 144 contents, op-web `:8080` forward, Lungs_Socket paths,
      no in-container egress) before any later task relies on it.
    - Record B-02 (no mount or Lungs_Socket for the current user) and B-03
      (no 3tchedW3_Identity) if they still hold.
    - _Requirements: 1.2, 1.3, 1.6, 1.9_
  - [~] 1.3 Record the Bound_Identity and Execution_Record operations per mounted Branch (BLOCKED: B-02, B-01)
    - **BLOCKED (proposed):** needs a mounted namespace for the Branch (B-02;
      host view also B-01). Until then, write the Blocker_Record of Req 1.8
      per Branch and record the Bound_Identity as unidentified.
    - When a mount exists: identify the Bound_Identity only from a
      `get_own_identity` Receipt through that mount's Operation_File; record
      Public_Identity_Fields and the Receipt.
    - Run the D-11, D-12, D-13 and D-14 discovery procedures by read-only
      local file I/O on the mounted catalog and method declarations (D-44),
      recording method names and Required_Capabilities or explicit "none".
      Record candidate fields for D-25 (principal id) and SID1/OIA1 grammar
      source for D-18. Read `chat_fs.get_protocol` for D-16 diagnostic kinds.
    - _Requirements: 1.4, 1.5, 1.8, 1.9, 6.6_
  - [~] 1.4 Build the Legacy_Chat_Inventory with Jev triage (read-only)
    - Start from the D-36 removal candidates and search `/srv/git/odbus-verified`
      and `/srv/git/3tched.com` read-only for further chat code.
    - Per item record: path, symbol, pinned commit, callers traced in
      source, classification (chat code to remove; identity, grant or
      storage binding to keep; threetched-backed service to keep; unclear).
    - Run the credential check on every source excerpt first; send only
      source code and non-secret text to Jev. Never send SID1, OIA1, keys,
      `.env` content, grant files or identity records.
    - Ask `jev-latest` through `POST /v1/systemone` one `choice` question
      (chat code / threetched-backed service / identity-grant-storage
      binding / unclear) and `noul` anomaly questions (naming-rule violation,
      3tched code depending on threetched internals, bypass of operation
      files or grants, dead or unreachable path, duplicate chat stack). Load
      `TYPESAFE_API_KEY` into the call environment only; never print, log or
      commit it.
    - Record question, model, answer, probabilities, confidence and judged
      paths. Confidence below 0.6 is inconclusive. Confirm every
      classification by caller trace before any later task acts on it; a Jev
      answer authorizes nothing.
    - Classify `ctl_plane_chatbot` as kept and integrated; `chat_router*` and
      other threetched-backed services as kept (naming defects);
      `chat_fs`, `chatbot-identity` and the control-plane identity bindings
      as kept and listed for the owner. List every item whose role is unclear
      for the owner and leave it untouched.
    - _Requirements: 21.2, 21.4, 21.8, 22.1, 22.6, 22.8_
  - [~] 1.5 Record the threetched naming defects
    - Record `chat_router` (renamed from `tched_router` in `71fa232`) and every
      other `chat_*` or `tched_router` name on a threetched-backed service as a
      naming defect in the Baseline_Record, with path and pinned commit.
    - Record the `ctl_plane_chatbot` coupling to `chat_router` types, limits and
      plugin id (`ctl_plane_chatbot.rs`, `ctl_plane_chatbot_methods.rs`) as a
      3tched-on-threetched dependency to cut (task 20).
    - Hackathon code and docs address the Threetched_Surface only by
      `threetched` names.
    - _Requirements: 18.6, 22.8_

- [ ] 2. Project scaffold and shared types
  - [~] 2.1 Create the `apps/identity-chat/` scaffold without installing anything (PARTIAL: D-01, D-34)
    - Create `package.json` with exact versions from the D-01 table for every
      Pinned_Package_Set entry except the Model_Provider package (D-04 OPEN;
      `@ai-sdk/anthropic` is added only by task 19.9). Add `svelte`,
      `@sveltejs/kit`, `vite` and `typescript` at the D-01 recorded versions,
      and `fast-check` 4.10.2 as a dev dependency. The test runner is a dev
      dependency chosen here and recorded in D-01 with an exact version.
    - Create `svelte.config.js`, `vite.config.ts`, `tsconfig.json` (strict),
      `.gitignore` covering `node_modules/` and `.runtime/`, and the empty
      directory layout of the design (`src/lib/server/...`, `src/lib/client`,
      `src/lib/catalog`, `src/routes`, `scripts`, `tests/property`,
      `tests/unit`, `tests/fixtures/synthetic`).
    - Do not run `npm install` and do not create `package-lock.json`; that is
      task 19.1 (owner request, D-01 confirmation, D-34).
    - No React, JSX, TSX or Next.js file or dependency; no `@ai-sdk/mcp` or
      `@modelcontextprotocol/*`.
    - _Requirements: 2.1, 2.2, 2.4, 3.3, 3.5, 3.6_
  - [~] 2.2 Define shared types, typed errors and configuration files
    - Create `src/lib/server/types.ts` with the Data Models of the design
      (branded `PrincipalId`, `ChatSessionId`, `ToolKey`, `ViewKind`,
      `CatalogPin`, `RegistryEntry`, `RegistrySnapshot`,
      `HeldCapabilityResult`, `PendingToolCall`, `ToolCallStatus`,
      `ToolCallStatusUpdate`, `ReceiptSummary`, `BoundIdentityRecord`,
      `PublicIdentityFields`, `ChatSession`, `IdentityProfile`,
      `BlockerRecord`, `SecurityEvent`, `CredentialClass`,
      `ExecutionRecordView`).
    - Create `src/lib/server/errors/typed-error.ts`: `TypedError` with exactly
      one of the eight kinds, `failingItem`, `nextStep` looked up from
      `config/error-next-steps.json` (D-19 table, PROPOSED-DEFAULT).
    - Create `config/read-back.json` as an empty map (D-14 OPEN) and
      `config/naming-defect-exclusions.json` as an empty list with a header
      member citing the Baseline_Record entry; the two excluded names are
      added only by task 19.6 (D-32, B-14).
    - _Requirements: 15.6, 8.12, 6.7_

- [ ] 3. Secret_Guard
  - [~] 3.1 Implement `guard/credential-registry.ts` and `guard/secret-guard.ts` `checkItem` (PARTIAL: D-18 envelope grammar OPEN)
    - Held-value rule: complete values and every contiguous 16-byte window,
      precomputed per D-18 encoding (raw UTF-8, base64 at all three byte
      alignments, base64url, hex both cases, percent-encoding, JSON string
      escaping) into a partition-local in-memory set. Nothing derived from a
      held value is logged, persisted or hashed.
    - Envelope rule: interim D-18 rule (`SID1` or `OIA1` followed by 16 or more
      characters of `[A-Za-z0-9+/=_.:-]`) in every listed encoding. The real
      grammar is task 19.7.
    - Declared-secret field rule: JSON members named `sealed_id` or carrying
      `x-oscal-subid` `src.service.identity-sled.sealed-id@v1`.
    - `registerHeld(cls, value)` per partition; verdict
      `{ clean } | { clean: false, cls } | { error }`.
    - _Requirements: 5.5, 5.1, 5.2, 1.9, 19.5_
  - [ ]* 3.2 Write property test for held-value and envelope detection
    - **Property 1: Held-value and envelope detection**
    - **Validates: Requirements 5.5, 1.9, 19.5**
  - [~] 3.3 Implement `guard/stream-guard.ts` with chunk hold-back
    - Hold back 15 bytes for raw windows and the encoded window length minus
      one for encoded windows; check `heldBack + chunk`; release only bytes
      that can no longer join a window; a match blocks the chunk and the held
      bytes; `end()` checks and flushes the tail.
    - _Requirements: 5.5, 5.6_
  - [ ]* 3.4 Write property test for chunk-split detection
    - **Property 2: Chunk-split detection equals whole-item detection**
    - **Validates: Requirements 5.5, 5.6**
  - [~] 3.5 Implement fail-closed output gating and the record writers
    - Wrap every outbound channel (`model-context`, `ui-stream`, `log`,
      `telemetry`, `persisted-server`, `persisted-client`) with a 2 000 ms
      check limit; detection, error or timeout withholds the whole item and
      emits a `blocked-output` or credential-check typed error.
    - Create `records/security-events.ts` and `records/blocker-records.ts`:
      JSON lines under `apps/identity-chat/.runtime/` (git-ignored), every
      field passed through the guard, no part, window or digest of a detected
      value. Provide the redacting logger used by the Chat SDK intake.
    - Typed-error fields are redacted with a class-only marker before return.
    - _Requirements: 5.2, 5.6, 5.7, 12.8, 15.9, 7.2_
  - [ ]* 3.6 Write property test for fail-closed detection
    - **Property 3: Detection and guard failure fail closed**
    - **Validates: Requirements 5.1, 5.2, 5.6, 5.7, 12.8, 15.9**
  - [~] 3.7 Implement request-intake checking
    - Check every field of turn, decision and re-verify requests before they
      reach the turn loop, the context builder, the executor or the logger; a
      match rejects the request with a credential-refused error, no state
      change and one security event.
    - _Requirements: 5.8_
  - [ ]* 3.8 Write property test for intake refusal
    - **Property 4: Credential-bearing requests are refused at intake**
    - **Validates: Requirements 5.8**

- [ ] 4. Profile_Loader and Session_Mode resolution
  - [~] 4.1 Write `config/identity-profile.schema.json` and `profile/profile-loader.ts`
    - Schema per D-22 (PROPOSED-DEFAULT): JSON Schema 2020-12,
      `additionalProperties: false` everywhere, `format`, `principal_id`,
      `display_name`, optional `expected_session_id` and `session_mode`,
      `deployment_targets[]` with `kind` and `mount_paths` holding exactly the
      `3tched` and `threetched` keys (absolute paths, no `..`).
    - `parse`: size check (<= 65 536 bytes) before decoding, strict UTF-8,
      duplicate-key-rejecting `JSON.parse`, Ajv validation, Secret_Guard pass
      on every string, immutable result; errors name field path and rule,
      never the value.
    - `format`: canonical JSON in schema key order, list order preserved.
    - Session_Mode resolution: exactly `standard`, `control_plane` or
      `ui_build`, otherwise `standard`; limit 5000 for `control_plane` and
      `ui_build`, 5 for `standard` (D-20; the standard value must stay in
      1..10). A non-string `session_mode` is a type error.
    - _Requirements: 19.3, 19.4, 19.5, 19.6, 19.7, 19.11, 14.7, 14.8_
  - [ ]* 4.2 Write property test for profile round trip
    - **Property 36: Profile round trip**
    - **Validates: Requirements 19.6, 19.7**
  - [ ]* 4.3 Write property test for profile rejection
    - **Property 37: Profile rejection**
    - **Validates: Requirements 19.5, 19.11**
  - [ ]* 4.4 Write property test for Session_Mode resolution
    - **Property 33: Session_Mode resolution**
    - **Validates: Requirements 14.7, 14.8**

- [ ] 5. Schema_Validator
  - [~] 5.1 Implement schema validation in the executor and mounted-file adapter
    - `Ajv2020` with `strict: false`, `allErrors: true`, `coerceTypes: false`,
      `useDefaults: false`, `removeAdditional: false`, `ajv-formats`.
    - Non-JSON inputs fail with path `""` and constraint `type`; failures list
      path and keyword, never the value.
    - Render the accepted value once with `JSON.stringify` into `inputBytes`;
      reject over the 1 MiB argument bound.
    - _Requirements: 8.3, 8.8, 8.9_
    - Replacement note (2026-11-09): removed the unused standalone wrapper;
      validation now runs in `exec/tool-executor.ts` before holding input and
      `exec/operation-file-runner.ts` before submission. Do not recreate a second
      validator path. Remaining detailed error/reporting criteria stay open.
  - [ ]* 5.2 Write property test for out-of-schema rejection
    - **Property 18: Out-of-schema arguments are rejected**
    - **Validates: Requirements 8.3, 8.8**
    - Property 19 (round trip to the call input) needs the fake adapter's
      input-byte log and is placed with the Tool_Executor (task 10.10).

- [ ] 6. Tool_Registry with dual-view derivation — **RETIRED (owner, 2026-11-09). DO NOT IMPLEMENT.**
  - **Retirement (owner, 2026-11-09):** The Tool Registry is retired. A tool IS a plug-in; there is no tool registry and no registry pattern. When the identity-chat needs tools, each is implemented as a one-file plugin (one .rs file plus one schema JSON per plugin, a `*State` struct as the single typed present-state contract, per-method `*Input`/`*Output` pairs with `#[serde(deny_unknown_fields)]` on inputs, a `dispatch_*_method` dispatcher, a `*_schema()` builder registering methods with capability/subid IDs and schemars x-oscal extensions, fail-closed), never as a registry, registry table or registry lookup. Do NOT implement this task or any of its subtasks (6.1–6.7). The Rust `registry` module (`derive`, `tool_names`, `RegistrySnapshot` derivation) and the `RegistryEntry`/`RegistrySnapshot`/`CatalogPin`/`SideEffect`/`UnavailableReason`/`HeldCapabilityResult` types were removed from `crates/op-chat-interface` (2026-11-09); `ToolKey` remains only as a plugin-method identifier (`plugin/method`) on `PendingToolCall`. `src/lib/server/registry/tool-registry.ts` (ToolRegistryImpl) is dead code — leave it alone. Requirement 6 is retired. References to the Tool_Registry elsewhere in this spec are retired too; do not wire them into the Rust backend.
  - [ ] 6.1 Discover and implement an authoritative mounted caller-capability reader (D-11, B-11) — RETIRED, see above
    - The interim always-undetermined capability-source scaffold was removed.
      Do not ship a fake reader. Until an attributed self-grant read is available,
      the registry accepts an explicit undetermined result and offers no tools.
      Required-capability indexes are not held grants. The source is task 19.2.
    - A Denial from any future source produces a Blocker_Record (operation,
      caller, missing capability or policy, error, timestamp).
    - No code path reads `/dev/shm/opdbus/capability-grants.json` or calls
      `op-grants-materializer`.
    - _Requirements: 6.3, 6.6, 6.11, 7.1_
  - [~] 6.2 Implement `registry/tool-registry.ts` `derive` and `aiSdkTools` — RETIRED, see above
    - Pure function of its input: both views' discovery outputs, each with its
      own `CatalogPin`, the held result, the exclusion set and the
      sticky-denied set. Reads no other partition.
    - One entry per distinct `(plugin, method)` pair; `args`, `returns`,
      `requiredCapability`, `sideEffect` (anything not `read` is `mutation`,
      D-15), `idempotent` and `subid` copied unchanged; `viewKind` set to the
      view that exposed it (D-35).
    - Wrong-view schemas (the `threetched` plugin in the `3tched` view, any
      other plugin in the `threetched` view) are excluded from offered and
      unavailable entries and produce one naming-defect Blocker_Record each.
    - An absent view marks its tools unavailable with reason "view absent";
      the present view still derives. Unparseable schemas or a missing
      argument schema make that entry unavailable with a schema typed error.
    - Excluded plugin names come only from
      `config/naming-defect-exclusions.json`; no TypeScript file contains them
      (D-32).
    - Offered iff a Required_Capability is declared, the held result is a set
      containing it, and the tool is not sticky-denied; otherwise unavailable
      with `absent` or `undetermined`.
    - Per-view re-derivation: a pin change in one view re-derives that view's
      entries only, leaving the other view's entries identical.
    - AI SDK tool names `${plugin}__${method}` sanitized to
      `[A-Za-z0-9_-]` with `_${n}` on collision, used only as lookup keys; the
      AI SDK tool definitions carry no `execute`.
    - _Requirements: 6.1, 6.2, 6.3, 6.5, 6.7, 6.8, 6.12, 6.13, 6.14, 6.15, 7.5, 15.4, 20.6, 19.1_
  - [ ]* 6.3 Write property test for registry derivation — RETIRED, see task 6
    - **Property 13: Registry derivation**
    - **Validates: Requirements 6.1, 6.7, 15.4**
  - [ ]* 6.4 Write property test for the offered subset — RETIRED, see task 6
    - **Property 14: Offered tools are a subset of held capabilities**
    - **Validates: Requirements 6.2, 6.3, 6.8**
  - [ ]* 6.5 Write property test for wrong-view exclusion and missing view — RETIRED, see task 6
    - **Property 46: Wrong-view exclusion and missing view**
    - **Validates: Requirements 6.12, 6.13, 6.14**
  - [ ]* 6.6 Write property test for upgrade isolation — RETIRED, see task 6
    - **Property 47: Upgrade isolation**
    - **Validates: Requirements 6.15**
  - [ ]* 6.7 Write property test for registry isolation — RETIRED, see task 6
    - **Property 38: Registry isolation**
    - **Validates: Requirements 20.6**

- [ ] 7. Tool_Call status machine and Partitioned_Store
  - [~] 7.1 Implement `status/tool-call-status.ts`
    - Pure `next(status, event)` over Confirmation_Events, Operation_File
      results, Receipts and read-backs; no model-text input.
    - States and transitions exactly as the design state diagram;
      `completed`, `rejected`, `denied` and `failed` absorbing; `completed`
      only with Receipt plus verified read-back; otherwise `unconfirmed`;
      validation and not-offered refusals are `failed` with kind
      `validation`. `seq` increases per `toolCallId`.
    - _Requirements: 11.1, 11.2, 11.5, 11.6, 11.7, 8.12_
  - [ ]* 7.2 Write property test for the status state machine
    - **Property 27: Status state machine**
    - **Validates: Requirements 11.1, 11.2, 11.5, 11.6, 8.12**
  - [~] 7.3 Implement `state/partitioned-store.ts`
    - `open(principalId)` returns an opaque `PartitionHandle`; only the
      session manager calls it, with the principal id from the bind Receipt.
    - Missing handle or a handle without an active session throws
      `PartitionError` and writes one redacted security event.
    - `drop(handle, chatSessionId)` removes that session's entries only.
      Memory only; a restart drops everything, including pending calls.
    - _Requirements: 20.1, 20.9, 20.11, 9.9_
  - [ ]* 7.4 Write unit tests for the Partitioned_Store
    - Opaque handle enforcement, rejection with no state change, session drop
      scope, restart behaviour.
    - _Requirements: 20.9, 20.11, 9.9_

- [ ] 8. Confirmation_Gate
  - [~] 8.1 Implement `gate/confirmation-gate.ts`
    - `hold` stores a `PendingToolCall` in the session partition with the held
      `inputBytes` and the D-17 digest: lowercase hex SHA-256 over
      `["identity-chat-confirm/1", principalId, chatSessionId, toolCallId, plugin, method, inputText]`.
    - `confirm` is one synchronous compare-and-set (`pending` to `released`)
      with no `await` between check and write; returns `failedCheck` values
      `no-such-call`, `already-consumed`, `rejected`, `expired`, `discarded`,
      `session-mismatch`, `identity-mismatch`, `digest-mismatch`; a failed
      confirm leaves the record unchanged.
    - `reject`, `discardWhere` (catalog change, denial of the tool, session
      end, identity re-check failure, turn stream close) and `expireDue` at
      300 s on an injectable clock (D-21).
    - The executor receives only the held `inputBytes`; client digests serve
      only for comparison. Release of an Unknown_Outcome repetition needs no
      new Confirmation_Event.
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5, 9.7, 9.8, 9.9, 20.7_
  - [ ]* 8.2 Write property test for exactly-once consumption
    - **Property 23: Exactly-once consumption**
    - **Validates: Requirements 9.4**
  - [ ]* 8.3 Write property test for failed confirmations
    - **Property 24: Failed confirmations change nothing**
    - **Validates: Requirements 9.7, 9.8**

- [~] 9. Checkpoint: core components
  - Ensure all tests pass, ask the user if questions arise. Tests run only on
    explicit owner request; otherwise confirm the core compiles in review and
    ask the owner whether to run the suite.

- [ ] 10. Tool_Executor and identity binder against a fake two-view adapter
  - [~] 10.1 Replace the subprocess runner with `exec/operation-file-runner.ts` (D-44)
    - Remove `exec/3tched-fs-runner.ts` and migrate every caller; leave no
      subprocess transport or compatibility wrapper.
    - Implement `MountedNamespace` using direct local file I/O. Resolve
      plugin, method and view from the pinned registry and mount path from
      the server-side binding; verify mount identity, catalog and mounted
      method metadata/schema before submission.
    - Reject missing/changed mounts, unbound or credential-mode mounts and
      paths outside the pinned declaration. Preserve mount-id checks and
      descriptor-relative no-symlink protections; record a blocker if the
      runtime cannot implement them rather than weakening the contract.
    - After validation and mutation confirmation, write at most 1 MiB of
      argument bytes to `call`, sync once, then read the terminal receipt
      from offset zero on the same handle. Re-verify the mount afterwards.
      Keep denials on that handle; no write retry after an uncertain result.
    - Use the 30 s deadline and typed outcomes, distinguishing refusal before
      submission from unknown outcome afterwards. Spawn no child process,
      open no op-dbus socket and write no session credential into the call.
    - _Requirements: 5.10, 8.1, 8.9, 13.10, 1.10, 30.1 to 30.7_
    - Replacement note (2026-11-09): adapter and migrated binder/executor are
      implemented and covered by the passing backend suite. Runtime open remains
      blocked: both deployed mounts lack the current source's `surface` marker.
      Do not weaken identity/view checks or mark live operation verification done.
  - [ ]* 10.2 Write property test for the direct file submission boundary
    - **Property 6: Direct Operation_File submission boundary**
    - **Validates: Requirements 5.10, 8.1, 8.9, 13.10, 30.1 to 30.6**
  - [~] 10.3 Implement the fake two-view Operation_File adapter as a Synthetic_Fixture
    - `tests/fixtures/synthetic/fake-operation-file-adapter.ts`, starting with
      `// SYNTHETIC-FIXTURE: identity-chat-interface` (D-28).
    - Serves two independent fake views (`3tched`, `threetched`) with their
      own catalog pins and identity kinds; implements `OperationFileRunner`;
      records mount path, operation path, input bytes, submission and timing.
    - Scriptable Receipts, `MountedDiagnostic` Denials, timeouts, dropped
      replies, catalog pin changes, malformed output and absent views. Spawns
      no process and opens no mount. Uses only D-28 reserved values.
    - Add a fake clock used by every timer of D-21.
    - _Requirements: 19.10, 8.13_
  - [~] 10.4 Implement `identity/identity-binder.ts` and `identity/public-fields.ts` (PARTIAL: D-25 principal id OPEN, D-24 PROPOSED-DEFAULT)
    - `bind` makes one `identity_sled.get_own_identity {}` call through the
      runner in the session's `3tched` view, 30 s timeout; Denial, timeout or
      unparseable result yields a typed error plus one Blocker_Record and no
      retry through another mount, transport or identity.
    - Principal id derivation behind one function using the D-25 proposed
      default (`genesis`); an identity without it gets a Blocker_Record. The
      live mapping is task 19.8.
    - `recheck` re-reads and compares every field, returning differing field
      names only.
    - `BoundIdentityRecord.fields` excludes declared-secret members;
      `public-fields.ts` projects exactly the D-24 key set.
    - _Requirements: 4.1, 4.2, 4.5, 4.7, 4.8, 4.13, 19.13_
  - [ ]* 10.5 Write property test for the public field projection
    - **Property 12: Public field projection**
    - **Validates: Requirements 4.8**
  - [~] 10.6 Implement `exec/tool-executor.ts` `execute`
    - Order: (1) read the entry's view catalog pin, refuse with "catalog
      changed" if it differs and send nothing; (2) mutation only: refuse
      cross-identity arguments (`session_id`, `principal_id`, `principal`,
      `identity`, `genesis` or an `x-oscal-subid` naming a session or identity
      source) that differ from the Bound_Identity_Record; (3) mutation only:
      `IdentityBinder.recheck`, any difference ends the session; (4) run the
      runner on the mount of the entry's `viewKind` only, with
      `--confirm-side-effects` only when released by the gate; (5) read-back.
    - D-16 classification: terminal diagnostic is a Denial; local refusals
      before sending are `missing-mount` or `schema`; non-zero exit after
      sending with neither receipt nor diagnostic is Unknown_Outcome; a
      receipt with `success: false` is `failed`.
    - D-33: register every declared-secret field value with the Secret_Guard
      before anything else reads the receipt, then replace it with
      `"[redacted: sealed_id]"`.
    - Never route a call through the other view, another mount, identity,
      transport, Door_8090, database edit, raw Incus command or chain call,
      including after a Denial, error or Unknown_Outcome.
    - _Requirements: 4.3, 4.5, 4.10, 4.13, 4.15, 6.10, 7.4, 8.1, 8.13, 8.14, 13.10, 15.1, 20.5, 20.10_
  - [~] 10.7 Handle uncertainty in the mounted runner and executor (D-10, B-13)
    - The unused standalone retry scaffold was removed. No operation is
      automatically replayed; any permitted read retry policy must stay bounded
      by the specification and must not create a second transport implementation.
    - Mutation_Methods: zero repetitions (D-10 default), immediate `unknown`
      status, a Blocker_Record and an unknown-outcome tool result. Any
      mutation repetition or receipt recovery is task 19.5.
    - _Requirements: 8.6, 9.4, 15.8_
  - [~] 10.8 Implement `exec/read-back.ts` (PARTIAL: D-14)
    - Look up `config/read-back.json` (`plugin.method` to
      `{ plugin, method, args_from }`); run the mapped Read_Method through the
      same view; with no mapping, a Denial or an error, return the receipt
      with `readBack.verified = false` and no repetition of the original call.
    - The map ships empty, so every mutation ends `unconfirmed` until task
      19.4.
    - _Requirements: 8.5, 8.12, 11.6_
  - [ ]* 10.9 Write property test for the identity re-check
    - **Property 10: Identity re-check guards every mutation**
    - **Validates: Requirements 4.5, 4.13**
  - [ ]* 10.10 Write property test for the call-input round trip
    - **Property 19: Accepted arguments round-trip to the call input**
    - **Validates: Requirements 8.9, 9.2**
  - [ ]* 10.11 Write property test for cross-identity refusal
    - **Property 11: Cross-identity mutation targets are refused**
    - **Validates: Requirements 4.15, 4.10**
  - [ ]* 10.12 Write property test for view routing
    - **Property 45: View routing**
    - **Validates: Requirements 8.13, 8.14**
  - [ ]* 10.13 Write property test for Unknown_Outcome bounds
    - **Property 21: Unknown_Outcome repetition bounds**
    - **Validates: Requirements 8.6, 15.8**
  - [ ]* 10.14 Write property test for held-input execution
    - **Property 20: Executed mutation input is the held input**
    - **Validates: Requirements 9.2**
  - [ ]* 10.15 Write property test for mount routing per Bound_Identity
    - **Property 41: Mount routing per Bound_Identity**
    - **Validates: Requirements 4.3, 20.5, 20.10**
  - [ ]* 10.16 Write unit tests for executor classification and redaction
    - D-16 cases, D-33 `sealed_id` redaction, read-back present and absent,
      catalog-changed refusal, missing-mount refusal.
    - _Requirements: 8.5, 8.12, 15.1, 15.4_

- [ ] 11. Chain_Verifier
  - [~] 11.1 Implement `chain/chain-verifier.ts` `verify` (continuity only, D-30)
    - Pure; accepts only segments with `source: "operation-file"`;
      `continuity` broken iff some `i >= 1` has
      `records[i].prevHash !== records[i - 1].execHash`; `firstBrokenIndex`
      the least such `i`; anchor never checked.
    - Verdict precedence broken over unverified over verified; displays an
      exposed `verify_integrity` flag; no `hash_execution` recomputation
      (Req 10.5 not authorized).
    - _Requirements: 10.3, 10.4, 10.8, 10.9, 10.10_
  - [ ]* 11.2 Write property test for continuity equivalence
    - **Property 25: Continuity equivalence**
    - **Validates: Requirements 10.3, 10.8**
  - [ ]* 11.3 Write property test for verdict precedence
    - **Property 26: Verdict precedence**
    - **Validates: Requirements 10.4, 10.10, 10.11**
  - [~] 11.4 Implement segment reading and receipt linking with interim results (PARTIAL: D-12, D-13, B-12)
    - Define the segment-reader interface (reads only through the
      Tool_Executor in the session's own view). Ship the interim reader that
      reports `unverified` with reason "no Execution_Record read granted" and
      writes one Blocker_Record per Branch.
    - Every Tool_Call is `unlinked` with reason "mapping not recorded" until
      D-13 is resolved. Receipts and earlier records are never changed.
    - The real read and mapping are task 19.3.
    - _Requirements: 10.1, 10.6, 10.7, 10.11_

- [ ] 12. Model_Context_Builder
  - [~] 12.1 Implement `context/system-prompt.ts` and `context/model-context-builder.ts`
    - System prompt: the D-26 text verbatim (PROPOSED-DEFAULT), slots filled
      once per session from Public_Identity_Fields.
    - `build(partitionHandle, session)` uses only the session's own partition;
      tool results only as `tool` role messages linked by `toolCallId`, after
      the D-33 projection; fixed result shapes for denial
      (`{ denied, plugin, method, missing, error }`), validation failure,
      not offered, rejection, expiry, limit reached, catalog changed and
      unknown outcome; tool definitions for offered entries only.
    - Every built context passes `SecretGuard.checkItem` before any provider
      call.
    - _Requirements: 12.3, 12.5, 14.1, 14.6, 5.1, 7.6, 8.3, 8.4, 9.5, 9.8, 20.4_
  - [ ]* 12.2 Write property test for context construction
    - **Property 30: Context construction**
    - **Validates: Requirements 12.3, 14.1, 14.6, 20.4**
  - [ ]* 12.3 Write property test for credential-free contexts
    - **Property 5: No Credential_Material in any Model_Context**
    - **Validates: Requirements 12.5, 5.1, 12.3**

- [~] 13. Checkpoint: executor, chain and context
  - Ensure all tests pass, ask the user if questions arise. Tests run only on
    explicit owner request.

- [ ] 14. Chat_Route wiring behind `ChatRouteHost` with the in-process harness
  - Model calls in this task use only an injected AI SDK `LanguageModel`; tests
    and the harness use the mock model from `ai/test`. No provider package is
    constructed here (D-04, task 19.9).
  - [~] 14.1 Define the host adapter boundary in `route/hosts/host-adapter.ts`
    - `ChatRouteHost`, `HostBinding` (profile file source and both view mount
      paths), `ChatRouteCore`, `SessionKey`, `DeploymentTargetKind`,
      `PathRecord`. The session key comes from the host's server-side session
      table, never from a request body. Adapters add no listening socket.
    - _Requirements: 13.2, 13.10, 20.1, 4.2_
  - [~] 14.2 Implement session start and end in `route/chat-route.ts`
    - Start: read-only observation of `.3tchedfs.json` and `view.json` in both
      views (D-08); derive the Deployment_Target from `mount_identity.kind`;
      require each host binding path to equal the profile's `mount_paths`
      entry; load the profile; bind; compare `principal_id`,
      `expected_session_id` and any display name; check the target against
      `deployment_targets`; open the partition with the bound principal id;
      read the Held_Capability_Set; derive the registry; when the history
      capability is held, call `identity_sled.get_session_history`
      `{ session_id, limit: 100 }` (30 s; Denial or timeout continues without
      history and writes a Blocker_Record); stream `data-session`.
    - Any failure before binding completes sends no model request and offers
      no tool; refusals write Blocker_Records naming fields without
      Credential_Material. Profile values are frozen for the session.
    - No system-initiated `identity_sled` write (D-27).
    - End: discard that session's pending calls only and drop its partition
      entries.
    - _Requirements: 1.10, 4.1, 4.2, 4.4, 4.6, 4.7, 4.9, 4.14, 6.1, 6.14, 15.1, 15.5, 19.2, 19.4, 19.8, 19.12, 19.13, 20.1, 20.11_
  - [~] 14.3 Implement `route/turn-loop.ts`
    - One operator message starts one turn; AI SDK `streamText` with offered
      tools and no `execute`.
    - Per structured tool-call part: increment the turn counter first; stop
      dispatching and requesting model responses past the Session_Mode limit
      and stream one `data-limit` part.
    - Check order: offered (registry entry by tool name), Schema_Validator,
      Confirmation_Gate for mutations, Tool_Executor. Tool names or
      tool-call-shaped data in text or tool results dispatch nothing.
    - Catalog pins checked before each model request: on change, re-derive,
      re-read capabilities and discard pending calls.
    - Denial handling: Blocker_Record before the `denied` status (UI shows a
      "Blocker_Record not created" error if the write fails), sticky
      unavailability, discard pending calls naming the tool, denial tool
      result.
    - Stream text through the stream guard and status through
      `data-tool-status` parts from `next()`; pending mutations keep the turn
      open up to 300 s; closing the stream discards the turn's pending calls.
    - Provider error or no response in 60 s: `provider` typed error, no
      dispatch from the partial response, existing calls unchanged.
    - Session_Mode changes only the limit; no other component takes a mode
      argument.
    - _Requirements: 6.5, 6.10, 7.2, 7.4, 7.5, 7.6, 7.7, 7.9, 8.2, 8.3, 8.4, 8.10, 11.1, 11.2, 11.4, 11.5, 14.3, 14.5, 14.7, 14.9, 15.3, 15.6_
  - [~] 14.4 Implement decisions, re-verification and request rejection in `route/chat-route.ts`
    - `handleDecision` accepts only `{ type: "approve", toolCallId, digest }`
      or `{ type: "reject", toolCallId }`, attributes them through the
      server-side `SessionKey`, and refuses decisions for denied tools.
    - Reject any request field the route does not expect, including client
      tool definitions, capability claims, offered markings, Tool_Calls,
      Session_Mode or limit values, identity or mount values.
    - `reverify` re-reads the segment through task 11.4 and streams
      `data-chain` with the read time.
    - Items tagged with a principal other than the addressed session are
      rejected with no state change and a redacted security event naming both
      display names.
    - _Requirements: 6.9, 7.8, 8.11, 9.2, 9.3, 9.5, 9.7, 10.9, 10.10, 14.8, 20.3, 20.7_
  - [~] 14.5 Implement `route/chat-sdk-intake.ts` (PARTIAL: D-02 sub-check B-15; needs task 19.1 to read installed sources)
    - `chat` + `@chat-adapter/web` + `@chat-adapter/state-memory`; `getUser`
      returns `{ id: base64url(principalId) }` from the server-side session or
      `null`; thread id `web:{base64url(principalId)}:{conversationId}`;
      redacting logger from task 3.5; only the last user message's text is
      used.
    - Before relying on `thread.post` for `data-*` parts, confirm against the
      installed package source (B-15). Record a supported integration that
      retains Chat SDK intake and AI SDK output, carrying all chunks
      unchanged over the Chat_Link. If none is available, record the blocker;
      do not drop Chat SDK or substitute an HTTP/SSE browser endpoint.
    - _Requirements: 6.9, 8.11, 20.1, 3.8_
  - [~] 14.6 Implement `route/hosts/in-process.ts`
    - Drives `ChatRouteCore` with Fetch `Request` objects built in-process,
      uses the fake two-view adapter and the `ai/test` mock model, opens no
      socket, and delivers each tagged item only to the client whose
      server-side `SessionKey.principalId` equals the tag.
    - Not a Deployment_Target; it is the only adapter in scope (D-06).
    - _Requirements: 13.2, 20.2_
  - [ ]* 14.7 Write property test for binding gating the session
    - **Property 7: Binding gates the session**
    - **Validates: Requirements 4.1, 4.7, 19.4, 19.13**
  - [ ]* 14.8 Write property test for start refusal
    - **Property 8: Start refusal on any identity or target mismatch**
    - **Validates: Requirements 4.4, 19.8, 19.12**
  - [ ]* 14.9 Write property test for untrusted values selecting nothing
    - **Property 9: Untrusted values select nothing**
    - **Validates: Requirements 1.10, 4.2, 6.9, 8.11, 10.9, 14.4, 14.8**
  - [ ]* 14.10 Write property test for catalog change handling
    - **Property 15: Catalog change re-derives and discards**
    - **Validates: Requirements 6.5, 6.10**
  - [ ]* 14.11 Write property test for denial finality
    - **Property 16: A denial is final for its method in the session**
    - **Validates: Requirements 7.2, 7.4, 7.5, 7.8, 7.9**
  - [ ]* 14.12 Write property test for the dispatch pipeline
    - **Property 17: Dispatch pipeline**
    - **Validates: Requirements 8.1, 8.2, 8.4, 8.10, 14.3, 14.5**
  - [ ]* 14.13 Write property test for confirmation before mutation
    - **Property 22: No mutation without an accepted confirmation**
    - **Validates: Requirements 9.1, 9.3, 9.6, 14.3**
  - [ ]* 14.14 Write property test for status independence from model text
    - **Property 28: Status is independent of model text**
    - **Validates: Requirements 11.4, 11.7**
  - [ ]* 14.15 Write property test for the tool-call limit
    - **Property 34: Tool-call limit per operator message**
    - **Validates: Requirements 14.7**
  - [ ]* 14.16 Write property test for raised modes changing only the limit
    - **Property 35: Raised modes change only the limit**
    - **Validates: Requirements 14.9**
  - [ ]* 14.17 Write property test for no cross-Branch confirmation
    - **Property 39: No cross-Branch confirmation**
    - **Validates: Requirements 20.7**
  - [ ]* 14.18 Write property test for non-interference
    - **Property 40: Non-interference**
    - **Validates: Requirements 20.8, 20.1**
  - [ ]* 14.19 Write property test for partition rejection
    - **Property 42: Partition rejection**
    - **Validates: Requirements 20.2, 20.3, 20.9, 20.11**
  - [ ]* 14.20 Write unit and example tests for route error paths
    - One example per typed error kind (15.1 to 15.8); denial display and
      failed Blocker_Record write (7.3, 7.7); history allowed, denied and timed
      out (4.6, 4.14); reject flow (9.5); restart and session end discard
      (9.9); "no Execution_Record read granted" (10.6); provider configuration
      missing or mismatched (12.7) against an injected configuration check;
      profile frozen at start (19.2).
    - _Requirements: 4.6, 4.14, 7.3, 7.7, 9.5, 9.9, 10.6, 12.7, 15.1, 15.2, 15.3, 15.4, 15.5, 15.6, 15.7, 15.8, 19.2_

- [ ] 15. Chat_UI for the backend contract (UI LAST: after backend, transport and model-selection wiring)
  - [~] 15.1 Implement the json-render Catalog and Component_Registry
    - `src/lib/catalog/catalog.ts` and `registry.ts` with Svelte components
      `ModelOutput` (labelled "model output"), `ToolCallCard`, `ErrorNotice`
      (kind, failing item, next step), `IdentityHeader` (Public_Identity_Fields
      only), `ToolList` (offered and unavailable tools with plugin, method,
      capability or "none declared", reason), `ChainPanel` (records in read
      order with `prev_hash`, `exec_hash`, link, per-link result, read time,
      display-cache label) and `ConfirmationPrompt` (plugin, method, args,
      capability, display name, approve and reject controls).
    - All untrusted text rendered as literal text; a spec with an
      undeclared or unimplemented element type is refused whole with a
      Catalog error naming the type.
    - Accessible names, keyboard operation, polite live region, focus to and
      from `ConfirmationPrompt`, text status labels besides color, WCAG 2.2
      AA contrast tokens.
    - _Requirements: 2.1, 2.5, 2.6, 4.8, 6.4, 7.3, 8.5, 9.1, 10.1, 10.2, 10.7, 11.3, 14.2, 15.6, 16.1, 16.2, 16.3, 16.4, 16.5, 16.6_
  - [~] 15.2 Implement the client status store, decisions and transport (PARTIAL: D-06, B-04, B-05)
    - `src/lib/client/chat-transport.ts`: implement the Vercel AI SDK
      `ChatTransport` contract over the protobuf gRPC-Web Chat_Link (D-03).
      Decode each server-stream frame into its unchanged AI SDK chunk in
      arrival order. Use unary RPCs for decisions, re-verification and hints,
      and server streams for responses and per-plugin feeds. The production
      host connection is gated by task 19.10; a harness is not a fallback.
    - Status store keeps, per `toolCallId`, only the highest-`seq`
      `data-tool-status` part; derives no status locally.
    - `decisions.ts` sends approve and reject only from `ConfirmationPrompt`
      controls; 10 s connection timeout produces a `transport` error notice
      with no other path tried.
    - Nothing in localStorage, sessionStorage, IndexedDB, script-readable
      cookies, URLs or the service worker cache. `src/routes/+page.svelte`
      composes the Catalog elements with `@ai-sdk/svelte` `Chat` (D-03).
    - The Chat_UI sends no request to a Model_Provider.
    - _Requirements: 5.4, 11.3, 13.9, 15.7, 9.3, 32.3, 32.4, 32.5, 32.6_
  - [ ]* 15.3 Write property test for literal rendering
    - **Property 31: Literal rendering of untrusted text**
    - **Validates: Requirements 14.2**
  - [ ]* 15.4 Write property test for Catalog closure
    - **Property 32: Catalog closure of rendered specs**
    - **Validates: Requirements 2.5, 2.6**
  - [ ]* 15.5 Write property test for the UI status store
    - **Property 29: UI shows the latest streamed status**
    - **Validates: Requirements 11.3**
  - [ ]* 15.6 Write accessibility and storage component tests
    - Keyboard-only operation, accessible names, live region announcements,
      focus movement, text status labels, contrast against theme tokens,
      browser storage untouched.
    - _Requirements: 16.1, 16.2, 16.3, 16.4, 16.5, 16.6, 5.4_

- [ ] 16. Repository checks
  - [~] 16.1 Implement `scripts/check-forbidden-imports.ts`
    - Fails on React, JSX or TSX files, Next.js, `@ai-sdk/mcp`,
      `@modelcontextprotocol/*`, AI SDK symbols containing `MCP`, imports
      resolving into `/srv/git/3tched.com` or `/srv/git/odbus-verified`, any
      reference to the grant file or `op-grants-materializer`, and imports
      from the removed legacy chat code listed in the Legacy_Chat_Inventory.
    - Scans the client import graph (`src/lib/client`, `src/lib/catalog`,
      `src/routes`) for Pinned_Package_Set entries D-01 records as
      Chat_Route only.
    - _Requirements: 2.3, 2.4, 3.5, 3.7, 7.1, 13.9, 21.1_
  - [~] 16.2 Implement `scripts/check-branch-literals.ts` (PARTIAL: D-18 envelope rule interim; runtime-value row needs task 1.2)
    - D-29 categories: session-id, principal-or-genesis, public-key,
      container-name, runtime-value, plugin-allowlist, capability-list,
      credential. Prints `path:line:category`, never the value; exits 1 on
      any finding; exempts only D-28 reserved values inside
      Synthetic_Fixtures; reports Runtime_Record values found inside
      fixtures.
    - _Requirements: 19.1, 19.9, 19.10, 5.9_
  - [ ]* 16.3 Write property test for the branch-literal check
    - **Property 44: Branch-literal check**
    - **Validates: Requirements 19.9, 19.10, 5.9**
  - [~] 16.4 Implement `scripts/check-pins.ts`
    - Compares the D-01 table in `design.md`, `package.json` specifiers and
      `package-lock.json` resolutions; reports differing versions and every
      non-exact specifier (ranges, tags, wildcards, `latest`, `workspace:`);
      exits 1 iff it reports a finding. Reports a missing lockfile as a
      finding until task 19.1.
    - _Requirements: 3.3, 3.9_
  - [ ]* 16.5 Write property test for the pin drift check
    - **Property 43: Pin drift check**
    - **Validates: Requirements 3.3, 3.9**
  - [~] 16.6 Implement `scripts/check-catalog.ts`
    - Reports every visible element used in `src/routes` and
      `src/lib/catalog` components that lacks a Catalog declaration or a
      Component_Registry implementation.
    - _Requirements: 2.7_
  - [~] 16.7 Wire the checks and credential scan into `package.json` scripts
    - Add `check:*` scripts for the four checks; extend the Cockpit_Spec
      Requirement 8.5 credential scan configuration to cover
      `apps/identity-chat/**`. Scripts run only on explicit owner request.
    - _Requirements: 5.9, 17.2_

- [~] 17. Checkpoint: route, UI and checks
  - Ensure all tests pass, ask the user if questions arise. Tests, linters,
    formatting checks and smoke runs happen only on explicit owner request.

- [ ] 18. Provenance and status records
  - [~] 18.1 Add the Chat_Interface entries to `PROVENANCE.md` and the Status_Report
    - Each component with path and producing task ids; each pinned
      dependency (op-dbus modules, `identity_sled`, execution tracker, grant
      materializer, Operation_File contract, Door_8090, Op_Web_Connector,
      Pinned_Package_Set) with path or package and commit or version; cite
      this spec, the Cockpit_Spec and the steering files.
    - Status per component and, for deployed, per Branch and
      Deployment_Target, with proposed, implemented, built, tested and
      deployed stated separately.
    - _Requirements: 18.1, 18.2, 18.3, 18.4, 18.6_

- [ ] 19. Blocked integration tasks (proposed status until each gate clears)
  - Each sub-task names its gating IDs. Do not start a sub-task until every
    gate is resolved in `design.md` (decisions) or the Runtime_Record
    (blockers). Any Live_Change inside them also needs Owner_Authorization.
  - [~] 19.1 Create the lockfile and confirm package facts (BLOCKED: D-01 confirmation, D-34, explicit owner request)
    - **BLOCKED (proposed):** runs `npm install` in `apps/identity-chat/` only
      when the owner asks and after the owner confirms the D-01 versions.
    - Commit `package-lock.json`; confirm from installed sources the UI
      message stream protocol version each entry emits or consumes, the
      React/JSX/Next.js and runtime peers (Blocker_Record if any violates
      Req 2 or 13), and resolve B-15 for task 14.5.
    - _Requirements: 3.3, 3.4, 3.6, 3.8_
  - [~] 19.2 Implement the real Held_Capability_Set reader (BLOCKED: D-11, B-11, B-02)
    - **BLOCKED (proposed):** needs the source discovered in task 1.3 and
      recorded in D-11 without naming a Branch.
    - Replace the interim source of task 6.1 with a reader that calls the
      recorded Read_Method through the session's own view and parses the
      result; Denial, no Receipt or unparseable result stays `undetermined`.
    - _Requirements: 6.2, 6.3, 6.6, 6.11_
  - [~] 19.3 Implement the Execution_Record read and Receipt linking (BLOCKED: D-12, D-13, B-12, B-02)
    - **BLOCKED (proposed):** needs the granted read operation (D-12) and the
      Receipt-to-record mapping (D-13).
    - Replace the interim reader of task 11.4; link Tool_Calls by the
      recorded execution identifier; keep `unlinked` reasons for missing
      identifiers or records.
    - _Requirements: 10.1, 10.2, 10.6, 10.10, 10.11_
  - [~] 19.4 Populate `config/read-back.json` (BLOCKED: D-14, B-02)
    - **BLOCKED (proposed):** add only mappings confirmed in mounted schemas
      (candidates `identity_sled.touch_session` to `get_own_identity`,
      `record_session_event` to `get_session_history`).
    - _Requirements: 8.5, 11.1_
  - [~] 19.5 Mutation Unknown_Outcome reconciliation (RESOLVED 2026-11-09: no repetition)
    - Requirement 8.6 was amended: Mutation_Methods are never repeated. Task
      10.7's zero-repetition behaviour is the final behaviour; nothing more
      to implement here unless a later amendment adds idempotent repetition
      or receipt recovery.
    - _Requirements: 8.6, 9.4, 15.8_
  - [~] 19.6 Add the excluded plugin names (BLOCKED: D-32, B-14)
    - **BLOCKED (proposed):** owner decision on Requirement 6.7 against
      Cockpit_Spec 25.2. If confirmed, add the two names only to
      `config/naming-defect-exclusions.json`, citing the Baseline_Record.
    - _Requirements: 6.7_
  - [~] 19.7 Implement the real SID1 and OIA1 envelope grammar (BLOCKED: D-18 part, B-02 for discovery if the grammar is runtime-only)
    - **BLOCKED (proposed):** read the grammar from the pinned identity source,
      record it in D-18, then replace the interim envelope rule in tasks 3.1
      and 16.2.
    - _Requirements: 5.5, 19.9_
  - [~] 19.8 Fix the principal id mapping (BLOCKED: D-25 under D-23, B-02)
    - **BLOCKED (proposed):** confirm whether `genesis`, `wireguard_pubkey` or a
      dedicated runtime field is the principal id, record it in D-25, and
      update the single derivation function of task 10.4.
    - _Requirements: 4.4, 19.12, 20.1_
  - [~] 19.9 Implement `provider/model-provider.ts` (BLOCKED: D-04, D-05, D-07, B-06, B-07, B-08)
    - **BLOCKED (proposed):** needs the Model_Provider, exact model identifier
      and version, retention terms, the provider package entry in D-01, the
      Provider_Credential location per Deployment_Target and a named
      Egress_Path.
    - Then: read the credential server-side at process start from the
      recorded location, register it with the Secret_Guard as
      `provider-credential`, construct only the recorded provider and model
      with no fallback, refuse with a `configuration` error when the
      credential is unreadable or the configured model differs, and send
      requests only after the guard check, outbound only over the recorded
      Egress_Path.
    - _Requirements: 12.1, 12.2, 12.4, 12.6, 12.7, 12.8, 13.6, 13.8_
  - [~] 19.10 Implement the gRPC-Web Chat_Link host adapter and SDK transport (BLOCKED: D-06, D-45, B-04, B-05, B-24)
    - **BLOCKED (proposed):** record the actual existing ingress-to-TypeScript
      connection, including endpoint, protocol, owner, permissions and both
      process identities. Do not treat `.add_service` or the existence of
      Lungs_Socket as proof of a working hop.
    - Define Chat_Link messages in a protobuf file in the Hackathon_Repository.
      Use unary and server-streaming RPCs only. Map requests into Chat SDK
      intake; frame AI SDK output chunks unchanged after Secret_Guard checks.
      Preserve content and order on browser decoding and test that round trip.
    - Connect `@ai-sdk/svelte` through the custom `ChatTransport` of task 15.2.
      No HTTP/SSE endpoint, native browser gRPC or fallback transport.
    - Preserve Tonic's existing non-chat backend mux services. Use Tonic for
      browser carriage where the recorded ingress requires it; any pinned
      connection change requires separate dated Owner_Authorization.
    - Verify that the listening-socket set is unchanged. The Chat_Route opens
      no listener; any local socket hop uses only Lungs_Socket as a client,
      after its required protocol support is established. Keep containers on
      their existing host channel with no Incus proxy, `egress-socket`,
      HTTP proxy or extra socket.
    - Record absent/refused path errors without trying another path. Gate
      deployment separately; this spec correction authorizes no Live_Change.
    - _Requirements: 13.1, 13.2, 13.3, 13.4, 13.5, 13.7, 15.7, 32.1 to 32.12_
  - [~] 19.11 Enable the Identity_Container executor path (BLOCKED: B-02; B-10 for affected output)
    - **BLOCKED (proposed):** needs the identity's mounted views and verified
      deployed file protocol (D-44). Historical CLI flag blocker B-09 does
      not gate direct file I/O. Any task relying on `refresh_credential`
      output still waits for B-10. No live run without named authorization.
    - _Requirements: 1.7, 5.10, 30.1 to 30.7_
  - [~] 19.12 Route 3tchedW3 calls through the Chatbot_Glue (BLOCKED: B-03, B-02)
    - **BLOCKED (proposed):** needs the 3tchedW3_Identity and the Cockpit_Spec
      Requirement 18 glue; calls the glue's existing interface only.
    - _Requirements: 8.7_
  - [~] 19.13 Enable host 3tchedFS views (BLOCKED: B-01)
    - **BLOCKED (proposed):** only after a Runtime_Record entry timestamped
      after the deployed catalog fix shows the host view mounted. No other
      mount path, no catalog edit, no fix deployment without
      Owner_Authorization.
    - _Requirements: 1.11_
  - [~] 19.14 Run the Chat_Interface under a Branch (BLOCKED: Owner_Authorization naming the Branch, plus 19.9, 19.10 and B-01 or B-02)
    - **BLOCKED (proposed):** a Live_Change under Cockpit_Spec Requirement 20.
    - _Requirements: 4.12, 17.1_

- [ ] 20. Cut the `ctl_plane_chatbot` coupling to `chat_router` in op-dbus
  - Owner exception for chat code under steering `chat-clean-slate.md`.
    Requirement 17.4 ("leave `/srv/git/odbus-verified` unmodified") is
    withdrawn; the change still needs the Owner_Authorization of
    Requirement 21.7. `chat_router` itself is a threetched-backed service and is not
    removed.
  - [~] 20.1 Claim and trace the coupling (read-only first)
    - Claim `crates/op-plugins/src/state_plugins/ctl_plane_chatbot.rs` and
      `ctl_plane_chatbot_methods.rs` under the op-dbus HANDOFF rules (one
      owner per file). Check that no other agent's build, seal or activation
      is running.
    - Trace every use of `chat_router::{ChatInput, ChatMessage, ChatOutput}`,
      `MAX_CHAT_REQUEST_BYTES`, `MAX_CHAT_RESPONSE_BYTES` and
      `plugin_id: Some("chat_router")`, plus callers in
      `crates/op-grpc-bridge/src/ctl_plane_chatbot_dispatch.rs` and
      `crates/op-cozo-store/src/ctl_plane_chatbot_episodes.rs`.
    - _Requirements: 22.6, 22.8, 21.7_
  - [~] 20.2 Replace the coupled types, limits and plugin id with ones owned by `ctl_plane_chatbot`
    - Keep every other method, record field and the schema contract except
      the removed `chat_router` coupling; no shim.
    - _Requirements: 22.6, 22.8_
  - [~] 20.3 Verify the cut (BLOCKED for the live part: B-01 or B-02, Owner_Authorization)
    - Scoped build of the owning crate only:
      `cargo build -p op-plugins --target x86_64-unknown-linux-gnu`, never
      alongside another Cargo process. Search shows no `chat_router` reference
      left in `ctl_plane_chatbot`.
    - **BLOCKED (proposed):** catalog reseal, both-view mount and a granted
      `ctl_plane_chatbot` read through the `3tched` view with Receipt and
      read-back are Live_Changes needing Owner_Authorization and a mount.
    - On a broken build or service: restore from version control and record a
      Blocker_Record.
    - _Requirements: 22.8, 21.5, 21.6_

- [ ] 21. Remove legacy chat code (one claim per inventory item)
  - Applies only to items task 1.4 classified as chat code and confirmed by
    caller trace. Never touches `chat_router*` or other threetched-backed
    services, `ctl_plane_chatbot`, `chat_fs`, `chatbot-identity`, the
    control-plane identity or any identity, grant or storage binding.
  - Removals in `/srv/git/3tched.com` need the Owner_Authorization of
    Requirement 21.7 (Requirement 17.4 is withdrawn).
  - [~] 21.1 Remove one confirmed chat-code item completely
    - Claim under the owning repository's HANDOFF rules; never during
      another agent's build, seal or activation.
    - Remove source, module declarations, Cargo membership, plugin
      registration, sealed schema entry, generated code, routes, runit
      services, image units, UI pages, tests, fixtures and docs that describe
      it as current. No remnant, no shim. Repeat per item with one focused
      commit each.
    - _Requirements: 21.3, 21.7_
  - [~] 21.2 Verify each removal (BLOCKED for the live part: B-01 or B-02, Owner_Authorization)
    - Repository search shows no reference to the removed symbols, paths,
      plugin ids or service names outside archived history; scoped
      `x86_64-unknown-linux-gnu` build of the affected crates only.
    - **BLOCKED (proposed):** catalog reseal without the removed plugin and a
      granted read with Receipt and read-back through each of the two views
      need a mount and Owner_Authorization; deploying the removal is a
      Live_Change.
    - _Requirements: 21.5_
  - [~] 21.3 Restore and record on failure
    - If a removal breaks the scoped build, a remaining service or either
      view, restore the item from version control, write a Blocker_Record
      naming the dependency and leave that removal proposed.
    - _Requirements: 21.6_

- [ ] 22. Cut the control-plane identity over to the new Chat_Interface (BLOCKED: Owner_Authorization, tasks 20 and 21, 19.9, 19.10, B-01 or B-02)
  - The identity, its session, credentials, grants, storage and container
    binding stay unchanged. No identity-specific code path is added.
  - [~] 22.1 Bind the control-plane identity as an ordinary Branch
    - **BLOCKED (proposed):** write its Identity_Profile as data (no code),
      with both view paths and its allowed Deployment_Target, after the
      Chat_Route location (D-06) and Egress_Path (D-07) exist.
    - `ctl_plane_chatbot` methods enter the Tool_Registry from the `3tched`
      view under the normal capability, validation and confirmation rules.
    - _Requirements: 22.1, 22.2, 22.3, 22.7_
  - [~] 22.2 Verify the cutover
    - **BLOCKED (proposed):** Live_Change. Start a Chat_Session under the
      identity; record the `get_own_identity` Receipt, one granted read
      Tool_Call Receipt with read-back, and a search showing no call from the
      identity's services into removed chat code.
    - _Requirements: 22.4_
  - [~] 22.3 Record an unreachable cutover
    - If no recorded existing path reaches the Chat_Interface, write a
      Blocker_Record and keep the identity and its state unchanged.
    - _Requirements: 22.5_

- [~] 23. Final checkpoint
  - Ensure all tests pass, ask the user if questions arise. Update the
    Status_Report (task 18.1) with proposed, implemented, built, tested and
    deployed status per component.

- [ ] 24. Gate schema and state visibility in the 3tchedFS mount (D-48; owner, 2026-11-09: "fuse it is")
  - Language: Rust. Every change in this task is in the existing Rust crates
    `crates/_3tched-fs` and `crates/op-grpc-bridge` of `/srv/git/odbus-verified`.
    No TypeScript and no Chat_Interface code is changed by this task.
  - [ ] 24.1 Claim and trace (read-only first)
    - Claim the affected `crates/_3tched-fs` files under the op-dbus HANDOFF
      rules; never during another agent's build, seal or activation.
    - Trace where the namespace builds method directories, `input.schema.json`,
      `metadata.json` and `capabilities/` entries (`namespace.rs`), how file
      modes are set (`fuse_fs.rs`, currently 0444 for schemas), and where the
      mount's kernel-bound caller and its held capabilities are known.
    - _Requirements: 25.1, 25.2, 30.1_
  - [ ] 24.2 Expose only granted methods and state
    - Build each caller's view so a method's directory and schema files exist
      only when the caller holds its `required_capability`, and a plugin's
      state only when the caller holds its state-read capability. No
      placeholder entry, error file or name leak for hidden methods.
    - Re-evaluate on grant change so a revoked grant hides the entries
      (Requirement 25.8, 25.9).
    - Replace the `zero_ttl_private_calls_and_public_schema` expectation with
      one asserting that an ungranted method's schema is absent.
    - _Requirements: 25.1, 25.2, 25.9, 25.13_
  - [ ] 24.3 Align the bridge
    - Apply the same rule to `PluginService/GetSchema` and
      `StateSync/Subscribe` in `op-grpc-bridge`, so no path shows more than the
      mount (D-48, B-32).
    - _Requirements: 25.1, 25.2, 25.13_
  - [ ] 24.4 Scoped build and Live_Change
    - Scoped `x86_64-unknown-linux-gnu` build only, one Cargo process
      (Requirement 17.3). Deploying is a Live_Change needing Owner_Authorization
      (Requirement 17.1); tests run only as steering allows (17.2).
    - _Requirements: 17.1, 17.3_

## Notes

- Tasks marked `*` are optional test sub-tasks and can be skipped for a
  faster MVP. Core implementation tasks are never optional.
- Writing test files is part of the plan. Running tests, linters,
  formatting checks and smoke fixtures happens only on explicit owner
  request (Requirement 17.2, steering `security-boundaries.md`). No
  workspace-wide Cargo command; never two Cargo processes at once.
- Each property test uses `fast-check` 4.10.2, runs at least 100 iterations
  (`fc.assert(prop, { numRuns: 100 })` or more) and starts with the tag
  `// Feature: identity-chat-interface, Property N: <property title>`.
  Properties 1 to 47 each have exactly one sub-task.
- All tests run against the in-process host, the `ai/test` mock model, the
  fake two-view Operation_File adapter, the fake clock and
  Synthetic_Fixtures (D-28). No test touches a live mount, the
  Lungs_Socket, Door_8090, the Op_Web_Connector, a Model_Provider, a
  container or a grant.
- Jev answers are evidence, not authority. They never authorize a removal,
  a Live_Change or a grant change.
- Commits: one focused commit per task inside the Submission_Window, `.kiro`
  committed, no backdating (Requirement 18.5). This planning step made no
  commit.
- Requirement 17.4 (leave `/srv/git/odbus-verified` and `/srv/git/3tched.com`
  unmodified) is withdrawn because it conflicted with Requirements 21 and 22
  and steering `chat-clean-slate.md`. Changes there still need the
  Owner_Authorization of Requirement 21.7.
- Requirement 4.11 is met by D-27 (no system-initiated `identity_sled`
  writes); no task adds one.

### Status

| Field | Value |
|---|---|
| Proposed | This task plan, 2026-11-08. Every task whose checkbox is unchecked. |
| Implemented | Partial (2026-11-09): backend modules exist under `apps/identity-chat/` in the Hackathon_Repository; see `design.md` Status, "Backend replacement verification — 2026-11-09". Checkboxes have not been reconciled with that code and do not claim completion. |
| Built | `npm install` run; `package-lock.json` committed state per `design.md` Status. No Cargo command. |
| Tested | 2026-11-09: `npm run check` and `npm test` (52 tests, no failures) under Node v26.13.0, in-process only. No browser, provider or complete chat turn tested. Not deployment evidence. |
| Deployed | Nothing, on any Branch or Deployment_Target. No Live_Change. |

### Gate index

| Gate | Tasks held |
|---|---|
| D-01 confirmation, D-34, owner request | 19.1 (and the lockfile checks in 16.4, task 14.5's B-15 check) |
| D-02 sub-check, B-15 | 14.5 (partial) |
| D-04, D-05, D-07, B-06, B-07, B-08 | 19.9, 19.14, 22 |
| D-06, B-04, B-05 | 15.2 (partial), 19.10, 19.14, 22 |
| D-11, B-11 | 6.1 (partial), 19.2 |
| D-12, D-13, B-12 | 11.4 (partial), 19.3 |
| D-14 | 10.8 (partial), 19.4 |
| D-18 envelope grammar | 3.1 (partial), 16.2 (partial), 19.7 |
| D-23 / D-25 principal id | 10.4 (partial), 19.8 |
| D-32, B-14 | 19.6 |
| B-01 | 1.3, 19.13, live parts of 20.3, 21.2, 22 |
| B-02 | 1.3, 19.2 to 19.4, 19.7, 19.8, 19.11, 19.12, live parts of 20.3, 21.2, 22 |
| B-03 | 19.12 |
| B-09, B-10 | 10.1 (partial), 19.11 |
| Owner_Authorization | 19.14, live parts of 20.3 and 21.2, 22 |

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "1.2", "1.4", "2.1"] },
    { "id": 1, "tasks": ["1.3", "1.5", "2.2"] },
    { "id": 2, "tasks": ["3.1", "5.1", "7.1", "7.3"] },
    { "id": 3, "tasks": ["3.2", "3.3", "4.1", "5.2", "7.2", "7.4", "8.1"] },
    { "id": 4, "tasks": ["3.4", "3.5", "4.2", "4.3", "4.4", "8.2", "8.3"] },
    { "id": 5, "tasks": ["3.6", "3.7", "6.1", "10.1", "11.1"] },
    { "id": 6, "tasks": ["3.8", "6.2", "10.2", "10.3", "11.2", "11.3"] },
    { "id": 7, "tasks": ["6.3", "6.4", "6.5", "6.6", "6.7", "10.4", "12.1"] },
    { "id": 8, "tasks": ["10.5", "10.6", "12.2", "12.3"] },
    { "id": 9, "tasks": ["10.7", "10.8", "11.4"] },
    { "id": 10, "tasks": ["10.9", "10.10", "10.11", "10.12", "10.13", "10.14", "10.15", "10.16", "14.1"] },
    { "id": 11, "tasks": ["14.2", "14.3", "15.1"] },
    { "id": 12, "tasks": ["14.4", "14.5", "14.6", "15.2"] },
    { "id": 13, "tasks": ["14.7", "14.8", "14.9", "14.10", "14.11", "14.12", "14.13", "14.14", "14.15", "14.16", "14.17", "14.18", "14.19", "14.20", "15.3", "15.4", "15.5", "15.6", "16.1", "16.2", "16.4", "16.6"] },
    { "id": 14, "tasks": ["16.3", "16.5", "16.7", "18.1"] },
    { "id": 15, "tasks": ["19.1", "19.2", "19.3", "19.4", "19.5", "19.6", "19.7", "19.8", "20.1"] },
    { "id": 16, "tasks": ["19.9", "19.11", "19.12", "19.13", "20.2"] },
    { "id": 17, "tasks": ["19.10", "20.3", "21.1"] },
    { "id": 18, "tasks": ["21.2"] },
    { "id": 19, "tasks": ["19.14", "21.3"] },
    { "id": 20, "tasks": ["22.1"] },
    { "id": 21, "tasks": ["22.2", "22.3"] }
  ]
}
```
