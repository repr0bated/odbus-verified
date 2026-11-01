# Requirements Document

## Introduction

The 3tched UI (SvelteKit, @ai-sdk/svelte) chat has no working reply path. The existing `chat_response.submit_turn` method waits for a responder agent that does not exist, causing every message to time out with "unknown outcome". The demo path uses `chat_router.Chat`, which records a mutation per message.

This spec defines requirements for a new HTTP route on the existing 8090 door that accepts Vercel AI SDK chat requests and translates them into one admitted `threetched.agent` plugin call. The bridge handles the protocol translation, identity verification, and streaming response.

**Scope**: Bridge route, SSE stream encoder, UI transport adapter. No new services, ports, or control-plane mechanisms.

## Glossary

- **AI SDK**: Vercel AI SDK (@ai-sdk/svelte), the chat framework used by the UI.
- **UIMessage**: The SDK's message type with id, role, parts array.
- **SSE**: Server-Sent Events, text/event-stream protocol.
- **OIA1**: Oracle Identity Assertion, x-oracle-identity-assertion-bin header.
- **SID1**: Sealed Identity, x-opdbus-sealed-id-bin header.
- **threetched.agent**: The plugin method that executes agent turns.
- **UI-message stream**: AI SDK's streaming protocol with start/text-start/text-delta/text-end/finish chunks.

## Requirements

### Requirement 1: HTTP Chat Route

**User Story:** As a chat UI, I want to POST chat requests to the 8090 door, so that I can use the Vercel AI SDK's standard transport.

#### Acceptance Criteria

1. **WHEN** a POST request arrives at the new route **THEN** the system SHALL accept a JSON body with fields: `id` (string), `messages` (UIMessage[]), `provider` (string), `model` (string).
2. **WHEN** the route is registered **THEN** the system SHALL add it beside the existing `/mqtt` route in `crates/op-grpc-bridge/src/server.rs`.
3. **WHEN** the route is registered **THEN** the system SHALL NOT create a new port or a new service.
4. **IF** the request body is malformed **THEN** the system SHALL return HTTP 400 with a user-safe error message.

### Requirement 2: Identity Verification

**User Story:** As a security gatekeeper, I want to verify identity from existing door headers, so that only authenticated requests are dispatched.

#### Acceptance Criteria

1. **WHEN** a request arrives **THEN** the system SHALL extract identity from exactly one of: OIA1 header (x-oracle-identity-assertion-bin) or SID1 header (x-opdbus-sealed-id-bin).
2. **IF** no credential is present **THEN** the system SHALL return HTTP 401 with zero dispatch attempts.
3. **IF** the credential is invalid or expired **THEN** the system SHALL return HTTP 403 with zero dispatch attempts.
4. **WHEN** identity is verified **THEN** the system SHALL use the existing interceptor path in `crates/op-grpc-bridge/src/interceptor.rs`.
5. **WHILE** processing a request **THEN** the system SHALL NEVER mint a new identity.

### Requirement 3: Single Dispatch

**User Story:** As a bridge operator, I want exactly one plugin call per message, so that mutations and receipts are properly accounted.

#### Acceptance Criteria

1. **WHEN** a valid request with a user message arrives **THEN** the system SHALL call `MutationEngine::dispatch_method_call_with_identity("threetched", "agent", args, Some(cap), principal, session, genesis)` exactly once.
2. **WHEN** dispatching **THEN** the system SHALL use capability `cap.software.threetched.call.cd4f0bc5a29de06b510f9aa428f1eedba926012b591fef7a518e776a7c9bd1824@v1`.
3. **WHEN** constructing arguments **THEN** the system SHALL read the schema from the sealed blob at `/dev/shm/opdbus/plugin-blobs/threetched.*.blob`.
4. **WHEN** constructing arguments **THEN** the system SHALL include fields: `--message` (latest user text), `--model-provider` (from request), `--model` (from request), `--session-state-file` (identity-scoped path).
5. **IF** the dispatch fails **THEN** the system SHALL return an error SSE chunk with a user-safe message.

### Requirement 4: Streaming Response

**User Story:** As a chat UI, I want to receive streaming text deltas, so that the user sees the reply appear progressively.

#### Acceptance Criteria

1. **WHEN** the dispatch begins streaming **THEN** the system SHALL respond with Content-Type `text/event-stream`.
2. **WHEN** the response starts **THEN** the system SHALL send header `x-vercel-ai-ui-message-stream: v1`.
3. **WHEN** streaming content **THEN** the system SHALL send SSE chunks in order: `start`, `text-start` (with id), `text-delta` (repeated), `text-end`, `finish`, `[DONE]`.
4. **IF** an error occurs **THEN** the system SHALL send an `error` SSE chunk with a user-safe message instead of crashing the connection.
5. **WHEN** the stream completes **THEN** the system SHALL send the `[DONE]` chunk and close the connection.

### Requirement 5: Cancellation

