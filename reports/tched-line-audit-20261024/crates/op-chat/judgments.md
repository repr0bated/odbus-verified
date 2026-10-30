# op-chat declarative JEV receipts

Task `tched-line-audit-20261024/op-chat`; 2026-10-24; Codex crate worker.

Source was read line by line, including every file without an occurrence. No JEV relevance/routing/reranking operation was used. Eval executed only declarative judgments over already-read source evidence.

## Criteria

Four contrary choice labels were declared: `leave_as_is` (qualified correct name, retained contract, or ordinary English), `replace_with_threetched` (incorrect spelling for the external ZeroClaw-derived executable/router), `replace_with_3tched` (incorrect spelling for our infrastructure/filesystem projection), `insufficient_evidence` (referent not established). Preserve existing object names and paths absent an explicit migration. Source is not deployment evidence.

## Call 1 — protocol routing owner

Successful `await judge(...)`, JS eval title `Judging protocol naming context`.

Question: “Classify all four threetched occurrences in this exact protocol comment cluster by contextual intent.”

Source: `crates/op-chat/proto/chat.proto:1-9,41-47`. Submitted surrounding delegator comments and the complete SendRequest excerpt, including:

```proto
// threetched owns LLM provider/model routing (OD-28). SendRequest carries
// the provider/model selection from threetched's state; the server does not
// make routing decisions.
message SendRequest {
  string conversation_id = 1;   // Groups episodes into task chains (threaded)
  bytes  ui_messages = 2;       // JSON-encoded UIMessage[] (AI SDK shape)
  optional string persona_id = 3;
  string provider = 4;          // threetched-selected provider (OD-28)
  string model = 5;             // threetched-selected model (OD-28)
}
```

Actual answer: `leave_as_is`; confidence **0.95**. Probabilities: leave0.96 / threetched0.01 / 3tched0.02 / insufficient0.01.

Decision: preserve occurrences at lines7,8,45,46. The comments describe the external routing owner, not the filesystem.

## Failed batch attempt — exact tool failure

JS eval title `Judging remaining naming clusters` attempted the documented `judge_batch(...)` syntax. Actual failure:

```text
ReferenceError: judge_batch is not defined
at <anonymous> (js-cell-2d3888e0-ca2c-4913-885b-c7805d86dd87.js:2:30)
Command exited with code 1
```

No batch answers exist. Reported through `xd://report_issue`: eval docs expose judge_batch but runtime does not define it. `judge()` itself works. Recovery used one combined evidence state and eight independent typed questions in one `judge()` call, not a loop or invented endpoint.

## Call 2 — eight independent contextual choices

Successful `await judge(...)`, JS eval title `Judging combined exact-source evidence`.

Each question used these instructions: “For every literal tched substring in the exact supplied source cluster, choose a single contextual naming disposition. Classify meaning, not substring spelling alone. External ZeroClaw-derived executable/router is threetched. Our infrastructure/filesystem lungs projection uses 3tched and 3tchedFS; preserve published path/object identifiers. Ordinary English matched/unmatched/batched/patched is not product branding. Do not infer deployed state.” Each question then named its one cluster; generated_chat additionally forbade independent generated-source edits.

| Question/cluster | Source | Actual choice | Confidence | Probabilities leave / threetched / 3tched / insufficient | Final decision |
| --- | --- | --- | --- | --- | --- |
| service | src/chat_service.rs:1-11 | leave_as_is | 0.39 | .54/.28/.15/.03 | preserve external routing owner; supplemental support below |
| generated_chat | src/orchestration/proto/op_chat.chat.rs:1-22 | leave_as_is | 0.76 | .82/.07/.01/.10 | preserve two correct derived comments |
| context_matching | src/agent_tools.rs:178-194 | leave_as_is | 0.53 | .65/.01/.01/.33 | preserve ordinary variable; manually resolved below |
| semantic_memory | src/memory_loop.rs:204-228 | leave_as_is | 0.71 | .78/0/.01/.21 | preserve matched/unmatched retrieval vocabulary |
| batch_vocabulary | src/orchestration/grpc_pool.rs:1-10 | leave_as_is | 0.87 | .90/0/0/.10 | preserve Batched execution vocabulary |
| patch_vocabulary | src/orchestration/skills_builtin/python_testing_patterns.md:41-52 | leave_as_is | 0.93 | .95/0/0/.05 | preserve mock patch example name |
| regression_vocabulary | tests/e2e_memory_loop_voyage_qdrant.rs:131-138 | leave_as_is | 0.89 | .92/0/0/.08 | preserve unmatched memory diagnostic |
| infrastructure_branding | src/system_prompt.rs:30-37,245-260 | leave_as_is | 0.36 | .52/.11/.35/.02 | preserve qualified platform/object/socket forms; manually resolved below |

All paths in this table are relative to `crates/op-chat/`.

### Submitted exact-source evidence excerpts and context

The actual call included the full indicated surrounding source clusters. These excerpts identify the occurrence-bearing lines and the decisive adjacent context:

**service** (`src/chat_service.rs:6-9`):

```rust
//! - threetched owns provider/model routing (OD-28) — SendRequest carries them.
//! - gemma_brain routes to the selected model and applies compliance tags.
//! - The chatbot is a DELEGATOR — forced tool calling is mandatory; even user
//!   responses are emitted as tool calls (respond_to_user).
```

