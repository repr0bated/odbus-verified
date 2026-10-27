# J4 — every-occurrence accuracy/intent evidence

Continuation of native-line-audit-20261024/op-cognitive on2026-10-24. This is a supplemental declarative `judge(state, questions)` receipt, not JEV routing. Existing full sequential reads retained. No coordinator-followup.md appeared in the assignment directory listing. Five pre-edit case-insensitive substring occurrences are accounted for: four semantic native/natively uses plus one conservative embedded substring inside alternative. No search or extraction used.

## Evidence supplied to JEV

### O1 — cognitive_tools.rs:779-797

```rust
#[test]
fn qdrant_payload_serializes_as_native_values() {
 let payload: qdrant_client::Payload = serde_json::json!({"container_id":"session/a/personal", "entry_key":"k", "content":"current"}).try_into().unwrap();
 let point = qdrant_client::qdrant::PointStruct::new(uuid::Uuid::new_v4().to_string(), vec![1.0], payload);
 assert_eq!(serde_json::to_value(&point.payload).unwrap(), serde_json::json!({"container_id":"session/a/personal", "entry_key":"k", "content":"current"}));
}
```

### O2 — voyage.rs:23-33,65-85,124-130

```rust
#[derive(Serialize)]
struct EmbeddingRequest<'a> {
 input: Vec<&'a str>,
 model: &'a str,
 input_type: Option<&'a str>,
 /// Pinned to the shared-space dimension so callers always land in the same
 /// Qdrant collection regardless of which model in the trio is active —
 /// without this, Voyage returns each model's native (differing) dimension.
 truncation: bool,
 output_dimension: u32,
}
```

Also supplied: constructor resolves voyage_embed_params endpoint/key/model; request uses `model: &self.model`, `truncation: true`, `output_dimension: VoyageModel::SHARED_EMBEDDING_DIMS`; returned dimensions and finite values checked. Owner policy from loaded AGENTS: Voyage-4 variants share1024 dimensions and one compatible embedding space; _large/_lite identify model quality tiers. The already-read integration fixture labels its1024 constant as shared-space dimension for voyage-4-large/voyage-4/voyage-4-lite.

Proposed comment-only replacement:

```rust
 truncation: bool,
 /// Request the shared embedding width used by this client's consumers.
 /// Supported Voyage-4 variants share a vector space; pinning the width
 /// does not select or override the configured embedding model.
 output_dimension: u32,
```

### O3 — rag_pipeline.rs:1223-1238

```rust
/// Build a Qdrant client from environment.
///
/// Connection is always TCP to localhost. When the qdrant container socket is
/// declared via UNIX_SOCKET_ENDPOINTS (e.g. qdrant:/run/qdrant.sock:6334),
/// xray transparently proxies 127.0.0.1:6334 → container unix socket using
/// its native domain-socket ("network": "ds") outbound — no custom tonic
/// connector needed here.
///
/// Override the URL with COGNITIVE_QDRANT_URL.
pub fn qdrant_client_from_env() -> Result<Qdrant> {
 let url = std::env::var("COGNITIVE_QDRANT_URL").unwrap_or_else(|_| "http://127.0.0.1:6334".into());
 Qdrant::from_url(&url).build().context("Failed to build Qdrant client")
}
```

Also supplied: function does not configure Xray forwarding; no live topology receipt. Proposed comment:

```rust
/// Build a Qdrant client from COGNITIVE_QDRANT_URL (loopback TCP by default).
///
/// Any container Unix-socket forwarding, including Xray's native domain-socket
/// ("network": "ds") outbound, must be configured separately; this function
/// only constructs a client for the selected URL.
```

### O4 — e2e_voyage_qdrant.rs:112-125,163-193

```rust
// 1. Voyage vectorization
let ovs_doc = "The rovs plugin drives Open vSwitch natively over OVSDB JSON-RPC: it creates bridges, attaches ports, and installs OpenFlow rules.";
let mail_doc = "The mail-3tched container runs Postfix and Dovecot, exposing SMTP submission on 587 and IMAPS on 993 for mailbox delivery.";
let ovs_vector = shuttle.embed_document(ovs_doc).await.context("Voyage embed_document failed for the OVS document")?;
let mail_vector = shuttle.embed_document(mail_doc).await.context("Voyage embed_document failed for the mail document")?;
```

Also supplied: test embeds an OVS port-addition question and expects ovs-note to rank first. Semantic similarity input, not OVS implementation/topology probe.

### O5 — gemini_fallback.rs:277-303

```rust
// Steps 2..depth: Drill deeper
let drill_prompts = [
 "What are the key technical details and implementation specifics?",
 "What are the trade-offs, limitations, and alternative approaches?",
 "What are the security implications and best practices?",
 "What are the performance characteristics and optimization strategies?",
];
for step in 1..depth as usize {
 let prompt_idx = (step - 1).min(drill_prompts.len() - 1);
 let drill_prompt = format!("Regarding '{}': {}\n\nBased on what we know so far:\n{}", topic, drill_prompts[prompt_idx], accumulated_knowledge);
 match self.gemini_query(&drill_prompt, Some(&accumulated_knowledge)).await { /* result appends knowledge and section */ }
}
```

Excerpts above normalize indentation/line wrapping exactly as submitted; summarized surrounding logic is labeled, not represented as verbatim complete source.

## Questions and actual answers

| Question | Explicit criteria | Answer | Confidence | Probabilities |
| --- | --- | --- | --- | --- |
| O1_accuracy: Is native_values accurate against the JSON payload assertion? | preserve / correct_wording / insufficient | preserve | .70 | .80 / .18 / .02 |
| O2_accuracy: Is differing native width supported by shared1024 owner contract; does proposed comment correct it without behavior/admission change? | correct_as_proposed / preserve_existing / insufficient | correct_as_proposed | .97 | .98 / .01 / .01 |
| O3_accuracy: Is whole native-socket helper description justified; should separate-configuration wording replace overclaim? | correct_as_proposed / preserve_existing / insufficient | correct_as_proposed | .98 | .99 / 0 / .01 |
| O4_accuracy: Is natively intelligible fixture prose rather than live OVS proof? | preserve_fixture / correct_wording / insufficient | preserve_fixture | .98 | .99 / .01 / 0 |
| O5_accuracy: Does native inside alternative have separate native semantics or require repair? | preserve_alternative / correct_wording / insufficient | preserve_alternative | 1 | 1 / 0 / 0 |

Human decision: O1 .70 is above the .60 escalation threshold; assertion explicitly compares ordinary JSON strings/object against exact expected values, so retain. O2/O3: implement bounded documentation corrections. O4/O5: preserve. This supersedes J1/J2's too-narrow preservation decisions for wording accuracy at O2/O3. No runtime/library/deployment acceptance follows from these source-only judgments.