**User Story:** As a user, I want to cancel a message, so that I can stop long-running or unwanted replies.

#### Acceptance Criteria

1. **IF** the client disconnects mid-stream **THEN** the system SHALL cancel the dispatch.
2. **WHEN** canceling **THEN** the system SHALL follow the pattern in `crates/op-grpc-bridge/src/chat_service.rs` send() method.
3. **WHEN** cancellation completes **THEN** the system SHALL log the cancellation without erroring.

### Requirement 6: One Mutation Per Message

**User Story:** As an auditor, I want one mutation per message, so that the system has a clear operation record.

#### Acceptance Criteria

1. **WHEN** a dispatch completes **THEN** the system SHALL record exactly one mutation from the admitted call's own commit or receipt.
2. **WHILE** on this route **THEN** the system SHALL NEVER call `chat_response.submit_turn`.
3. **WHILE** on this route **THEN** the system SHALL NEVER call `respond_to_user`.

### Requirement 7: UI Transport Adapter

**User Story:** As a UI developer, I want the chat to use the new transport, so that replies actually work.

#### Acceptance Criteria

1. **WHEN** the UI initializes chat **THEN** `src/svelte/session-chat-transport.ts` SHALL use `DefaultChatTransport` pointed at the new route.
2. **WHEN** sending requests **THEN** the transport SHALL include the door identity headers.
3. **WHEN** sending requests **THEN** the transport SHALL include `{ provider, model }` from the confirmed route.
4. **FOR** the demo **THEN** the provider SHALL be `openrouter` and the model SHALL be `meta/muse-spark-1.3` (from RuntimeHost.svelte DEMO_ROUTE).
5. **IF** direct TLS to 8090 fails from the 8080 page **THEN** the UI SHALL use op-web's same-origin connector (see `sameOriginDoorUrl` in `src/grpc/door.ts`).
6. **WHEN** the adapter is complete **THEN** the demo `chat_router.Chat` call SHALL be removed from `src/chat/door-chat-adapter.ts` submit() method (commit 5a935be reference).

### Requirement 8: Owner-Approved Grant Change

**User Story:** As an operator, I want the grant change documented for approval, so that security is not bypassed.

#### Acceptance Criteria

1. **WHERE** the implementing identity is `c9bdf32e-811d-2495-b688-277c9e5c4d36` **THEN** the spec SHALL document these steps for owner approval:
   - a. Back up `/etc/opdbus/capability-grants.json`.
   - b. Use `jq` to add capability `cap.software.threetched.call.cd4f0bc5a29de06b510f9aa428f1eedba926012b591fef7a518e776a7c9bd1824@v1` to that identity's `.capabilities` array.
   - c. Run `sv restart opdbus-grants`.
   - d. Compare the file against `/dev/shm/opdbus/capability-grants.json` with `cmp`.
2. **WHILE** implementing **THEN** the system SHALL NOT apply the grant change automatically.

### Requirement 9: Non-Negotiable Constraints

**User Story:** As an architect, I want the constraints enforced, so that the system remains coherent.

#### Acceptance Criteria

1. **WHILE** translating the protocol **THEN** the system SHALL NEVER relay raw bytes to threetched's gateway at 127.0.0.1:8084.
2. **WHILE** dispatching **THEN** the system SHALL use D-Bus as the only control plane (no subprocess, no direct file reads for live state).
3. **WHILE** handling schema **THEN** the system SHALL read only from the sealed blob (no new schema files).
4. **WHILE** implementing **THEN** the system SHALL use Rust only (no new Python).
5. **WHILE** building **THEN** the system SHALL run only one Cargo process at a time.
6. **WHILE** implementing **THEN** the system SHALL NOT run tests or clippy unless the owner explicitly requests them.
7. **WHILE** deploying **THEN** the system SHALL use only `./activate-golden.sh` with `OP_PERSISTED_CATALOG_SOURCE=/var/tmp/release-catalog-20261101-b` (or a fresh `opblob seal-plugins` catalog if schema changes).

## Correctness Properties

### Property 1: Authentication Gate

**INVARIANT**: Every dispatch is preceded by successful identity verification.

**PROPERTY**: For all requests, dispatch_count = 1 implies auth_success = true.

### Property 2: Single Dispatch

**INVARIANT**: At most one dispatch per HTTP request.

**PROPERTY**: For all requests, dispatch_count <= 1.

### Property 3: Mutation Accounting

**INVARIANT**: Each successful dispatch produces exactly one mutation.

**PROPERTY**: For all requests, dispatch_success = true implies mutation_count = 1.

### Property 4: Error Isolation

**INVARIANT**: Errors do not leak internal details.

**PROPERTY**: For all error responses, error_message is user-safe (no stack traces, no internal paths, no credential values).

### Property 5: Cancellation Safety

**INVARIANT**: Client disconnect does not leave orphan dispatches.

**PROPERTY**: For all streams, client_disconnect = true implies dispatch_cancelled = true within timeout.