**generated_chat** (`src/orchestration/proto/op_chat.chat.rs:13-18`, plus the file's generated header and SendRequest declaration):

```rust
    /// threetched-selected provider (OD-28)
    #[prost(string, tag = "4")]
    pub provider: ::prost::alloc::string::String,
    /// threetched-selected model (OD-28)
    #[prost(string, tag = "5")]
    pub model: ::prost::alloc::string::String,
```

**context_matching** (`src/agent_tools.rs:178-194`):

```rust
/// Register agent tools based on user context keywords.
pub async fn register_context_agents(
    registry: &ToolRegistry,
    message: &str,
) -> Result<Vec<String>> {
    let matched = match_context_agents(message);
    if matched.is_empty() {
        return Ok(Vec::new());
    }

    let descriptors = builtin_agent_descriptors();
    let mut registered = Vec::new();

    for descriptor in descriptors {
        if !matched.contains(&descriptor.agent_type) {
            continue;
        }
```

**semantic_memory** (`src/memory_loop.rs:204,225-228`, submitted with semantic_boost signature, empty-entry guard, namespace-derived container ID, embed_query_text and search_user_memory calls):

```rust
    /// Semantic boost: embed query, search Qdrant, return matched entries in priority order
        // Qdrant determines priority, while CozoDB remains the source of truth
        // for the value and tags injected into the prompt. Ignore stale vector
        // hits, de-duplicate entry keys, then append unmatched Cozo entries so
        // semantic retrieval boosts relevant memory without hiding the rest.
```

**batch_vocabulary** (`src/orchestration/grpc_pool.rs:3-10`):

```rust
//! Manages persistent gRPC connections to run-on-connection agents.
//! Provides:
//! - Connection pooling with health checks
//! - Automatic reconnection with exponential backoff
//! - Streaming support for long-running operations
//! - Batched execution for parallel phases
//! - Circuit breaker pattern for fault tolerance
//! - Comprehensive metrics and logging
```

**patch_vocabulary** (`src/orchestration/skills_builtin/python_testing_patterns.md:41-52`): Mocking subsection imports `Mock, patch, MagicMock`, supplies a mock service example, then:

```python
@patch('module.external_function')
def test_patched(mock_func):
    mock_func.return_value = "mocked"
```

**regression_vocabulary** (`tests/e2e_memory_loop_voyage_qdrant.rs:131-138`):

```rust
    ensure!(
        first_memory.contains("Rust with the Tokio async runtime"),
        "semantic memory did not rank the Rust preference first: {first_memory}"
    );
    ensure!(
        injected.domain_block.contains("Postfix and Dovecot"),
        "semantic boost hid an unmatched durable memory"
    );
```

**infrastructure_branding**: submitted base prompt lines30-37 and topology lines245-260. Occurrence-bearing text:

```text
const FIXED_BASE_PROMPT: &str = r#"3tched AI infrastructure platform — Artix Linux, runit service supervision (controlled with `sv`), Incus containers, OVS switching fabric, WARP identity transport (WireGuard terminates at the decoy).
- **Container orchestration** via Incus (`assistant`, `mail-3tched`)
The shared container socket is `/run/3tched-lungs.socket`, backed by `/run/opdbus/lungs/lungs.sock`.
mail-3tched    ERROR     —                     Mail server (needs repair)
```

The full cluster also identifies the host gRPC-bridge and xray, platform capabilities, address ownership and the Incus table. These are existing source declarations, not audited live state.

## Supplemental grounded checks

Three actual `jev_verify` calls were made, each over the matching already-read source and explicit claim:

1. **External router spelling**: claim that chat_service.rs:6 names the external provider/model routing owner, not our filesystem, and should remain qualified `threetched`. Result **supports**, confidence0.79, verbatim0.98, misleading0.3/3, tokens741/78. Quote: `threetched owns provider/model routing (OD-28)`.
2. **Matching variable**: claim that the three `matched` occurrences are keyword-selected agent-set vocabulary, not product branding. Result **insufficient**, confidence0.83, verbatim0.96, misleading1.2/3, tokens680/81. Quote: `let matched = match_context_agents(message);`.
3. **Infrastructure names**: claim that the four qualified3tched forms denote platform branding, existing Incus object references and the lungs socket, not the external executable; retain without claiming deployment. Result **insufficient**, confidence0.25 (explicit low-confidence review flag), verbatim0.97, misleading0.9/3, tokens875/81. Quote: `/run/3tched-lungs.socket`.

Insufficient results are not recorded as support. Low-confidence choice results are not decisive authority.

## Manual resolution of uncertain evidence

- `matched`: inspected full `match_context_agents` definition (src/agent_tools.rs:163-176) and its consumer (178-213). The function lowercases the message and inserts agent-type strings when rule keywords match; the local set is checked for emptiness and descriptor membership. No filesystem, mount, router or product-name referent exists. Preserve the three lexical substrings.
- `3tched` platform/object/socket forms: inspected the complete558-line prompt file and owner rule requiring preservation of correct qualified3tched forms. The first line explicitly says “AI infrastructure platform”; the socket explicitly says “shared container socket” and “lungs”; `mail-3tched` is an existing Incus object name shown twice. No bare abbreviation or external executable invocation appears. Renaming those identifiers would invent an out-of-scope migration. Preserve all four; stale topology claims do not authorize a naming change.
- service: the explicit routing-owner comment is the same external-provider meaning as chat.proto; supported by supplemental grounded check. Preserve existing qualified `threetched`.

Final disposition: **19 leave_as_is; zero replacements**. No source mutation or generated-source rewrite.
