# Implementation Plan: AI SDK Door Adapter

## Overview

This implementation adds an HTTP route to the 8090 door that accepts Vercel AI SDK chat requests and translates them into one admitted `threetched.agent` plugin call.

## Tasks

- [ ] 1. Claim work in HANDOFF.md (Required) — Add a row to Active work table before any edits. File: `/srv/git/odbus-verified/HANDOFF.md`

- [ ] 2. Create SSE stream encoder module (Required) — Create `crates/op-grpc-bridge/src/ai_chat_stream.rs` with `UiMessageStreamEncoder` struct. Methods: start, text_start, text_delta, text_end, finish, done, error. Each returns `axum::response::sse::Event`.

- [ ] 3. Create AI chat route handler (Required) — Create `crates/op-grpc-bridge/src/ai_chat_route.rs`. Define `ChatRequest` struct. Implement identity extraction from OIA1/SID1 headers. Build args from sealed blob. Dispatch to `MutationEngine::dispatch_method_call_with_identity("threetched", "agent", args, cap, principal, session, genesis)`.

- [ ] 4. Register route in server (Required) — Edit `crates/op-grpc-bridge/src/server.rs`. Add module declarations. Add `.route("/chat/ai", post(ai_chat_route::handler))` beside `/mqtt`.

- [ ] 5. Update lib.rs exports (Required) — Edit `crates/op-grpc-bridge/src/lib.rs`. Add `pub mod ai_chat_route; pub mod ai_chat_stream;`

- [ ] 6. Build bridge (Required) — Run `cargo build --release -p op-grpc-bridge`. If schema changed, use workspace build and `opblob seal-plugins`.

- [ ] 7. Update UI transport (Required) — Edit `/srv/git/3tched.com/src/svelte/session-chat-transport.ts`. Replace session-based transport with `DefaultChatTransport` pointing at new route with identity headers.

- [ ] 8. Remove demo chat path (Required) — Edit `/srv/git/3tched.com/src/chat/door-chat-adapter.ts`. Remove `chat_router.Chat` call from submit() method (reference commit 5a935be).

- [ ] 9. Build UI (Required) — Run `cd /srv/git/3tched.com && LANDING=prod VITE_OPDBUS_DOOR=https://10.0.0.2:8090 npm run publish:op-web`

- [ ] 10. Document grant change (Required) — In `/srv/git/odbus-verified/HANDOFF.md`, document grant for identity `c9bdf32e-811d-2495-b688-277c9e5c4d36` with capability `cap.software.threetched.call.cd4f0bc5a29de06b510f9aa428f1eedba926012b591fef7a518e776a7c9bd1824@v1`. DO NOT APPLY.

- [ ] 11. Deploy (Required) — Run `OP_PERSISTED_CATALOG_SOURCE=/var/tmp/release-catalog-20261101-b nohup ./activate-golden.sh > /var/tmp/ai-sdk-door-activate.receipt.log 2>&1 &`

- [ ] 12. Verify acceptance criteria (Required) — Test: authenticated request streams deltas; unauthenticated gets 401; bridge log shows one dispatch; UI shows reply at http://10.0.0.2:8080

- [ ] 13. Update HANDOFF.md completion (Required) — Add handoff entry with commits, release ID, evidence, and limitations.

## Task Dependency Graph

```json
{
  "waves": [
    {
      "wave": 1,
      "tasks": [1],
      "description": "Claim work in handoff"
    },
    {
      "wave": 2,
      "tasks": [2, 3],
      "description": "Create new modules (can run in parallel)"
    },
    {
      "wave": 3,
      "tasks": [4, 5],
      "description": "Integrate into server"
    },
    {
      "wave": 4,
      "tasks": [6],
      "description": "Build bridge"
    },
    {
      "wave": 5,
      "tasks": [7, 8],
      "description": "Update UI (can run in parallel)"
    },
    {
      "wave": 6,
      "tasks": [9],
      "description": "Build UI"
    },
    {
      "wave": 7,
      "tasks": [10],
      "description": "Document grant change"
    },
    {
      "wave": 8,
      "tasks": [11],
      "description": "Deploy"
    },
    {
      "wave": 9,
      "tasks": [12],
      "description": "Verify"
    },
    {
      "wave": 10,
      "tasks": [13],
      "description": "Final handoff"
    }
  ]
}
```

