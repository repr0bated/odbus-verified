# Requirements: Control-Plane Chatbot Reasoning Episode Vectorization

**Status:** Requirements / pre-design
**Scope:** Production
**Related:** `op-cognitive-mcp`, `op-snowball`, `op-plugins`, Qdrant, Voyage AI
**Open decision:** Voyage model selection (POC to validate voyage-4-lite vs voyage-4)

---

## Context

The control-plane chatbot is an autonomous agent that sits at the helm of the system. It receives
high-level goals, reasons about them, delegates to tools/plugins/MCP, oversees execution, and
makes control decisions. It is not a user-facing chat interface — it is the system's decision
engine.

The chatbot operates in two modes: **reasoning** (planning, evaluating, deciding) and
**executing/responding** (acting on a decision). This spec covers the reasoning mode only.

A **reasoning episode** is the span of time the chatbot spends in reasoning state — from entry
to exit. It is the primary unit of vectorization.

The primary requirement is: a human operator must be able to semantically search what the
chatbot reasoned about, what it decided, and why — with near-zero lag after the episode ends.
Secondary: the chatbot itself may use this index for recall (persistent memory / self-lookup).

---

## Requirements

### REQ-1: Reasoning State Definition

The system must define **reasoning** as a first-class state in the control-plane chatbot's
lifecycle.

- Reasoning state is entered when the chatbot begins planning, evaluating options, or forming
  a decision in response to any trigger (goal received, tool result returned, interrupt, etc.).
- Reasoning state is exited when any of the following occurs:
  - A tool or plugin call is dispatched
  - A response is emitted (to user, to another agent, or to the system)
  - A direction change is committed (replanning)
  - A scheduling or configuration decision is finalized
  - An external interrupt is received and acknowledged
- Nesting of reasoning episodes (reasoning within reasoning) is controlled by a system-wide
  policy flag. Default: **flat** (no nesting). When flat, a new reasoning trigger while already
  in reasoning state extends the current episode rather than opening a new one.

### REQ-2: Reasoning Episode Record

Each reasoning episode must produce a structured record at close. The record must capture:

| Field | Description |
|---|---|
| `episode_id` | Unique ID (UUID v7 recommended for time-ordering) |
| `started_at` | Timestamp of reasoning entry |
| `ended_at` | Timestamp of reasoning exit |
| `duration_ms` | Wall-clock duration |
| `trigger` | What caused reasoning to start (goal, tool_result, interrupt, replan, system_event) |
| `exit_reason` | What ended reasoning (tool_call, response_emitted, direction_change, goal_achieved, config_set, task_scheduled, interrupt) |
| `goal_text` | The high-level goal or prompt active at episode start |
| `reasoning_summary` | Compact natural-language summary of what was reasoned about and what was decided — this is the primary embedding input |
| `tools_consulted` | Ordered list of tools/plugins/MCP calls made during the episode (names only, no payloads) |
| `decision_output` | The decision, plan, or action the episode produced |
| `outcome_class` | Enum: `goal_achieved` / `config_set` / `task_scheduled` / `delegated` / `interrupted` / `direction_changed` / `inconclusive` |
| `confidence` | Optional float 0.0–1.0 if the model emits one |
| `plugin_id` | Plugin that owns the context being reasoned about, if applicable |
| `conversation_id` | Groups episodes belonging to the same high-level task chain |
| `content_hash` | SHA-256 of canonical serialized record — used for exact dedup before upsert |
| `pii_flagged` | Bool — if true, `reasoning_summary` and `decision_output` are redacted before vectorization |

### REQ-3: Plugin Schema

A new plugin `ctl-plane-chatbot` must be registered in the plugin schema registry with:

- The fields defined in REQ-2 as its schema contract
- PII tagging support at the field level (at minimum `goal_text`, `reasoning_summary`,
  `decision_output` must be individually taggable)
- Significance classification: reasoning episodes are always at least `Contextual`; episodes
  with `outcome_class` of `goal_achieved`, `config_set`, or `task_scheduled` are `Signal`

### REQ-4: Vectorization Pipeline

**Trigger:** Episode record is finalized at reasoning exit.

**Order of operations (must be strictly preserved):**

