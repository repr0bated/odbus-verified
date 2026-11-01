# Design Document

## Overview

This design adds an HTTP route to the existing op-grpc-bridge 8090 door that accepts Vercel AI SDK chat requests and translates them into one admitted `threetched.agent` plugin call. The route handles identity verification, argument construction from the sealed schema, SSE streaming, and cancellation.

## Architecture

### Component View

```
┌─────────────────────────────────────────────────────────────────┐
│                         8090 TLS Door                           │
│  (existing tonic server + new Axum route in server.rs)          │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────────┐  │
│  │   /mqtt      │    │  /chat/ai    │    │  gRPC routes     │  │
│  │  (existing)  │    │  (NEW)       │    │  (existing)      │  │
│  └──────────────┘    └──────┬───────┘    └──────────────────┘  │
│                             │                                   │
│                             ▼                                   │
│                    ┌────────────────┐                          │
│                    │ Interceptor    │                          │
│                    │ (OIA1/SID1)    │                          │
│                    └───────┬────────┘                          │
│                            │                                   │
│                            ▼                                   │
│                    ┌────────────────┐                          │
│                    │ MutationEngine │                          │
│                    │ dispatch_      │                          │
│                    │ method_call_   │                          │
│                    │ with_identity  │                          │
│                    └───────┬────────┘                          │
│                            │                                   │
│                            ▼                                   │
│                    ┌────────────────┐                          │
│                    │ threetched     │                          │
│                    │ .agent         │                          │
│                    │ (via D-Bus)    │                          │
│                    └────────────────┘                          │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### Data Flow

```
1. UI POST /chat/ai
   └─ Headers: OIA1 or SID1
   └─ Body: { id, messages: UIMessage[], provider, model }

2. Route Handler (ai_chat_route)
   ├─ Extract identity from headers (interceptor.rs)
   ├─ Validate identity (401/403 if bad)
   ├─ Extract latest user message text
   └─ Build args from sealed schema

3. Dispatch
   └─ MutationEngine::dispatch_method_call_with_identity(
        "threetched", "agent", args, cap, principal, session, genesis
      )

4. Stream Response
   ├─ Set headers: text/event-stream, x-vercel-ai-ui-message-stream: v1
   ├─ SSE chunks: start → text-start → text-delta* → text-end → finish → [DONE]
   └─ On error: error chunk

5. Cancellation
   └─ On client disconnect: cancel dispatch (watch channel pattern from chat_service.rs)
```

## Design Details

### D1: Route Registration

**File**: `crates/op-grpc-bridge/src/server.rs`

Add a new Axum route beside `/mqtt`:

```rust
// In the router construction
Router::new()
    .route("/mqtt", get(mqtt_handler))
    .route("/chat/ai", post(ai_chat_route::handler))  // NEW
    // ... existing routes
```

The route:
- Path: `/chat/ai`
- Method: POST
- Content-Type: application/json (request), text/event-stream (response)

### D2: Identity Extraction

**File**: `crates/op-grpc-bridge/src/ai_chat_route.rs` (new)

Reuse the existing interceptor functions:

```rust
use crate::interceptor::{ASSERTION_METADATA_KEY, SEALED_ID_METADATA_KEY};

fn extract_identity(headers: &http::HeaderMap) -> Result<GhostbridgeIdentity, Status> {
    let oia1 = headers.get(ASSERTION_METADATA_KEY);
    let sid1 = headers.get(SEALED_ID_METADATA_KEY);
    
    match (oia1, sid1) {
        (Some(_), None) => validate_oia1(oia1),
        (None, Some(_)) => validate_sid1(sid1),
        (None, None) => Err(Status::unauthenticated("identity required")),
        (Some(_), Some(_)) => Err(Status::invalid_argument("exactly one identity header required")),
    }
}
```

### D3: Argument Construction

**File**: `crates/op-grpc-bridge/src/ai_chat_route.rs`

Read the argument schema from the sealed blob:

```rust
fn build_agent_args(
    messages: &[UIMessage],
    provider: &str,
    model: &str,
    session_id: &str,
) -> Result<serde_json::Value, Error> {
    // Schema fields: --message, --model-provider, --model, --session-state-file
    let latest_user_text = messages.iter()
        .rev()
        .find_map(|m| m.role == "user" then m.text());
    
    serde_json::json!({
        "--message": latest_user_text,
        "--model-provider": provider,
        "--model": model,
        "--session-state-file": format!("/run/opdbus/sessions/{}/agent-state.json", session_id),
    })
}
```

### D4: SSE Stream Encoder

**File**: `crates/op-grpc-bridge/src/ai_chat_stream.rs` (new)

```rust
use axum::response::sse::{Event, Sse};