## Notes

- **No tests/clippy unless owner explicitly requests**
- **One Cargo process at a time**
- **D-Bus only control plane — no subprocess, no raw file reads**
- **Sealed blob only schema — no new schema files**
- **Grant change requires owner approval — document, don't apply**

---

# Spark Hand-off Prompt

```
## Task: Implement AI SDK Door Adapter

You are implementing the ai-sdk-door-adapter spec for the 3tched chat system.

### Problem
Chat in the 3tched UI has no working reply path. The existing chat_response.submit_turn 
waits for a responder agent that doesn't exist, so every message times out with 
"unknown outcome".

### Solution
Add an HTTP route on the existing 8090 door that accepts Vercel AI SDK chat requests 
and translates them into one admitted `threetched.agent` plugin call. The bridge 
handles protocol translation, identity verification, and SSE streaming.

### Non-Negotiable Constraints
1. Translate the protocol; NEVER relay bytes to threetched's gateway at 127.0.0.1:8084
2. D-Bus is the ONLY control plane — no subprocess, no direct file reads for live state
3. Sealed blob is the ONLY schema — no new schema files
4. Rust ONLY — no new Python
5. One Cargo process at a time
6. Tests/clippy ONLY when owner explicitly requests
7. Deploy ONLY via `./activate-golden.sh` with OP_PERSISTED_CATALOG_SOURCE

### Spec Files
- Requirements: /srv/git/odbus-verified/.kiro/specs/ai-sdk-door-adapter/requirements.md
- Design: /srv/git/odbus-verified/.kiro/specs/ai-sdk-door-adapter/design.md
- Tasks: /srv/git/odbus-verified/.kiro/specs/ai-sdk-door-adapter/tasks.md

### Before You Start
1. Read HANDOFF.md at /srv/git/odbus-verified/HANDOFF.md
2. Add a row to the Active work table claiming this task
3. Do NOT reset, stash, or overwrite work another agent has claimed

### Key Files to Create/Modify
- crates/op-grpc-bridge/src/ai_chat_stream.rs (NEW)
- crates/op-grpc-bridge/src/ai_chat_route.rs (NEW)
- crates/op-grpc-bridge/src/server.rs (add route)
- crates/op-grpc-bridge/src/lib.rs (export modules)
- /srv/git/3tched.com/src/svelte/session-chat-transport.ts (update transport)
- /srv/git/3tched.com/src/chat/door-chat-adapter.ts (remove demo path)

### Grant Change (DOCUMENT ONLY, DO NOT APPLY)
Identity c9bdf32e-811d-2495-b688-277c9e5c4d36 needs capability:
cap.software.threetched.call.cd4f0bc5a29de06b510f9aa428f1eedba926012b591fef7a518e776a7c9bd1824@v1

Document the change for owner approval in HANDOFF.md.

### Build
Without schema change: cargo build --release -p op-grpc-bridge
With schema change: See design.md Task 5B

### Deploy
OP_PERSISTED_CATALOG_SOURCE=/var/tmp/release-catalog-20261101-b ./activate-golden.sh

### Acceptance Criteria
- Authenticated request streams text-delta chunks and ends with finish
- Unauthenticated request gets 401, zero dispatch
- Bridge log shows exactly one threetched.agent call per message
- http://10.0.0.2:8080 shows reply in chat panel
- demo chat_router.Chat path removed
- HANDOFF.md has entry with commits, release id, evidence
```