1. Episode record written to persistent store (snowball / event log)
2. Embedding enqueued immediately (non-blocking — must not delay reasoning exit)
3. Voyage API called asynchronously
4. Vector upserted to Qdrant on response

The control-plane chatbot must not be blocked on embedding completion. If the Voyage API is
unavailable, the episode record is retained and embedding is retried from the queue.

**Embedding input text** is constructed from `reasoning_summary` + `goal_text` +
`outcome_class` + `tools_consulted` (joined as structured plain text). Raw payloads are never
included. If `pii_flagged` is true, only `outcome_class` and `tools_consulted` are used.

**Voyage input_type:** `"document"` for episode upsert, `"query"` for retrieval.

**Model:** voyage-4-lite (POC validation target). voyage-4 as fallback if retrieval quality
is insufficient. voyage-code-3 is explicitly out of scope for this index — reasoning episodes
are mixed prose/config/system text, not code.

### REQ-5: Qdrant Collection

- Separate collection from mutation footprints and schema footprints
- Collection name: `ctl_plane_reasoning_episodes`
- Vector dimensions: 1024 (voyage-4-lite default; flexible dims may be revisited post-POC)
- Payload stored alongside vector: `episode_id`, `started_at`, `ended_at`, `outcome_class`,
  `trigger`, `exit_reason`, `plugin_id`, `conversation_id`, `content_hash`
- Full `reasoning_summary` and `decision_output` stored in payload for human review display
- Filterable fields: `outcome_class`, `plugin_id`, `conversation_id`, `started_at`

### REQ-6: Human Search Interface

- A human operator must be able to issue a natural-language query against
  `ctl_plane_reasoning_episodes` and receive ranked results with:
  - `reasoning_summary`
  - `decision_output`
  - `outcome_class`
  - `started_at` / `duration_ms`
  - `tools_consulted`
- Results must reflect episodes that completed within the last few seconds (near-real-time)
- Filtering by `outcome_class`, `plugin_id`, time range must be supported
- Interface surface (UI vs API vs CLI) is out of scope for this spec — this is a data/pipeline
  requirement only

### REQ-7: Deduplication

- Before any Qdrant upsert, check `content_hash` against recent upserts
- Exact duplicate episodes (same hash) must not produce duplicate vectors
- Hash collision window: configurable, default 24 hours

### REQ-8: Privacy

- `pii_flagged` episodes: embed only `outcome_class` + `tools_consulted` (no summary text)
- `pii_flagged` episodes: full record still written to event log (audit trail preserved)
- `pii_flagged` episodes: `reasoning_summary` and `decision_output` omitted from Qdrant payload
- PII tagging follows the same rules as `activity_filter.rs` — schema-level or field-level tags

### REQ-9: Worker Priority

- Reasoning episode embedding worker: **high priority** (human is waiting to search)
- Lower than real-time control-plane operations, higher than mutation footprint embedding
- NUMA affinity: same node as Qdrant client if local deployment

### REQ-10: Observability

- Structured tracing span per episode: `reasoning_episode.vectorized`
- Span attributes: `episode_id`, `duration_ms`, `outcome_class`, `voyage_latency_ms`,
  `qdrant_upsert_latency_ms`, `pii_flagged`
- Alert if embedding queue depth exceeds configurable threshold (default: 50 pending episodes)

---

## Out of Scope (this spec)

- Chat turn vectorization (separate spec)
- Mutation footprint vectorization (separate spec)
- Schema footprint vectorization (separate spec)
- Voyage model selection validation (POC task, not a spec requirement)
- Search UI implementation
- Chatbot self-lookup / RAG over this index (persistent memory spec)

---

## Open Questions

| # | Question | Owner |
|---|---|---|
| OQ-1 | Nesting policy default — confirm flat is correct for v1 | Architecture |
| OQ-2 | `reasoning_summary` — generated by the model itself at episode close, or derived from the episode record by a separate summarizer? | Architecture |
| OQ-3 | Voyage model final selection — voyage-4-lite vs voyage-4 | POC validation |
| OQ-4 | Qdrant instance — shared with other collections or dedicated for control-plane? | Infrastructure |
| OQ-5 | Retry policy for failed Voyage calls — max retries, backoff, dead-letter behavior | Engineering |