const UI_MESSAGE_STREAM_VERSION: &str = "v1";

pub struct UiMessageStreamEncoder {
    text_id: String,
}

impl UiMessageStreamEncoder {
    pub fn start() -> Event { Event::default().data(r#"{"type":"start"}"#) }
    
    pub fn text_start(id: &str) -> Event {
        Event::default().data(&format!(r#"{{"type":"text-start","id":"{}"}}"#, id))
    }
    
    pub fn text_delta(id: &str, delta: &str) -> Event {
        Event::default().data(&format!(
            r#"{{"type":"text-delta","id":"{}","delta":"{}"}}"#,
            id, delta
        ))
    }
    
    pub fn text_end(id: &str) -> Event {
        Event::default().data(&format!(r#"{{"type":"text-end","id":"{}"}}"#, id))
    }
    
    pub fn finish() -> Event { Event::default().data(r#"{"type":"finish"}"#) }
    
    pub fn done() -> Event { Event::default().data("[DONE]") }
    
    pub fn error(message: &str) -> Event {
        Event::default().data(&format!(r#"{{"type":"error","message":"{}"}}"#, message))
    }
}
```

### D5: Cancellation

**File**: `crates/op-grpc-bridge/src/ai_chat_route.rs`

Follow the pattern from `chat_service.rs`:

```rust
use tokio::sync::watch;

async fn handle_chat(
    Extension(engine): Extension<Arc<MutationEngine>>,
    headers: http::HeaderMap,
    body: Json<ChatRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    let identity = extract_identity(&headers)?;
    
    // Cancellation channel (like chat_service.rs send())
    let (cancel_tx, cancel_rx) = watch::channel(false);
    
    // Spawn dispatch task
    let dispatch = tokio::spawn(async move {
        engine.dispatch_method_call_with_identity(
            "threetched", "agent", args, Some(cap), 
            principal, session, genesis
        )
    });
    
    // Race: dispatch completion vs client disconnect
    tokio::select! {
        result = dispatch => {
            // Stream the response
            stream_response(result)
        }
        _ = cancel_rx.changed() => {
            // Cancelled
            dispatch.abort();
            Err(StatusCode::NO_CONTENT)
        }
    }
}
```

### D6: Error Mapping

**File**: `crates/op-grpc-bridge/src/ai_chat_route.rs`

```rust
fn map_error(e: anyhow::Error) -> (StatusCode, String) {
    // User-safe messages only
    if e.chain().any(|c| c.to_string().contains("unauthenticated")) {
        (StatusCode::UNAUTHORIZED, "Authentication required".into())
    } else if e.chain().any(|c| c.to_string().contains("permission denied")) {
        (StatusCode::FORBIDDEN, "Permission denied".into())
    } else if e.chain().any(|c| c.to_string().contains("invalid")) {
        (StatusCode::BAD_REQUEST, "Invalid request".into())
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, "Internal error".into())
    }
}
```

### D7: UI Transport

**File**: `/srv/git/3tched.com/src/svelte/session-chat-transport.ts`

Replace the session-based transport with DefaultChatTransport:

```typescript
import { DefaultChatTransport } from "ai";

export function aiSdkChatTransport(
  url: string,
  identity: DoorIdentity,
  route: { provider: string; model: string }
): ChatTransport<UIMessage> {
  const headers = identity.kind === "oia1" 
    ? { [OIA1_HEADER]: identity.assertion }
    : { [SID1_HEADER]: identity.sealedId };
  
  return new DefaultChatTransport({
    url,
    headers: {
      ...headers,
      "Content-Type": "application/json",
    },
    body: {
      provider: route.provider,
      model: route.model,
    },
  });
}
```

**File**: `/srv/git/3tched.com/src/chat/door-chat-adapter.ts`

Remove the demo `chat_router.Chat` call from `submit()`:

```typescript
// DELETE this section (reference commit 5a935be):
// const result = await client.call(TCHED_ROUTER, "Chat", args, ROUTER_CHAT_CAPABILITY);
```

### D8: Build Notes

The bridge must be built in a private mount namespace that bind-mounts an empty directory over `/var/lib/tched-router` (the threetched capture needs only the path). Template: `/tmp/claude-1000/-srv-git-odbus-verified/f8e7bf68-1189-4b28-9e1a-074672aaaba6/scratchpad/bridge-release.sh`.

**Without schema change:**
```bash
cargo build --release -p op-grpc-bridge
```

**With schema change:**
```bash
# 1. Workspace build
CXXFLAGS="-include cstdint" cargo build --workspace --release

# 2. op-release must byte-match
cargo build --release -p op-deployment --bin op-release
# Verify: sha256sum target/release/op-release /usr/local/libexec/opdbus-release/op-release

# 3. Seal plugins
sudo target/release/opblob seal-plugins /var/tmp/release-catalog-20261101-b
# Refuse if any plugin would be removed
```

### D9: Deploy Procedure

```bash
# Set catalog source
export OP_PERSISTED_CATALOG_SOURCE=/var/tmp/release-catalog-20261101-b

# Run golden build and activation detached with receipt
nohup ./activate-golden.sh > /var/tmp/ai-sdk-door-activate.receipt.log 2>&1 &
echo $! > /var/tmp/ai-sdk-door-activate.receipt.pid

# Monitor
tail -f /var/tmp/ai-sdk-door-activate.receipt.log
```

## File Changes Summary

| File | Change |
|------|--------|
| `crates/op-grpc-bridge/src/server.rs` | Add `/chat/ai` route |
| `crates/op-grpc-bridge/src/ai_chat_route.rs` | NEW: Route handler |
| `crates/op-grpc-bridge/src/ai_chat_stream.rs` | NEW: SSE encoder |
| `crates/op-grpc-bridge/src/lib.rs` | Export new modules |
| `/srv/git/3tched.com/src/svelte/session-chat-transport.ts` | Replace with DefaultChatTransport |
| `/srv/git/3tched.com/src/chat/door-chat-adapter.ts` | Remove chat_router.Chat demo path |

## Security Considerations

1. **Identity**: Only existing OIA1/SID1 headers accepted. No minting.
2. **Schema**: Only sealed blob consulted. No file fallbacks.
3. **Errors**: User-safe messages only. No stack traces or internal paths.
4. **Cancellation**: Proper cleanup on client disconnect.
5. **D-Bus**: Only control plane. No subprocess spawning.

## Components and Interfaces

### AiChatRoute (new)

**Module**: `crates/op-grpc-bridge/src/ai_chat_route.rs`

**Responsibilities**:
- Accept POST requests at `/chat/ai`
- Extract and validate identity from headers
- Build threetched.agent arguments from sealed schema
- Dispatch to MutationEngine
- Stream SSE response

**Interface**:
```rust
pub async fn handler(
    Extension(engine): Extension<Arc<MutationEngine>>,
    headers: http::HeaderMap,
    Json(body): Json<ChatRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, Json<ErrorResponse})>
```

### AiChatStream (new)

**Module**: `crates/op-grpc-bridge/src/ai_chat_stream.rs`

**Responsibilities**:
- Encode SSE chunks in AI SDK UI-message stream format
- Handle start/text-start/text-delta/text-end/finish/done sequence
- Provide error chunk encoding

**Interface**:
```rust
pub struct UiMessageStreamEncoder;

impl UiMessageStreamEncoder {
    pub fn start() -> Event;
    pub fn text_start(id: &str) -> Event;
    pub fn text_delta(id: &str, delta: &str) -> Event;
    pub fn text_end(id: &str) -> Event;
    pub fn finish() -> Event;
    pub fn done() -> Event;
    pub fn error(message: &str) -> Event;
}
```

### SessionChatTransport (modified)

**File**: `/srv/git/3tched.com/src/svelte/session-chat-transport.ts`

**Change**: Replace session-based transport with DefaultChatTransport using the new route.

### DoorChatAdapter (modified)

**File**: `/srv/git/3tched.com/src/chat/door-chat-adapter.ts`

**Change**: Remove the demo `chat_router.Chat` call path.

## Data Models

### ChatRequest (incoming)

```typescript
interface ChatRequest {
  id: string;              // Conversation ID
  messages: UIMessage[];   // Message history
  provider: string;        // e.g., "openrouter"
  model: string;           // e.g., "meta/muse-spark-1.3"
}

interface UIMessage {
  id: string;
  role: "user" | "assistant" | "system";
  parts: UIMessagePart[];
}

type UIMessagePart = 
  | { type: "text"; text: string }
  | { type: "image"; image: string }
  // ... other part types
```

### AgentArgs (to threetched.agent)

```rust
struct AgentArgs {
    #[serde(rename = "--message")]
    message: String,
    #[serde(rename = "--model-provider")]
    model_provider: String,
    #[serde(rename = "--model")]
    model: String,
    #[serde(rename = "--session-state-file")]
    session_state_file: String,
}
```

### SSE Chunks (outgoing)

```typescript
type SSEChunk =
  | { type: "start" }
  | { type: "text-start"; id: string }
  | { type: "text-delta"; id: string; delta: string }
  | { type: "text-end"; id: string }
  | { type: "finish" }
  | { type: "error"; message: string }
  | "[DONE]"
```

## Error Handling

### Error Categories

1. **Authentication errors** (401): Missing or invalid identity headers
2. **Authorization errors** (403): Valid identity but missing capability
3. **Validation errors** (400): Malformed request body
4. **Dispatch errors** (500): threetched.agent call failed
5. **Stream errors**: Sent as SSE error chunks

### Error Response Format

```json
{
  "type": "error",
  "message": "User-safe error message without internal details"
}
```

### User-Safe Messages

| Error | User Message |
|-------|--------------|
| No identity header | "Authentication required" |
| Invalid/expired identity | "Authentication required" |
| Missing capability | "Permission denied" |
| Malformed request | "Invalid request format" |
| Dispatch failure | "Internal error" |

## Testing Strategy

Tests run only when owner explicitly requests. When requested:

### Unit Tests

1. **ai_chat_route tests**: Identity extraction, argument building, error mapping
2. **ai_chat_stream tests**: SSE chunk encoding, valid JSON output

### Integration Tests

1. **Authenticated request**: Valid OIA1/SID1 → 200 with stream
2. **Unauthenticated request**: No header → 401, zero dispatch
3. **Invalid identity**: Bad signature/expired → 403, zero dispatch
4. **Cancellation**: Client disconnect → dispatch cancelled

### Manual Verification

1. POST to `https://10.0.0.2:8090/chat/ai` with valid identity
2. Verify SSE chunks arrive in correct order
3. Check bridge log for exactly one threetched.agent dispatch
4. Verify `http://10.0.0.2:8080` shows reply in chat panel

## Correctness Properties

### Property 1: Authentication Gate
**Validates: Requirements 2.1, 2.2, 2.3**

**Property**: Every successful dispatch has verified identity.

**Verification**: Log analysis shows dispatch events only after successful identity extraction.

### Property 2: Single Dispatch
**Validates: Requirements 3.1, 6.2**

**Property**: At most one dispatch per HTTP request.

**Verification**: Code review confirms single dispatch call; cancellation aborts existing dispatch.

### Property 3: Mutation Accounting
**Validates: Requirements 6.1, 3.1**

**Property**: Each successful dispatch produces one mutation.

**Verification**: Bridge log shows one mutation per dispatch completion.

### Property 4: Error Isolation
**Validates: Requirements 4.4, 1.4**

**Property**: Error responses contain no internal details.

**Verification**: Code review confirms user-safe messages only; no stack traces in responses.

### Property 5: Cancellation Safety
**Validates: Requirements 5.1, 5.2**

**Property**: Client disconnect cancels dispatch.

**Verification**: Test with client disconnect during stream; verify dispatch is aborted.
