# Plan: Stable Chatbot + Local Repo Indexing

## Context

operation-dbus needs a stable chatbot backed by OpenClaw (a full agent platform at `127.0.0.1:18789` with OpenAI-compatible API, model failover, stateful sessions, and MCP integration). The op-web `UnifiedOrchestrator` is already a working multi-turn chatbot with tool calling — it just needs a reliable LLM backend wired in.

Separately, 59 git repos (~180k files) at `/home/jeremy/git/` have GEMINI.md context files but no searchable index. Google Code Assist indexing is blocked by auth issues. A local FTS5-based code index will enable the chatbot to retrieve relevant code context.

The dead duplicate `LlmProvider` trait in `op-chat/src/llm.rs` has already been deleted.

---

## Priority 1: OpenClaw Provider (~2-4 hours)

### Step 1: Create `crates/op-llm/src/openclaw.rs`

New ~200-line module implementing `LlmProvider` trait. Pattern follows `anthropic.rs`.

```
OpenClawProvider {
    client: reqwest::Client,
    base_url: String,              // OPENCLAW_BASE_URL, default http://127.0.0.1:18789
    session_key: Option<String>,   // OPENCLAW_SESSION_KEY (stateful sessions)
    agent_id: Option<String>,      // OPENCLAW_AGENT_ID (agent routing)
}
```

- `from_env()` constructor reads env vars
- `chat_with_request()` — POST to `{base_url}/v1/chat/completions` with:
  - trusted internal-network access to the gateway
  - `x-openclaw-session-key` and `x-openclaw-agent-id` headers (if set)
  - `user` field for session routing
  - Tools via `ToolDefinition::to_openai_format()` (already exists at `provider.rs:144`)
  - Tool choice via `ToolChoice::to_api_format()` (already exists at `provider.rs:176`)
- Parse OpenAI-format response: `choices[0].message.tool_calls` → `Vec<ToolCallInfo>`
  - Note: OpenAI returns `arguments` as a JSON string, needs `simd_json::from_str()`
- `list_models()` — hit `{base_url}/v1/models` with auth header
- `list_models()` — hit `{base_url}/v1/models` over the internal bridge
- `provider_type()` — return `ProviderType::Custom("openclaw".into())`
- No new Cargo.toml deps needed (reqwest, simd-json, async-trait already present)

### Step 2: Wire into ChatManager

**`crates/op-llm/src/chat.rs`** — Add OpenClaw auto-detection block in `ChatManager::new()`, **before** the MCP Proxy block (highest priority when configured):

```rust
if std::env::var("OPENCLAW_BASE_URL").is_ok() || matches!(env_provider.as_deref(), Some("openclaw")) {
    match OpenClawProvider::from_env() { ... }
}
```

**`crates/op-llm/src/provider.rs`** — Add `"openclaw"` to `FromStr` impl for `ProviderType` so `LLM_PROVIDER=openclaw` works.

**`crates/op-llm/src/lib.rs`** — Add `pub mod openclaw;` and re-export.

### Step 3: Verify

- Set `OPENCLAW_BASE_URL` + `LLM_PROVIDER=openclaw`
- Start op-web, send chat messages through the Assistant UI
- Verify tool calling works (ask chatbot to list bridges, etc.)
- Verify multi-turn conversation works

### Files modified:
- `crates/op-llm/src/openclaw.rs` — **NEW**
- `crates/op-llm/src/provider.rs` — add `"openclaw"` to `FromStr`
- `crates/op-llm/src/chat.rs` — add import + auto-detection block
- `crates/op-llm/src/lib.rs` — add module + re-export

---

## Priority 2: Local Repo Indexing (Phased)

### Design decisions:
- **FTS5 first, embeddings later** — `op-ml` has an embedding pipeline but the `ml` feature deps aren't actually in Cargo.toml. FTS5 with BM25 ranking is effective for code and ships with zero external deps.
- **New crate `op-code-index`** — separate from `op-introspection` (D-Bus specific) and `op-ml` (embeddings only)
- **Exposed as MCP tools** — the chatbot discovers and uses them automatically via the existing tool registry

### Phase 2a: Core Indexer

**New crate: `crates/op-code-index/`**

Deps: `rusqlite` (bundled + fts5), `ignore` (gitignore-aware walking), `walkdir`, `simd-json`, `anyhow`, `tracing`, `tokio`

```
src/
  lib.rs              — public API
  crawler.rs          — repo discovery, file walking (ignore crate for .gitignore)
  chunker.rs          — language-aware chunking (fn/struct/impl boundaries for Rust, fixed-size for others)
  indexer.rs          — SQLite FTS5 index (follows op-introspection/indexer.rs pattern)
  search.rs           — query interface with BM25 ranking
  gemini_context.rs   — GEMINI.md parser (Focus Area, Why Index, Key Concepts, etc.)
```

**SQLite schema** (modeled on `crates/op-introspection/src/indexer.rs`):
- `repos` table — name, path, language, focus_area, description, file/chunk counts
- `chunks` table — repo_name, file_path, language, chunk_type, name, content, line range
- `chunks_fts` FTS5 virtual table with porter stemming
- Triggers for FTS sync (same pattern as `DbusIndexer`)

**Chunk types**: Function, TypeDefinition, Module, Block (fixed-size fallback), GeminiContext

**DB location**: `/var/lib/op-dbus/code-index.db`

### Phase 2b: MCP Tool Exposure

**New file: `crates/op-tools/src/builtin/code_search.rs`**

Tools to register in `crates/op-tools/src/builtin/mod.rs`:
- `code_search` — main search (query, optional repo/language filter, limit)
- `code_index_status` — stats (repos, files, chunks, last index time)
- `code_index_rebuild` — trigger reindex (one repo or all)
- `list_indexed_repos` — list repos with GEMINI.md context

The chatbot discovers these automatically via the existing tool directory in `UnifiedOrchestrator::process()`.

### Phase 2c: Verify RAG flow

No orchestrator changes needed — the LLM already sees all registered tools and can call `code_search` when users ask about code. Verify by asking the chatbot questions about the indexed repos.

### Future phases (not in scope):
- **2d**: Incremental indexing (inotify/polling, file hash tracking)
- **2e**: Embedding-based search (activate `op-ml`, add vector similarity)

### Files modified:
- `Cargo.toml` (workspace) — add `op-code-index` member
- `crates/op-code-index/` — **NEW CRATE** (6 source files)
- `crates/op-tools/Cargo.toml` — add `op-code-index` dep
- `crates/op-tools/src/builtin/code_search.rs` — **NEW**
- `crates/op-tools/src/builtin/mod.rs` — register code_search tools

---

## Verification

1. **OpenClaw chatbot**: Set env vars, start op-web, send chat messages, confirm tool calling and multi-turn work
2. **Code index**: Run index build, verify FTS5 search returns relevant results for known function names
3. **End-to-end RAG**: Ask chatbot "how does the D-Bus indexer work?" and verify it calls `code_search` and returns relevant code from `op-introspection/indexer.rs`
