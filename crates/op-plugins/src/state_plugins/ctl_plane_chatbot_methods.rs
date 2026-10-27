//! Durable singleton configuration and pure significance rules for the
//! control-plane chatbot. RuntimeDocuments owns the service-scoped row; this
//! module does not create an episode collection or embedding authority.

use super::ctl_plane_chatbot::{
    CtlPlaneChatbotConfig, KnowledgePush, OutcomeClass, ReasoningEpisode, Significance,
    SignificanceLevel,
};
use super::embedding_model::methods::{
    EmbedInput, EmbedOutput, EmbeddingInputType, EmbeddingProvider,
};
use super::runtime_documents::RuntimeDocuments;
use super::chat_router::{MAX_CHAT_REQUEST_BYTES, MAX_CHAT_RESPONSE_BYTES};
use anyhow::{Context, Result, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub const CONFIG_DOCUMENT_ID: &str = "ctl_plane_chatbot.singleton_config.v2";
const MAX_CONFIG_BYTES: usize = 64 * 1024;
const MAX_REASONING_SUMMARY_BYTES: usize = 64 * 1024;
const MAX_TOOLS: usize = 256;

pub struct CompletedChatEpisode {
    pub scope: op_cozo_store::EpisodeScope,
    pub conversation_id: Option<String>,
    pub goal_text: Option<String>,
    pub decision_output: String,
    pub tools: Vec<String>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub ended_at: chrono::DateTime<chrono::Utc>,
}

/// An event-triggered agent run, committed as an episode in the agent's own
/// scope (`Trigger::SystemEvent`), with its run record.
pub struct CompletedAgentRun {
    pub scope: op_cozo_store::EpisodeScope,
    pub trigger_id: String,
    pub goal_text: String,
    pub summary: String,
    pub tools: Vec<String>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub ended_at: chrono::DateTime<chrono::Utc>,
    pub run: super::ctl_plane_chatbot::RunRecord,
}

pub fn build_completed_agent_run(
    input: CompletedAgentRun,
) -> Result<op_cozo_store::ScopedEpisodeRecord> {
    use super::ctl_plane_chatbot::{ExitReason, RunStatus, Trigger};
    ensure!(
        input.ended_at >= input.started_at,
        "agent run ended before it started"
    );
    let duration_ms = u64::try_from(
        input
            .ended_at
            .signed_duration_since(input.started_at)
            .num_milliseconds(),
    )
    .context("agent run duration is invalid")?;
    let (exit_reason, outcome_class) = match input.run.status {
        RunStatus::Succeeded if input.run.actions.iter().any(|a| a.ok) => {
            (ExitReason::ToolCall, OutcomeClass::GoalAchieved)
        }
        RunStatus::Succeeded => (ExitReason::ResponseEmitted, OutcomeClass::GoalAchieved),
        RunStatus::Timeout | RunStatus::Dropped => {
            (ExitReason::Interrupt, OutcomeClass::Interrupted)
        }
        RunStatus::Failed => (ExitReason::ResponseEmitted, OutcomeClass::Inconclusive),
    };
    let mut episode = ReasoningEpisode {
        principal_id: input.scope.principal_id.clone(),
        session_id: input.scope.session_id.clone(),
        session_genesis: input.scope.session_genesis.clone(),
        on_behalf_of: input.scope.principal_id.clone(),
        episode_id: uuid::Uuid::now_v7().to_string(),
        started_at: input.started_at.to_rfc3339(),
        ended_at: input.ended_at.to_rfc3339(),
        duration_ms,
        trigger: Trigger::SystemEvent,
        exit_reason,
        goal_text: Some(input.goal_text),
        reasoning_summary: None,
        tools_consulted: input.tools,
        decision_output: Some(input.summary),
        outcome_class,
        confidence: None,
        plugin_id: Some("persona".into()),
        conversation_id: Some(format!("trigger:{}", input.trigger_id)),
        content_hash: String::new(),
        pii_flagged: true,
        run: Some(input.run),
    };
    episode.content_hash = episode_content_hash(&episode)?;
    validate_episode(&episode)?;
    Ok(op_cozo_store::ScopedEpisodeRecord {
        scope: input.scope,
        episode_id: episode.episode_id.clone(),
        content_hash: episode.content_hash.clone(),
        episode: serde_json::to_value(episode)?,
        embedding: None,
    })
}

pub fn build_completed_chat_episode(
    input: CompletedChatEpisode,
) -> Result<op_cozo_store::ScopedEpisodeRecord> {
    ensure!(
        input.ended_at >= input.started_at,
        "chat episode ended before it started"
    );
    let duration_ms = input
        .ended_at
        .signed_duration_since(input.started_at)
        .num_milliseconds();
    let duration_ms = u64::try_from(duration_ms).context("chat episode duration is invalid")?;
    let mut episode = ReasoningEpisode {
        principal_id: input.scope.principal_id.clone(),
        session_id: input.scope.session_id.clone(),
        session_genesis: input.scope.session_genesis.clone(),
        on_behalf_of: input.scope.principal_id.clone(),
        episode_id: uuid::Uuid::now_v7().to_string(),
        started_at: input.started_at.to_rfc3339(),
        ended_at: input.ended_at.to_rfc3339(),
        duration_ms,
        trigger: super::ctl_plane_chatbot::Trigger::Goal,
        exit_reason: super::ctl_plane_chatbot::ExitReason::ResponseEmitted,
        goal_text: input.goal_text,
        reasoning_summary: None,
        tools_consulted: input.tools,
        decision_output: Some(input.decision_output),
        outcome_class: OutcomeClass::Inconclusive,
        confidence: None,
        plugin_id: Some("chat_router".into()),
        conversation_id: input.conversation_id,
        content_hash: String::new(),
        pii_flagged: true,
        run: None,
    };
    episode.content_hash = episode_content_hash(&episode)?;
    validate_episode(&episode)?;
    Ok(op_cozo_store::ScopedEpisodeRecord {
        scope: input.scope,
        episode_id: episode.episode_id.clone(),
        content_hash: episode.content_hash.clone(),
        episode: serde_json::to_value(episode)?,
        embedding: None,
    })
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetConfigInput {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GetConfigOutput {
    /// Absent until an operator explicitly creates the singleton row.
    pub config: Option<CtlPlaneChatbotConfig>,
    /// Zero only when the row is absent.
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetConfigInput {
    /// Zero creates the previously absent singleton row. Any other value must
    /// exactly match the current durable revision.
    pub expected_revision: u64,
    pub config: CtlPlaneChatbotConfig,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigPublication {
    Committed,
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SetConfigOutput {
    pub config: CtlPlaneChatbotConfig,
    pub revision: u64,
    /// Whether the committed config value differs from the preceding row. A
    /// successful same-value CAS still advances the durable revision.
    pub changed: bool,
    pub publication: ConfigPublication,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClassifySignificanceInput {
    pub episode: ReasoningEpisode,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClassifySignificanceOutput {
    pub significance: Significance,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListEpisodesInput {
    #[serde(default)]
    pub after_episode_id: Option<String>,
    #[serde(default)]
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListEpisodesOutput {
    pub episodes: Vec<ReasoningEpisode>,
    pub next_episode_id: Option<String>,
    pub episode_count: u64,
    pub episode_revision: u64,
    pub episodes_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetEpisodeInput {
    pub episode_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetEpisodeOutput {
    pub episode: Option<ReasoningEpisode>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetDoorbellInput {}

/// How the verified caller rings in: its private-view owner key (also its
/// MQTT client id), the one topic it may subscribe to, and the WSS listener.
/// The credential is the caller's own door credential, never issued here.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetDoorbellOutput {
    /// `sha256(principal, session, genesis)` — opaque to the broker.
    pub owner_key: String,
    /// `opdbus/wake/<owner_key>`.
    pub topic: String,
    /// `wss://<host:port>` of the broker's doorbell listener.
    pub listener: String,
    /// Password prefixes the doorbell accepts (`sid1:` inline, `oia1:` base64).
    pub password_prefixes: Vec<String>,
    /// The private state children a wake tells the caller to re-read.
    pub knowledge_paths: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListKnowledgeInput {
    /// Newest pushes to return (0 = default 32, max 200).
    #[serde(default)]
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListKnowledgeOutput {
    /// Newest first.
    pub knowledge: Vec<KnowledgePush>,
    pub knowledge_count: u64,
    /// The scope revision (shared with episodes); a wake carries it as `seq`.
    pub episode_revision: u64,
    pub knowledge_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryContextInput {
    pub query: String,
    #[serde(default)]
    pub limit: u32,
    #[serde(default)]
    pub scan_limit: u32,
    #[serde(default)]
    pub after_episode_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextMatch {
    pub episode: ReasoningEpisode,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryContextOutput {
    pub results: Vec<ContextMatch>,
    pub next_episode_id: Option<String>,
    pub scanned_episodes: u32,
    pub indexed_episodes: u32,
    pub unvectorized_episodes: u32,
    pub incompatible_embeddings: u32,
    pub more_episodes: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VectorizeInput {
    pub episode_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VectorizeOutput {
    pub episode_id: String,
    pub content_hash: String,
    pub vector: Vec<f32>,
    pub dimensions: u32,
    pub model: String,
    pub provider_model: Option<String>,
    pub provider: EmbeddingProvider,
    pub config_revision: u64,
}

/// Exact nested request for the canonical embedding owner. PII-marked
/// episodes omit all three fields that the episode schema classifies as PII.
pub fn plan_vectorization(episode: &ReasoningEpisode) -> Result<EmbedInput> {
    validate_episode(episode)?;
    let mut lines = vec![
        format!("episode_id: {}", episode.episode_id),
        format!("trigger: {}", serde_json::to_string(&episode.trigger)?),
        format!(
            "exit_reason: {}",
            serde_json::to_string(&episode.exit_reason)?
        ),
        format!(
            "outcome_class: {}",
            serde_json::to_string(&episode.outcome_class)?
        ),
        format!("tools_consulted: {}", episode.tools_consulted.join(", ")),
    ];
    if !episode.pii_flagged {
        if let Some(goal) = &episode.goal_text {
            lines.push(format!("goal: {goal}"));
        }
        if let Some(reasoning) = &episode.reasoning_summary {
            lines.push(format!("reasoning: {reasoning}"));
        }
        if let Some(decision) = &episode.decision_output {
            lines.push(format!("decision: {decision}"));
        }
    }
    Ok(EmbedInput {
        texts: vec![lines.join("\n")],
        // The canonical embedding owner maps this semantic role only onto
        // providers that expose a native retrieval input-type field.
        input_type: Some(EmbeddingInputType::Document),
        truncate: false,
    })
}

pub fn complete_vectorization(
    episode: &ReasoningEpisode,
    mut embedded: EmbedOutput,
) -> Result<VectorizeOutput> {
    ensure!(
        embedded.vectors.len() == 1,
        "embedding owner returned an invalid vector count"
    );
    let vector = embedded.vectors.remove(0);
    ensure!(
        vector.len() == embedded.dimensions as usize,
        "embedding owner returned an invalid vector length"
    );
    Ok(VectorizeOutput {
        episode_id: episode.episode_id.clone(),
        content_hash: episode.content_hash.clone(),
        vector,
        dimensions: embedded.dimensions,
        model: embedded.model,
        provider_model: embedded.provider_model,
        provider: embedded.provider,
        config_revision: embedded.config_revision,
    })
}

pub fn cosine_similarity(left: &[f32], right: &[f32]) -> Result<f32> {
    ensure!(
        !left.is_empty() && left.len() == right.len(),
        "embedding vectors must have the same nonzero dimension"
    );
    let mut dot = 0.0_f64;
    let mut left_norm = 0.0_f64;
    let mut right_norm = 0.0_f64;
    for (&left, &right) in left.iter().zip(right) {
        ensure!(
            left.is_finite() && right.is_finite(),
            "embedding vector is not finite"
        );
        let left = f64::from(left);
        let right = f64::from(right);
        dot += left * right;
        left_norm += left * left;
        right_norm += right * right;
    }
    ensure!(
        dot.is_finite() && left_norm.is_finite() && right_norm.is_finite(),
        "embedding similarity overflowed"
    );
    if left_norm == 0.0 || right_norm == 0.0 {
        return Ok(0.0);
    }
    let score = dot / (left_norm.sqrt() * right_norm.sqrt());
    ensure!(score.is_finite(), "embedding similarity is not finite");
    Ok(score.clamp(-1.0, 1.0) as f32)
}

pub struct ConfigCommit {
    pub output: SetConfigOutput,
    /// Exact config fields represented by this durable row. MutationEngine
    /// merges these at the plugin root and preserves every unrelated child.
    pub represented_state: Value,
}

pub struct CtlPlaneChatbotMethods {
    documents: Arc<RuntimeDocuments>,
}

impl CtlPlaneChatbotMethods {
    pub fn new(documents: Arc<RuntimeDocuments>) -> Self {
        Self { documents }
    }

    pub async fn get_config(&self, input: GetConfigInput) -> Result<GetConfigOutput> {
        let _ = input;
        match self.documents.read_durable(CONFIG_DOCUMENT_ID).await? {
            None => Ok(GetConfigOutput {
                config: None,
                revision: 0,
            }),
            Some(row) => {
                let config = parse_config(row.document).with_context(|| {
                    format!(
                        "stored chatbot config at revision {} is unsupported or invalid; replace through set_config with expected_revision={}",
                        row.revision, row.revision
                    )
                })?;
                ensure!(row.revision > 0, "chatbot config row has invalid revision");
                Ok(GetConfigOutput {
                    config: Some(config),
                    revision: row.revision,
                })
            }
        }
    }

    pub async fn set_config(&self, input: SetConfigInput) -> Result<ConfigCommit> {
        validate_config(&input.config)?;
        let expected_revision = input.expected_revision;
        let candidate = input.config;
        let stored_candidate = candidate.clone();
        let (row, changed) = self
            .documents
            .update_at_revision_with_result(
                CONFIG_DOCUMENT_ID,
                expected_revision,
                move |previous| {
                    // Full replacement with an exact revision must also be
                    // able to repair an older, unsupported policy document.
                    // Validate the candidate, not the superseded intent.
                    let replacement = serde_json::to_value(&stored_candidate)?;
                    let changed = previous.as_ref() != Some(&replacement);
                    Ok((replacement, changed))
                },
            )
            .await?;
        let config = parse_config(row.document)?;
        ensure!(
            config == candidate,
            "durable chatbot config readback differs from the committed candidate"
        );
        Ok(ConfigCommit {
            represented_state: config_state_fields(&config)?,
            output: SetConfigOutput {
                config,
                revision: row.revision,
                changed,
                publication: ConfigPublication::Committed,
                diagnostic: None,
            },
        })
    }

    pub fn classify_significance(
        input: ClassifySignificanceInput,
    ) -> Result<ClassifySignificanceOutput> {
        validate_episode(&input.episode)?;
        let is_signal = matches!(
            input.episode.outcome_class,
            OutcomeClass::GoalAchieved | OutcomeClass::ConfigSet | OutcomeClass::TaskScheduled
        );
        Ok(ClassifySignificanceOutput {
            significance: Significance {
                level: if is_signal {
                    SignificanceLevel::Signal
                } else {
                    SignificanceLevel::Contextual
                },
                rule: Some(
                    "outcome_class in [goal_achieved, config_set, task_scheduled]".to_string(),
                ),
            },
        })
    }
}

pub fn config_state_fields(config: &CtlPlaneChatbotConfig) -> Result<Value> {
    let value = serde_json::to_value(config)?;
    ensure!(
        value.is_object(),
        "chatbot config must serialize as an object"
    );
    validate_config(config)?;
    Ok(value)
}

fn parse_config(value: Value) -> Result<CtlPlaneChatbotConfig> {
    let config: CtlPlaneChatbotConfig =
        serde_json::from_value(value).context("durable chatbot config is invalid")?;
    validate_config(&config)?;
    Ok(config)
}

fn validate_config(config: &CtlPlaneChatbotConfig) -> Result<()> {
    let defaults = CtlPlaneChatbotConfig::default();
    ensure!(
        config.embedding_plugin == defaults.embedding_plugin
            && config.qdrant_collection == defaults.qdrant_collection
            && config.output_dtype == defaults.output_dtype
            && config.input_type == defaults.input_type
            && config.dedup_window_hrs == defaults.dedup_window_hrs
            && config.queue_alert_threshold == defaults.queue_alert_threshold
            && config.nesting_policy == defaults.nesting_policy,
        "unsupported chatbot policy fields must retain their reserved default values"
    );
    for (field, value) in [
        ("embedding_plugin", config.embedding_plugin.as_str()),
        ("qdrant_collection", config.qdrant_collection.as_str()),
    ] {
        ensure!(
            !value.is_empty() && value.trim() == value && value.len() <= 256,
            "chatbot {field} must be 1..=256 bytes without surrounding whitespace"
        );
        ensure!(
            !value.chars().any(char::is_control),
            "chatbot {field} contains control characters"
        );
    }
    let bytes = serde_json::to_vec(config)?;
    ensure!(
        bytes.len() <= MAX_CONFIG_BYTES,
        "chatbot config exceeds {MAX_CONFIG_BYTES} bytes"
    );
    Ok(())
}

pub fn validate_episode(episode: &ReasoningEpisode) -> Result<()> {
    let bytes = serde_json::to_vec(episode)?;
    ensure!(
        bytes.len() <= op_cozo_store::MAX_CHATBOT_EPISODE_BYTES,
        "reasoning episode exceeds the durable episode bound"
    );
    ensure!(
        !episode.episode_id.is_empty() && episode.episode_id.len() <= 128,
        "reasoning episode_id must be 1..=128 bytes"
    );
    ensure!(
        episode.started_at.len() <= 64 && episode.ended_at.len() <= 64,
        "reasoning timestamps exceed 64 bytes"
    );
    let started = chrono::DateTime::parse_from_rfc3339(&episode.started_at)
        .context("reasoning started_at is not RFC3339")?;
    let ended = chrono::DateTime::parse_from_rfc3339(&episode.ended_at)
        .context("reasoning ended_at is not RFC3339")?;
    ensure!(ended >= started, "reasoning ended_at predates started_at");
    ensure!(
        episode
            .reasoning_summary
            .as_ref()
            .is_none_or(|value| value.len() <= MAX_REASONING_SUMMARY_BYTES),
        "reasoning summary exceeds {MAX_REASONING_SUMMARY_BYTES} bytes"
    );
    ensure!(
        !episode.principal_id.is_empty()
            && !episode.session_id.is_empty()
            && !episode.session_genesis.is_empty()
            && !episode.on_behalf_of.is_empty()
            && episode.on_behalf_of == episode.principal_id,
        "reasoning episode identity scope is invalid"
    );
    ensure!(
        episode
            .goal_text
            .as_ref()
            .is_none_or(|value| value.len() <= MAX_CHAT_REQUEST_BYTES)
            && episode
                .decision_output
                .as_ref()
                .is_none_or(|value| value.len() <= MAX_CHAT_RESPONSE_BYTES),
        "reasoning goal or decision exceeds the canonical Chat transport bound"
    );
    ensure!(
        episode.tools_consulted.len() <= MAX_TOOLS
            && episode
                .tools_consulted
                .iter()
                .all(|tool| !tool.is_empty() && tool.len() <= 256),
        "reasoning tools_consulted is invalid"
    );
    ensure!(
        episode
            .confidence
            .is_none_or(|value| value.is_finite() && (0.0..=1.0).contains(&value)),
        "reasoning confidence must be finite and between zero and one"
    );
    if let Some(run) = &episode.run {
        ensure!(
            !run.trigger_id.is_empty() && run.trigger_id.len() <= 64,
            "run trigger_id must be 1..=64 bytes"
        );
        ensure!(
            run.topic_ref.len() == 64 && run.topic_ref.bytes().all(|b| b.is_ascii_hexdigit()),
            "run topic_ref must be a 64-hex digest"
        );
        ensure!(run.actions.len() <= 64, "run records at most 64 actions");
        ensure!(
            run.actions.iter().all(|a| {
                !a.method.is_empty()
                    && a.method.len() <= 256
                    && a.error.as_ref().is_none_or(|e| e.len() <= 1024)
            }),
            "run action is invalid"
        );
        ensure!(
            run.reason.as_ref().is_none_or(|r| r.len() <= 1024),
            "run reason exceeds 1024 bytes"
        );
    }
    ensure!(
        episode.content_hash.len() == 64
            && episode
                .content_hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit()),
        "reasoning content_hash must be a 64-character hexadecimal SHA-256"
    );
    ensure!(
        episode.content_hash == episode_content_hash(episode)?,
        "reasoning content_hash does not match the episode"
    );
    Ok(())
}

/// SHA-256 of the typed episode serialized with its `content_hash` field
/// retained and set to the empty string.
pub fn episode_content_hash(episode: &ReasoningEpisode) -> Result<String> {
    let mut content = episode.clone();
    content.content_hash.clear();
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&content)?)
    ))
}

/// Largest durable knowledge push (typed, serialized).
pub const MAX_KNOWLEDGE_PUSH_BYTES: usize = 64 * 1024;

pub fn validate_knowledge_push(push: &KnowledgePush) -> Result<()> {
    let bytes = serde_json::to_vec(push)?;
    ensure!(
        bytes.len() <= MAX_KNOWLEDGE_PUSH_BYTES,
        "knowledge push exceeds {MAX_KNOWLEDGE_PUSH_BYTES} bytes"
    );
    ensure!(
        !push.principal_id.is_empty()
            && !push.session_id.is_empty()
            && !push.session_genesis.is_empty(),
        "knowledge push identity scope is invalid"
    );
    ensure!(
        push.push_id.len() == 20 && push.push_id.bytes().all(|b| b.is_ascii_digit()),
        "knowledge push_id must be the 20-digit zero-padded scope revision"
    );
    ensure!(
        !push.source_id.is_empty()
            && push.source_id.len() <= 128
            && !push.source_id.chars().any(char::is_control),
        "knowledge source_id must be 1..=128 bytes without control characters"
    );
    ensure!(
        push.created_at.len() <= 64,
        "knowledge created_at exceeds 64 bytes"
    );
    chrono::DateTime::parse_from_rfc3339(&push.created_at)
        .context("knowledge created_at is not RFC3339")?;
    ensure!(
        !push.trigger.is_empty() && push.trigger.len() <= 64,
        "knowledge trigger must be 1..=64 bytes"
    );
    ensure!(
        push.trigger_reason.len() <= 4096
            && push
                .suggested_action
                .as_ref()
                .is_none_or(|value| value.len() <= 4096),
        "knowledge reason or suggestion exceeds 4096 bytes"
    );
    ensure!(
        push.relevance_score.is_finite() && (0.0..=1.0).contains(&push.relevance_score),
        "knowledge relevance_score must be finite and between zero and one"
    );
    ensure!(
        push.content_hash.len() == 64
            && push
                .content_hash
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "knowledge content_hash must be a 64-character hexadecimal SHA-256"
    );
    ensure!(
        push.content_hash == knowledge_content_hash(push)?,
        "knowledge content_hash does not match the push"
    );
    Ok(())
}

/// SHA-256 of the typed push serialized with its `content_hash` field
/// retained and set to the empty string.
pub fn knowledge_content_hash(push: &KnowledgePush) -> Result<String> {
    let mut content = push.clone();
    content.content_hash.clear();
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&content)?)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_cozo_store::CozoGraphShuttle;

    fn service() -> CtlPlaneChatbotMethods {
        CtlPlaneChatbotMethods::new(Arc::new(RuntimeDocuments::new(
            CozoGraphShuttle::new_in_memory().unwrap(),
        )))
    }

    fn episode(outcome_class: OutcomeClass) -> ReasoningEpisode {
        let mut episode = ReasoningEpisode {
            principal_id: "did:op:human:test".into(),
            session_id: "test-session".into(),
            session_genesis: "ab".repeat(32),
            on_behalf_of: "did:op:human:test".into(),
            episode_id: "01912abc-def0-7abc-8def-0123456789ab".into(),
            started_at: "2026-09-13T10:00:00Z".into(),
            ended_at: "2026-09-13T10:00:01Z".into(),
            duration_ms: 1_000,
            trigger: super::super::ctl_plane_chatbot::Trigger::Goal,
            exit_reason: super::super::ctl_plane_chatbot::ExitReason::ResponseEmitted,
            goal_text: None,
            reasoning_summary: Some("bounded summary".into()),
            tools_consulted: vec![],
            decision_output: None,
            outcome_class,
            confidence: Some(0.75),
            plugin_id: None,
            conversation_id: None,
            content_hash: String::new(),
            pii_flagged: false,
            run: None,
        };
        episode.content_hash = episode_content_hash(&episode).unwrap();
        episode
    }

    #[tokio::test]
    async fn absent_config_remains_absent_then_revision_cas_is_exact() {
        let service = service();
        assert_eq!(
            service.get_config(GetConfigInput {}).await.unwrap(),
            GetConfigOutput {
                config: None,
                revision: 0
            }
        );
        let first = service
            .set_config(SetConfigInput {
                expected_revision: 0,
                config: CtlPlaneChatbotConfig::default(),
            })
            .await
            .unwrap();
        assert_eq!(first.output.revision, 1);
        assert!(first.output.changed);
        assert!(
            service
                .set_config(SetConfigInput {
                    expected_revision: 0,
                    config: CtlPlaneChatbotConfig::default(),
                })
                .await
                .is_err()
        );
        assert_eq!(
            service
                .get_config(GetConfigInput {})
                .await
                .unwrap()
                .revision,
            1
        );
    }

    #[tokio::test]
    async fn same_value_cas_advances_revision_without_claiming_content_change() {
        let service = service();
        service
            .set_config(SetConfigInput {
                expected_revision: 0,
                config: CtlPlaneChatbotConfig::default(),
            })
            .await
            .unwrap();
        let second = service
            .set_config(SetConfigInput {
                expected_revision: 1,
                config: CtlPlaneChatbotConfig::default(),
            })
            .await
            .unwrap();
        assert_eq!(second.output.revision, 2);
        assert!(!second.output.changed);
    }

    #[tokio::test]
    async fn unsupported_reserved_policies_are_rejected_before_revision_cas() {
        let service = service();
        for (field, value) in unsupported_policy_values() {
            let mut config = serde_json::to_value(CtlPlaneChatbotConfig::default()).unwrap();
            config[field] = value;
            let error = service
                .set_config(SetConfigInput {
                    expected_revision: 0,
                    config: serde_json::from_value(config).unwrap(),
                })
                .await
                .err()
                .expect("unsupported policy must fail");
            assert!(error.to_string().contains("reserved default values"));
            assert_eq!(
                service
                    .get_config(GetConfigInput {})
                    .await
                    .unwrap()
                    .revision,
                0
            );
        }
    }

    #[tokio::test]
    async fn unsupported_legacy_config_can_be_replaced_through_revision_cas() {
        let service = service();
        let mut legacy = serde_json::to_value(CtlPlaneChatbotConfig::default()).unwrap();
        legacy["qdrant_collection"] = serde_json::json!("legacy-unimplemented-collection");
        service
            .documents
            .update_at_revision_with_result(CONFIG_DOCUMENT_ID, 0, move |_| Ok((legacy, ())))
            .await
            .unwrap();
        let error = service.get_config(GetConfigInput {}).await.unwrap_err();
        assert!(error.to_string().contains("expected_revision=1"));
        assert!(
            service
                .set_config(SetConfigInput {
                    expected_revision: 0,
                    config: CtlPlaneChatbotConfig::default(),
                })
                .await
                .is_err()
        );
        let repaired = service
            .set_config(SetConfigInput {
                expected_revision: 1,
                config: CtlPlaneChatbotConfig::default(),
            })
            .await
            .unwrap();
        assert!(repaired.output.changed);
        assert_eq!(repaired.output.revision, 2);
        assert_eq!(
            service.get_config(GetConfigInput {}).await.unwrap().config,
            Some(CtlPlaneChatbotConfig::default())
        );
    }

    fn unsupported_policy_values() -> [(&'static str, Value); 7] {
        [
            ("embedding_plugin", serde_json::json!("other")),
            ("qdrant_collection", serde_json::json!("other")),
            ("dedup_window_hrs", serde_json::json!(12)),
            ("queue_alert_threshold", serde_json::json!(1)),
            ("nesting_policy", serde_json::json!("nested")),
            ("output_dtype", serde_json::json!("int8")),
            ("input_type", serde_json::json!("query")),
        ]
    }

    #[tokio::test]
    async fn singleton_config_reopens_from_the_same_durable_row() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("chatbot-config");
        {
            let service = CtlPlaneChatbotMethods::new(Arc::new(RuntimeDocuments::new(
                CozoGraphShuttle::new_persistent(path.clone()).unwrap(),
            )));
            let committed = service
                .set_config(SetConfigInput {
                    expected_revision: 0,
                    config: CtlPlaneChatbotConfig::default(),
                })
                .await
                .unwrap();
            assert_eq!(committed.output.revision, 1);
        }
        let reopened = CtlPlaneChatbotMethods::new(Arc::new(RuntimeDocuments::new(
            CozoGraphShuttle::new_persistent(path).unwrap(),
        )));
        let observed = reopened.get_config(GetConfigInput {}).await.unwrap();
        assert_eq!(observed.revision, 1);
        assert_eq!(observed.config, Some(CtlPlaneChatbotConfig::default()));
    }

    #[test]
    fn config_schema_rejects_unimplemented_policy_values() {
        let schema = serde_json::to_value(schemars::schema_for!(SetConfigInput)).unwrap();
        let validator = jsonschema::Validator::new(&schema).unwrap();
        let baseline = serde_json::to_value(SetConfigInput {
            expected_revision: 0,
            config: CtlPlaneChatbotConfig::default(),
        })
        .unwrap();
        assert!(validator.is_valid(&baseline));
        for (field, value) in unsupported_policy_values() {
            let mut input = baseline.clone();
            input["config"][field] = value;
            assert!(!validator.is_valid(&input), "unsupported field: {field}");
        }
    }

    #[test]
    fn significance_rule_is_exact_and_has_no_storage_side_effect() {
        for outcome in [
            OutcomeClass::GoalAchieved,
            OutcomeClass::ConfigSet,
            OutcomeClass::TaskScheduled,
        ] {
            assert_eq!(
                CtlPlaneChatbotMethods::classify_significance(ClassifySignificanceInput {
                    episode: episode(outcome)
                })
                .unwrap()
                .significance
                .level,
                SignificanceLevel::Signal
            );
        }
        assert_eq!(
            CtlPlaneChatbotMethods::classify_significance(ClassifySignificanceInput {
                episode: episode(OutcomeClass::Inconclusive)
            })
            .unwrap()
            .significance
            .level,
            SignificanceLevel::Contextual
        );
    }

    #[test]
    fn malformed_episode_and_unknown_input_are_rejected() {
        let mut invalid = episode(OutcomeClass::Inconclusive);
        invalid.confidence = Some(f64::NAN);
        assert!(
            CtlPlaneChatbotMethods::classify_significance(ClassifySignificanceInput {
                episode: invalid
            })
            .is_err()
        );
        assert!(
            serde_json::from_value::<ClassifySignificanceInput>(serde_json::json!({
                "episode": episode(OutcomeClass::Inconclusive),
                "vector_id": "invented"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<SetConfigInput>(serde_json::json!({
                "expected_revision":0,
                "config":{
                    "embedding_plugin":"embedding_model",
                    "qdrant_collection":"ctl_plane_reasoning_episodes",
                    "vector_dims":"1024",
                    "output_dtype":"float",
                    "input_type":"document",
                    "dedup_window_hrs":24,
                    "queue_alert_threshold":50,
                    "nesting_policy":"flat",
                    "vectorization_enabled":true,
                    "credential":"must-not-be-accepted"
                }
            }))
            .is_err()
        );
    }

    #[test]
    fn vectorization_plan_uses_typed_episode_and_redacts_pii_text() {
        let mut value = episode(OutcomeClass::GoalAchieved);
        value.goal_text = Some("private goal".into());
        value.reasoning_summary = Some("private reasoning".into());
        value.decision_output = Some("private decision".into());
        value.pii_flagged = true;
        value.content_hash = episode_content_hash(&value).unwrap();
        let plan = plan_vectorization(&value).unwrap();
        assert_eq!(plan.texts.len(), 1);
        assert!(!plan.texts[0].contains("private goal"));
        assert!(!plan.texts[0].contains("private reasoning"));
        assert!(!plan.texts[0].contains("private decision"));
        assert!(plan.texts[0].contains("outcome_class"));
        assert!(matches!(
            plan.input_type,
            Some(EmbeddingInputType::Document)
        ));
        assert!(!plan.truncate);

        let mut public = episode(OutcomeClass::GoalAchieved);
        public.goal_text = Some("public goal".into());
        public.reasoning_summary = Some("public reasoning".into());
        public.decision_output = Some("public decision".into());
        public.content_hash = episode_content_hash(&public).unwrap();
        let plan = plan_vectorization(&public).unwrap();
        assert!(plan.texts[0].contains("public goal"));
        assert!(plan.texts[0].contains("public reasoning"));
        assert!(plan.texts[0].contains("public decision"));
    }

    #[test]
    fn vectorization_completion_requires_one_exact_dimension_vector() {
        let input = episode(OutcomeClass::Inconclusive);
        let embedded = EmbedOutput {
            vectors: vec![vec![0.25, 0.75]],
            dimensions: 2,
            model: "fixture-model".into(),
            provider_model: Some("reported-model".into()),
            provider: EmbeddingProvider::Voyage,
            config_revision: 4,
        };
        let output = complete_vectorization(&input, embedded).unwrap();
        assert_eq!(output.episode_id, input.episode_id);
        assert_eq!(output.content_hash, input.content_hash);
        assert_eq!(output.vector, vec![0.25, 0.75]);
        assert_eq!(output.config_revision, 4);

        let invalid = EmbedOutput {
            vectors: vec![vec![0.25]],
            dimensions: 2,
            model: "fixture-model".into(),
            provider_model: None,
            provider: EmbeddingProvider::Voyage,
            config_revision: 4,
        };
        assert!(complete_vectorization(&input, invalid).is_err());
    }

    #[test]
    fn cosine_similarity_is_bounded_and_rejects_invalid_vectors() {
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]).unwrap(), 1.0);
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[-1.0, 0.0]).unwrap(), -1.0);
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 0.0]).unwrap(), 0.0);
        assert!(cosine_similarity(&[], &[]).is_err());
        assert!(cosine_similarity(&[1.0], &[1.0, 2.0]).is_err());
        assert!(cosine_similarity(&[f32::NAN], &[1.0]).is_err());
    }

    #[test]
    fn completed_chat_episode_binds_identity_and_content_hash() {
        let started_at = chrono::DateTime::parse_from_rfc3339("2026-09-14T10:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let ended_at = started_at + chrono::Duration::seconds(2);
        let row = build_completed_chat_episode(CompletedChatEpisode {
            scope: op_cozo_store::EpisodeScope {
                principal_id: "did:op:human:test".into(),
                session_id: "session".into(),
                session_genesis: "ab".repeat(32),
            },
            conversation_id: Some("conversation".into()),
            goal_text: Some("goal".into()),
            decision_output: "answer".into(),
            tools: vec![],
            started_at,
            ended_at,
        })
        .unwrap();
        let episode: ReasoningEpisode = serde_json::from_value(row.episode.clone()).unwrap();
        assert_eq!(episode.on_behalf_of, row.scope.principal_id);
        assert_eq!(episode.duration_ms, 2_000);
        assert!(episode.reasoning_summary.is_none());
        assert!(episode.pii_flagged);
        assert_eq!(row.content_hash, episode_content_hash(&episode).unwrap());
    }

    #[test]
    fn completed_chat_episode_preserves_supported_large_chat_text() {
        let started_at = chrono::DateTime::parse_from_rfc3339("2026-09-14T10:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let goal = "g".repeat(128 * 1024);
        let decision = "d".repeat(192 * 1024);
        let row = build_completed_chat_episode(CompletedChatEpisode {
            scope: op_cozo_store::EpisodeScope {
                principal_id: "did:op:human:test".into(),
                session_id: "session".into(),
                session_genesis: "ab".repeat(32),
            },
            conversation_id: None,
            goal_text: Some(goal.clone()),
            decision_output: decision.clone(),
            tools: vec![],
            started_at,
            ended_at: started_at + chrono::Duration::seconds(1),
        })
        .unwrap();
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        store.initialize_chatbot_episode_store().unwrap();
        store.insert_scoped_episode_and_snapshot(&row, 32).unwrap();
        let observed = store
            .get_scoped_episode(&row.scope, &row.episode_id)
            .unwrap()
            .unwrap();
        let episode: ReasoningEpisode = serde_json::from_value(observed.episode).unwrap();
        assert_eq!(episode.goal_text.as_deref(), Some(goal.as_str()));
        assert_eq!(episode.decision_output.as_deref(), Some(decision.as_str()));
        assert_eq!(episode.content_hash, row.content_hash);
    }
}
