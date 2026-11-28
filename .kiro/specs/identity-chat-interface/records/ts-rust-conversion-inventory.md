# TS -> Rust conversion inventory (identity-chat-interface)

Recorded 2026-11-19 (America/New_York) against `odbus-verified` origin/dev
`0b43e43` and `3tched.com` dev `cc6ed52` (read-only, `git show`/`git archive`
only; nothing in 3tched.com was changed). Spec: this directory
(`requirements.md`, `design.md`, `tasks.md`). The spec's task text names
TypeScript paths under `apps/identity-chat/`; per the owner mapping in
`3tched.com:crates/op-chat-interface/src/lib.rs:3-5` everything server-side is
Rust and only the browser UI stays TypeScript/Svelte.

Where the code lives:

- **Rust Chat_Route**: `3tched.com:crates/op-chat-interface` (moved out of
  odbus-verified by `f0d72e0` "refactor!: move op-chat-interface to the
  container repo"; landed in 3tched.com as `639c9ef`). Standalone crate, not a
  workspace member (`Cargo.toml` `[workspace]`).
- **Browser UI**: `3tched.com:src/svelte`, `src/chat` (TypeScript, Svelte 5,
  `@ai-sdk/svelte` ^5.0.159, `ai` ^7.0.159, `chat` ^4.43.0,
  `@json-render/svelte` ^0.20.0 in `package.json`).
- **Host side (bridge)**: `odbus-verified:crates/op-grpc-bridge` (ChatLink,
  execution tracker wiring, model-selection receivers). Owned by Muse/Wonder.
- **Sealed contracts**: `odbus-verified:crates/op-plugins/src/state_plugins/chat_router*.rs`,
  `model_registry.rs`; tracker types in `crates/op-execution-tracker`.

Status key: **done** (code present, matches spec intent), **partial**,
**missing**, **n/a** (retired or stays TS by design). Test counts are
`#[test]`/`#[tokio::test]` attributes in the file. Spec checkboxes: every
sub-task listed below is `[~]` in `tasks.md` (none `[x]`); top-level tasks are
`[ ]`. The Status table (`tasks.md:1042-1050`) says checkboxes were never
reconciled with code.

## AI SDK / Chat SDK pieces

| Piece (SDK concept) | Rust implementation | Status | Evidence | Tests | Spec task |
|---|---|---|---|---|---|
| UI message stream protocol (UIMessageChunk) | `op-chat-interface/src/bin/chat-route.rs` `enum AiPart` {Text, Reasoning, ToolCall, ToolResult} | **partial** | `chat-route.rs:89-160`. Emits whole `text`/`reasoning`/`tool-call`(`args`)/`tool-result` parts. AI SDK 5+ chunks the UI (`ai` ^7) consumes are `start`, `text-start`/`text-delta`/`text-end`, `tool-input-available`(`input`), `tool-output-available`(`output`), `finish`; no deltas, no start/finish, no `data-*` parts (`data-tool-status`, `data-chain`, design `design.md:983-1013`) | 7 in chat-route.rs (in-process) | 14.3, 32.5 |
| Same protocol, host side | `op-grpc-bridge/src/chat_link.rs` ChatLink `Exchange` stream of `ChatLinkChunk { json }` = one UIMessageChunk | **done (passthrough)** | `chat_link.rs:1-20`, `proto/operation.proto:85-99`; served on the existing TLS door `server.rs:76-83`; spawns one Node worker per principal from `IDENTITY_CHAT_WORKERS_FILE` (TS worker, not the Rust chat-route) | 15 in chat_link.rs | 19.10 |
| UIMessage / parts intake | `chat-route.rs:176-200` `ui_message_text`; `proto/chat.proto:43` `ui_messages` bytes | **partial** | concatenates `parts[].text` only; tool parts stripped by `guard/intake.rs` `check_request` | covered by chat-route tests | 3.7, 14.4 |
| Chat_Link wire (gRPC/gRPC-Web) | `op-chat-interface/proto/chat.proto` `ChatService { Send, Approve, Cancel }`, tonic 0.12 + tonic-web | **partial / diverged** | chat.proto `service ChatService`; bridge has a different ChatLink contract (`operation.proto` `ChatLink.Exchange`) and the legacy `ChatService` is intercepted (`design.md:3214-3216`). Two Chat_Link contracts exist; only one should remain | none for wire | 19.10 (D-03, D-45) |
| `useChat` / `Chat` state + `ChatTransport` | stays TS: `3tched.com:src/svelte/Chat.svelte:3,27` (`@ai-sdk/svelte` `Chat`), `src/svelte/session-chat-transport.ts:1-4` | **partial (TS)** | transport is the session transport, not the spec's gRPC-Web `ChatTransport` (D-03) | `Chat.test.ts`, `chat-session.test.ts` | 15.2, 19.10 |
| Chat SDK (`chat` + `@chat-adapter/web`, thread ids, state adapter) | none in Rust | **missing** | design `design.md:578-602` is TS-only; Rust has `state/partitioned_store.rs` (principal-keyed) but no thread id `web:{principal}:{conversation}` | 0 | 14.5 (B-15) |
| Tool calling: confirmation gate (needsApproval) | `op-chat-interface/src/gate/confirmation_gate.rs` | **done** | D-17 digest, one-time consume (`design.md:3181-3185`) | 0 in file (exercised via chat-route tests) | 8.1 |
| Tool-call status machine | `src/status/tool_call_status.rs` | **done** | 93 lines | 0 | 7.1 |
| Tool execution (one Operation_File call per tool call) | `chat-route.rs:540-640` | **missing (fails closed)** | always returns typed `missing-mount` (`chat-route.rs:547,636-637`); no Rust operation-file runner, no read-back | 0 | 10.1, 10.6, 10.7, 10.8 |
| Schema validation of tool args | `src/validation.rs` (jsonschema 0.29) | **done** | 56 lines | 0 | 5.1 |
| Identity binding (`get_own_identity`) | `chat-route.rs:321` `resolve_principal` | **partial** | profile-file fallback / synthetic principal; no FUSE `get_own_identity` read | 1 (`chat-route.rs:1064`) | 10.4 |
| Secret_Guard / stream guard / intake | `src/guard/{secret_guard,stream_guard,credential_registry,intake,records}.rs` | **done (interim D-18 grammar)** | base copy also in bridge `cockpit/secret_guard` (`f0d72e0`) | 0 | 3.1-3.7 |
| Profile_Loader + Session_Mode | `src/profile/profile_loader.rs` | **done** | 270 lines | 0 | 4.1 |
| Partitioned store | `src/state/partitioned_store.rs` | **done** | 132 lines | 0 | 7.3 |
| Chain_Verifier | `src/chain/mod.rs` | **done (continuity only, D-30)** | 318 lines | 8 | 11.1 |
| Execution_Record read + receipt linking | `src/types.rs:190` `ExecutionRecordView` only | **missing** | no sealed read method exposes own execution records (see below) | 0 | 11.4, 19.3 (D-12, D-13) |
| Model_Context_Builder / system prompt | none | **missing** | | 0 | 12.1 |
| Providers (`LanguageModel`) / model registry | `chat-route.rs:165-260` `trait ModelProvider` + `SyntheticProvider`; `--provider http` refused | **missing (synthetic only)** | `chat-route.rs:41-48,873` | 2 | 19.9 (D-04, B-06/07) |
| Provider/model *selection* | host: `chat_router` SetSelection/SetModel/SetProvider/role setters, `apply_catalog_model_binding`, `config_set/patch`; UI: `src/svelte/ModelPicker.svelte`, `src/chat/model-catalog.ts`, `control-plane-model.ts`, `native-routing.ts` | **done on host; UI stale** | see `model-selection-receiver.md` | host: chat_router tests; UI: `ModelPicker.test.ts`, `model-catalog.test.ts`, `control-plane-model.test.ts` | owner exception, plan step 3 |
| Tool_Registry | removed | **n/a (retired 2026-11-09)** | `lib.rs:20-29` | - | 6 |
| json-render catalog / UI | TS (`src/svelte/catalog`, `SpecRenderer.svelte`) | **partial (TS)** | | TS tests | 15.1 |

## Execution tracker: what already happens

Every container FUSE operation is already an execution-tracker record, keyed
by the verified identity, before any chat code is involved:

- `op-grpc-bridge/src/runtime.rs:707-717` `Runtime::call` builds the context
  and `tracker.reserve(...)` before admission; `:741-771` moves it through
  `Checking -> ReadyForAdmission -> ReceiptCompleted -> Completed/DomainFailed/NeedsReview`
  with an `ExecutionProof` holding the ledger attempt and terminal event
  id/hash (`MethodExecutionReceipt`).
- `runtime_checkpoint.rs:102-117` `execution_context`: `tool_name =
  "<plugin>.<method>"`, `executor_principal_id`/`executor_session_id` from the
  verified caller, `trace_id` from `caller.trace_id`.
- `runtime_checkpoint.rs:201-230` persists each transition in BridgeCore
  (`record_execution_control`) and `apply_committed`s it to the tracker.
- `op-execution-tracker/src/execution_tracker.rs:69,86,109,154`
  (`reserve`, `finish`, `apply_committed`, `recent_completed`; the old
  `list_recent_completed` name is gone).

So a chat tool call executed as one Operation_File call (spec 10.6) and a
generation started through a sealed method (`chat_router.start_generation`,
`threetched.agent`) are tracked with no chat-side code. Three gaps remain, all
in bridge or 3tchedFS code:

1. **No turn correlation.** `trace_id` comes only from the
   `X-Ghostbridge-Trace-ID` header/metadata (`op-grpc-bridge/src/tracing.rs:39-72`).
   An Operation_File write has no field for it, so tool calls of one chat turn
   cannot share the turn's trace id.
2. **No own-record read.** `Runtime::execution_checkpoint_page`
   (`runtime_checkpoint.rs:235`) has no caller and no sealed method, so the
   Chat_Route cannot read its own Execution_Records (D-12 stays open, task 19.3).
3. **The chat turn itself** is only tracked if its generation is a sealed
   method call. The Rust chat-route has no such call yet (synthetic provider).

## Duplicate stacks to resolve (owner decision)

- Rust `chat-route` (`3tched.com`) serves its own tonic/tonic-web
  `ChatService`; the bridge ChatLink (`chat_link.rs`) spawns a **Node** worker
  per principal. Only one can be the Chat_Route. The spec says server-side is
  Rust, which means ChatLink's worker should exec the Rust `chat-route` binary
  (or ChatLink is replaced), a bridge change.

## Test run (2026-11-19)

`cargo test --locked` on antix1 in a throwaway copy
(`git archive` of 3tched.com `cc6ed52` `crates/op-chat-interface` into
`/tmp/occi`, `CARGO_TARGET_DIR=/tmp/occi/target`; 3tched.com itself not
touched): **15 passed, 0 failed** (lib: 8 `chain::tests`; bin `chat-route`:
7). Doc tests 0. Every other module (guard, gate, profile, state, status,
validation) has no tests of its own, so the spec's property tests (Properties
1-47, `tasks.md` Notes) are not ported to Rust.
