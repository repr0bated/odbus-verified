//! Context Awareness & Proactive Knowledge Pushing
//!
//! Monitors session activity, detects context changes, and proactively pushes
//! relevant knowledge to connected clients via SSE notifications.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                    Session Activity Stream                           │
//! │  (tool calls, queries, context switches, idle patterns)            │
//! └──────────────────────────┬────────────────────────────────────────┘
//!                            │
//!                            ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                   Context Monitor (Background Task)                │
//! │  • Tracks active sessions and their states                          │
//! │  • Detects patterns (new topics, stuck sessions, context gaps)     │
//! │  • Maintains sliding window of recent activity                      │
//! └──────────────────────────┬────────────────────────────────────────┘
//!                            │
//!                            ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                 Knowledge Trigger Engine                           │
//! │  • Evaluates context against trigger rules                            │
//! │  • Scores knowledge relevance                                         │
//! │  • Rate-limits pushes to avoid spam                                   │
//! └──────────────────────────┬────────────────────────────────────────┘
//!                            │
//!                            ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                    RAG Retriever                                     │
//! │  • Semantic search based on context                                   │
//! │  • Hybrid: recent activity + predicted needs                        │
//! │  • Filters already-known information                                  │
//! └──────────────────────────┬────────────────────────────────────────┘
//!                            │
//!                            ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                SSE Push Notifications (Proactive)                  │
//! │  notification: {                                                     │
//! │    type: "knowledge_push",                                           │
//! │    trigger: "new_topic_detected",                                    │
//! │    relevance_score: 0.92,                                            │
//! │    content: { ... },                                                 │
//! │    suggested_action: "review_architecture_docs"                     │
//! │  }                                                                   │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Trigger Types
//!
//! - `new_topic_detected` - User started working on a new domain/area
//! - `stuck_session` - Repeated similar queries or errors detected
//! - `context_gap` - Query seems incomplete or missing prerequisite knowledge
//! - `pattern_match` - Usage pattern suggests need for specific resource
//! - `session_milestone` - Nth query in session, suggest review/summary
//! - `idle_recovery` - User returned after idle, refresh context

use crate::memory_store::{CognitiveMemoryStore, EntryQuery};
use crate::rag_pipeline::{default_collection_from_env, RagPipeline};
use anyhow::{Context as AnyhowContext, Result};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio::time::{interval, timeout};
use tracing::{info, trace, warn};

/// Maximum activity window to keep per session
const ACTIVITY_WINDOW_SIZE: usize = 50;
/// Minimum time between proactive pushes to same session
const MIN_PUSH_INTERVAL_SECS: u64 = 30;
/// Idle threshold for context refresh (seconds)
const IDLE_REFRESH_THRESHOLD_SECS: u64 = 300; // 5 minutes
/// Trigger evaluation interval (milliseconds)
const EVALUATION_INTERVAL_MS: u64 = 5000;
/// Minimum relevance score to push knowledge
const MIN_RELEVANCE_SCORE: f32 = 0.75;

/// Types of knowledge push triggers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PushTrigger {
    /// User started working on a new topic/domain
    NewTopicDetected,
    /// Session seems stuck (repeated similar queries/errors)
    StuckSession,
    /// Query appears incomplete or missing context
    ContextGap,
    /// Usage pattern suggests specific resource need
    PatternMatch,
    /// Session milestone reached (e.g., 10th query)
    SessionMilestone,
    /// User returned after idle period
    IdleRecovery,
    /// Tool usage suggests knowledge gap
    ToolGuidance,
    /// Error pattern detected
    ErrorAssistance,
}

impl std::fmt::Display for PushTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PushTrigger::NewTopicDetected => write!(f, "new_topic_detected"),
            PushTrigger::StuckSession => write!(f, "stuck_session"),
            PushTrigger::ContextGap => write!(f, "context_gap"),
            PushTrigger::PatternMatch => write!(f, "pattern_match"),
            PushTrigger::SessionMilestone => write!(f, "session_milestone"),
            PushTrigger::IdleRecovery => write!(f, "idle_recovery"),
            PushTrigger::ToolGuidance => write!(f, "tool_guidance"),
            PushTrigger::ErrorAssistance => write!(f, "error_assistance"),
        }
    }
}

/// A proactive knowledge push notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgePush {
    /// Unique push ID
    pub id: String,
    /// Session ID this push is for
    pub session_id: String,
    /// What triggered this push
    pub trigger: PushTrigger,
    /// Human-readable trigger description
    pub trigger_reason: String,
    /// Relevance score (0.0 - 1.0)
    pub relevance_score: f32,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Pushed knowledge content
    pub content: KnowledgeContent,
    /// Suggested action for user
    pub suggested_action: Option<String>,
    /// Whether this was automatically pushed or on-demand
    pub proactive: bool,
}

/// Content types for knowledge pushes
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum KnowledgeContent {
    /// Memory entries from the knowledge base
    MemoryEntries {
        namespace: String,
        entries: Vec<serde_json::Value>,
        context: String,
    },
    /// RAG search results
    RagResults {
        query: String,
        results: Vec<serde_json::Value>,
        sources: Vec<String>,
    },
    /// Session context summary
    ContextSummary {
        session_overview: String,
        related_namespaces: Vec<String>,
        suggested_next_steps: Vec<String>,
    },
    /// Tool usage guidance
    ToolGuidance {
        tool_name: String,
        usage_tips: Vec<String>,
        example_usage: serde_json::Value,
    },
    /// Error resolution guidance
    ErrorAssistance {
        error_pattern: String,
        resolution_steps: Vec<String>,
        related_docs: Vec<String>,
    },
}

/// Tracked activity within a session
#[derive(Debug, Clone)]
pub struct SessionActivity {
    pub timestamp: Instant,
    pub activity_type: ActivityType,
    pub content: String,
    pub metadata: serde_json::Value,
}

/// Types of session activity
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityType {
    ToolCall,
    Query,
    ContextSwitch,
    Error,
    Idle,
    ReturnFromIdle,
    // ─── coding signals ───
    /// A source file was opened/viewed in the workspace.
    FileOpened,
    /// An edit was applied to a source file.
    EditApplied,
    /// A build/compile error occurred (counts as an error signal).
    BuildError,
    /// A test failed (counts as an error signal).
    TestFailure,
    /// A diff/changeset was viewed.
    DiffViewed,
    /// Navigation to a symbol (definition/reference).
    SymbolNavigated,
}

impl ActivityType {
    /// Whether this activity should increment the recent-error counter used by
    /// stuck/error-assistance detection. Build errors and test failures are
    /// first-class error signals for coding sessions.
    pub fn is_error_signal(self) -> bool {
        matches!(
            self,
            ActivityType::Error | ActivityType::BuildError | ActivityType::TestFailure
        )
    }

    /// Parse a wire string into an `ActivityType`. Unknown values fall back to
    /// `Query` so callers never reject a request on a typo. Shared by the HTTP
    /// `/context/record` handler and the `code_context` MCP tool.
    pub fn parse(s: &str) -> Self {
        match s {
            "tool_call" => ActivityType::ToolCall,
            "query" => ActivityType::Query,
            "context_switch" => ActivityType::ContextSwitch,
            "error" => ActivityType::Error,
            "idle" => ActivityType::Idle,
            "return_from_idle" => ActivityType::ReturnFromIdle,
            "file_opened" => ActivityType::FileOpened,
            "edit_applied" => ActivityType::EditApplied,
            "build_error" => ActivityType::BuildError,
            "test_failure" => ActivityType::TestFailure,
            "diff_viewed" => ActivityType::DiffViewed,
            "symbol_navigated" => ActivityType::SymbolNavigated,
            _ => ActivityType::Query,
        }
    }
}

/// Per-session context state
#[derive(Debug)]
struct SessionContextState {
    session_id: String,
    /// Sliding window of recent activity
    activity_window: VecDeque<SessionActivity>,
    /// Last proactive push timestamp
    last_push: Option<Instant>,
    /// Last activity timestamp (for idle detection)
    last_activity: Instant,
    /// Current topic/domain being worked on
    current_topics: Vec<String>,
    /// Error count in recent window
    recent_error_count: u32,
    /// Query similarity tracking (for stuck detection)
    query_hashes: VecDeque<u64>,
    /// Push suppression flags
    suppressed_triggers: Vec<PushTrigger>,
}

impl SessionContextState {
    fn new(session_id: String) -> Self {
        Self {
            session_id,
            activity_window: VecDeque::with_capacity(ACTIVITY_WINDOW_SIZE),
            last_push: None,
            last_activity: Instant::now(),
            current_topics: Vec::new(),
            recent_error_count: 0,
            query_hashes: VecDeque::with_capacity(10),
            suppressed_triggers: Vec::new(),
        }
    }

    fn record_activity(
        &mut self,
        activity_type: ActivityType,
        content: String,
        metadata: serde_json::Value,
    ) {
        self.last_activity = Instant::now();

        if activity_type.is_error_signal() {
            self.recent_error_count += 1;
        }

        let activity = SessionActivity {
            timestamp: Instant::now(),
            activity_type,
            content: content.clone(),
            metadata,
        };

        self.activity_window.push_back(activity);

        // Keep window size bounded
        while self.activity_window.len() > ACTIVITY_WINDOW_SIZE {
            if let Some(removed) = self.activity_window.pop_front() {
                if removed.activity_type.is_error_signal() {
                    self.recent_error_count = self.recent_error_count.saturating_sub(1);
                }
            }
        }

        // Track query hashes for stuck detection
        if activity_type == ActivityType::Query {
            let hash = Self::compute_hash(&content);
            self.query_hashes.push_back(hash);
            while self.query_hashes.len() > 10 {
                self.query_hashes.pop_front();
            }
        }
    }

    fn compute_hash(content: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        hasher.finish()
    }

    fn is_idle(&self) -> bool {
        self.last_activity.elapsed().as_secs() > IDLE_REFRESH_THRESHOLD_SECS
    }

    fn can_push(&self) -> bool {
        if let Some(last) = self.last_push {
            last.elapsed().as_secs() >= MIN_PUSH_INTERVAL_SECS
        } else {
            true
        }
    }

    fn record_push(&mut self) {
        self.last_push = Some(Instant::now());
    }

    /// Detect if session appears stuck (similar repeated queries)
    fn is_stuck(&self) -> bool {
        if self.query_hashes.len() < 3 {
            return false;
        }

        // Check for repeated similar queries (simplified: exact hash match)
        let recent: Vec<_> = self.query_hashes.iter().rev().take(3).collect();
        recent.windows(2).any(|w| w[0] == w[1])
    }

    fn suppress_trigger(&mut self, trigger: PushTrigger) {
        if !self.suppressed_triggers.contains(&trigger) {
            self.suppressed_triggers.push(trigger);
        }
    }

    fn is_suppressed(&self, trigger: PushTrigger) -> bool {
        self.suppressed_triggers.contains(&trigger)
    }
}

/// Context awareness engine configuration
#[derive(Debug, Clone)]
pub struct ContextAwarenessConfig {
    /// Enable proactive pushing
    pub proactive_enabled: bool,
    /// Enable pattern detection
    pub pattern_detection_enabled: bool,
    /// Minimum relevance threshold
    pub min_relevance_score: f32,
    /// Push rate limit (seconds between pushes)
    pub push_rate_limit_secs: u64,
    /// Idle threshold for context refresh
    pub idle_threshold_secs: u64,
    /// Maximum pushes per session per hour
    pub max_pushes_per_hour: u32,
    /// Qdrant collection used for code/repomix RAG retrieval
    pub rag_collection: String,
}

impl Default for ContextAwarenessConfig {
    fn default() -> Self {
        Self {
            proactive_enabled: true,
            pattern_detection_enabled: true,
            min_relevance_score: MIN_RELEVANCE_SCORE,
            push_rate_limit_secs: MIN_PUSH_INTERVAL_SECS,
            idle_threshold_secs: IDLE_REFRESH_THRESHOLD_SECS,
            max_pushes_per_hour: 20,
            rag_collection: default_collection_from_env(),
        }
    }
}

/// Context awareness engine with proactive knowledge pushing
pub struct ContextAwarenessEngine {
    config: ContextAwarenessConfig,
    /// Session context states
    session_states: Arc<DashMap<String, RwLock<SessionContextState>>>,
    /// Memory store for context retrieval
    memory_store: Arc<CognitiveMemoryStore>,
    /// RAG pipeline for semantic search
    rag_pipeline: Option<Arc<RagPipeline>>,
    /// Broadcast channel for knowledge pushes
    push_tx: broadcast::Sender<KnowledgePush>,
    /// Activity event receiver
    activity_rx: Arc<RwLock<mpsc::Receiver<ActivityEvent>>>,
    /// Activity event sender (cloneable)
    activity_tx: mpsc::Sender<ActivityEvent>,
}

/// Activity event for the engine to process
#[derive(Debug, Clone)]
pub struct ActivityEvent {
    pub session_id: String,
    pub activity_type: ActivityType,
    pub content: String,
    pub metadata: serde_json::Value,
}

/// Result of trigger evaluation
#[derive(Debug)]
struct TriggerEvaluation {
    should_push: bool,
    trigger: Option<PushTrigger>,
    trigger_reason: String,
    relevance_score: f32,
    context_query: String,
}

impl ContextAwarenessEngine {
    /// Create new context awareness engine
    pub fn new(
        config: ContextAwarenessConfig,
        memory_store: Arc<CognitiveMemoryStore>,
        rag_pipeline: Option<Arc<RagPipeline>>,
    ) -> Self {
        let (push_tx, _push_rx) = broadcast::channel(100);
        let (activity_tx, activity_rx) = mpsc::channel(1000);

        Self {
            config,
            session_states: Arc::new(DashMap::new()),
            memory_store,
            rag_pipeline,
            push_tx,
            activity_rx: Arc::new(RwLock::new(activity_rx)),
            activity_tx,
        }
    }

    /// Get a sender for activity events
    pub fn activity_sender(&self) -> mpsc::Sender<ActivityEvent> {
        self.activity_tx.clone()
    }

    /// Subscribe to knowledge pushes
    pub fn subscribe_pushes(&self) -> broadcast::Receiver<KnowledgePush> {
        self.push_tx.subscribe()
    }

    /// Record activity for context tracking
    pub async fn record_activity(
        &self,
        session_id: &str,
        activity_type: ActivityType,
        content: String,
        metadata: serde_json::Value,
    ) {
        // Get or create session state
        let state = self
            .session_states
            .entry(session_id.to_string())
            .or_insert_with(|| RwLock::new(SessionContextState::new(session_id.to_string())));

        // Record in session state
        state
            .write()
            .await
            .record_activity(activity_type, content.clone(), metadata.clone());

        // Send to processing channel
        let event = ActivityEvent {
            session_id: session_id.to_string(),
            activity_type,
            content,
            metadata,
        };

        let _ = self.activity_tx.send(event).await;
    }

    /// Snapshot the awareness signals for a session (async, never blocks).
    ///
    /// Returns `None` if the session has no recorded activity. Used by the
    /// `code_context` tool to report stuck/error/topic state alongside results.
    pub async fn get_session_signals(&self, session_id: &str) -> Option<serde_json::Value> {
        let entry = self.session_states.get(session_id)?;
        let state = entry.read().await;
        Some(serde_json::json!({
            "session_id": state.session_id,
            "activity_count": state.activity_window.len(),
            "recent_error_count": state.recent_error_count,
            "is_stuck": state.is_stuck(),
            "is_idle": state.is_idle(),
            "current_topics": state.current_topics,
        }))
    }

    /// Start the background context monitoring task
    pub fn start_monitoring(self: Arc<Self>) {
        tokio::spawn(async move {
            info!("Context awareness monitoring started");

            let mut eval_interval = interval(Duration::from_millis(EVALUATION_INTERVAL_MS));

            loop {
                eval_interval.tick().await;

                // Process any pending activity events
                self.process_activity_events().await;

                // Evaluate all sessions for proactive pushes
                if self.config.proactive_enabled {
                    self.evaluate_all_sessions().await;
                }
            }
        });
    }

    async fn process_activity_events(&self) {
        let mut rx = self.activity_rx.write().await;

        while let Ok(Some(event)) = timeout(Duration::from_millis(10), rx.recv()).await {
            trace!(
                session_id = %event.session_id,
                activity_type = ?event.activity_type,
                "Processing activity event"
            );

            // Additional processing if needed
            if event.activity_type == ActivityType::Error {
                // Immediately evaluate for error assistance
                if let Some(state) = self.session_states.get(&event.session_id) {
                    let state = state.read().await;
                    if state.can_push() && !state.is_suppressed(PushTrigger::ErrorAssistance) {
                        drop(state);
                        if let Err(e) = self.evaluate_error_assistance(&event.session_id).await {
                            warn!(error = %e, "Error assistance evaluation failed");
                        }
                    }
                }
            }
        }
    }

    async fn evaluate_all_sessions(&self) {
        for entry in self.session_states.iter() {
            let session_id = entry.key().clone();
            let state = entry.value().read().await;

            if !state.can_push() {
                continue;
            }

            // Check various trigger conditions
            let evaluation = self.evaluate_session(&state).await;

            if evaluation.should_push
                && evaluation.relevance_score >= self.config.min_relevance_score
            {
                if let Some(trigger) = evaluation.trigger {
                    drop(state);

                    if let Err(e) = self
                        .generate_and_push(
                            &session_id,
                            trigger,
                            evaluation.trigger_reason,
                            evaluation.relevance_score,
                            evaluation.context_query,
                        )
                        .await
                    {
                        warn!(error = %e, session_id = %session_id, "Failed to generate knowledge push");
                    }
                }
            }
        }
    }

    async fn evaluate_session(&self, state: &SessionContextState) -> TriggerEvaluation {
        // Check for stuck session
        if state.is_stuck() && !state.is_suppressed(PushTrigger::StuckSession) {
            return TriggerEvaluation {
                should_push: true,
                trigger: Some(PushTrigger::StuckSession),
                trigger_reason: "Session appears stuck with repeated similar queries".to_string(),
                relevance_score: 0.9,
                context_query: self.build_context_query(state),
            };
        }

        // Check for high error rate
        if state.recent_error_count >= 3 && !state.is_suppressed(PushTrigger::ErrorAssistance) {
            return TriggerEvaluation {
                should_push: true,
                trigger: Some(PushTrigger::ErrorAssistance),
                trigger_reason: format!(
                    "Multiple recent errors detected ({}",
                    state.recent_error_count
                ),
                relevance_score: 0.95,
                context_query: self.build_context_query(state),
            };
        }

        // Check for idle recovery
        if state.is_idle() && !state.is_suppressed(PushTrigger::IdleRecovery) {
            // Check if there's a return from idle activity
            let has_return = state.activity_window.iter().any(|a| {
                a.activity_type == ActivityType::ReturnFromIdle
                    && a.timestamp.elapsed().as_secs() < 60
            });

            if has_return {
                return TriggerEvaluation {
                    should_push: true,
                    trigger: Some(PushTrigger::IdleRecovery),
                    trigger_reason: "User returned after idle period, refreshing context"
                        .to_string(),
                    relevance_score: 0.85,
                    context_query: self.build_context_query(state),
                };
            }
        }

        // Check for new topic (simple heuristic: sudden change in keywords)
        // This is a simplified version - a real implementation would use NLP
        let recent_queries: Vec<_> = state
            .activity_window
            .iter()
            .filter(|a| a.activity_type == ActivityType::Query)
            .rev()
            .take(5)
            .map(|a| a.content.clone())
            .collect();

        if recent_queries.len() >= 2 {
            let last = recent_queries[0].to_lowercase();
            let previous = recent_queries[1].to_lowercase();

            // Simple topic change detection
            let topic_keywords = vec![
                (
                    "database",
                    vec!["postgres", "sqlite", "query", "schema", "table"],
                ),
                ("api", vec!["endpoint", "rest", "grpc", "http", "request"]),
                ("frontend", vec!["react", "component", "ui", "html", "css"]),
                (
                    "infrastructure",
                    vec!["docker", "kubernetes", "deploy", "server"],
                ),
                (
                    "security",
                    vec!["auth", "encrypt", "token", "vulnerability", "oauth"],
                ),
            ];

            for (topic, keywords) in topic_keywords {
                let last_mentions = keywords.iter().any(|k| last.contains(k));
                let prev_mentions = keywords.iter().any(|k| previous.contains(k));

                if last_mentions
                    && !prev_mentions
                    && !state.is_suppressed(PushTrigger::NewTopicDetected)
                {
                    return TriggerEvaluation {
                        should_push: true,
                        trigger: Some(PushTrigger::NewTopicDetected),
                        trigger_reason: format!("Detected new topic area: {}", topic),
                        relevance_score: 0.88,
                        context_query: self.build_context_query(state),
                    };
                }
            }
        }

        // Default: no push
        TriggerEvaluation {
            should_push: false,
            trigger: None,
            trigger_reason: String::new(),
            relevance_score: 0.0,
            context_query: String::new(),
        }
    }

    async fn evaluate_error_assistance(&self, session_id: &str) -> Result<()> {
        let state = self
            .session_states
            .get(session_id)
            .context("Session not found")?;

        let state = state.read().await;

        if !state.can_push() {
            return Ok(());
        }

        let context_query = self.build_context_query(&state);
        drop(state);

        self.generate_and_push(
            session_id,
            PushTrigger::ErrorAssistance,
            "Error pattern detected - offering assistance".to_string(),
            0.95,
            context_query,
        )
        .await
    }

    fn build_context_query(&self, state: &SessionContextState) -> String {
        // Build a query from recent activity for RAG
        let recent: Vec<_> = state
            .activity_window
            .iter()
            .rev()
            .take(5)
            .filter(|a| {
                a.activity_type == ActivityType::Query || a.activity_type == ActivityType::ToolCall
            })
            .map(|a| a.content.clone())
            .collect();

        recent.join(" ")
    }

    async fn generate_and_push(
        &self,
        session_id: &str,
        trigger: PushTrigger,
        trigger_reason: String,
        relevance_score: f32,
        context_query: String,
    ) -> Result<()> {
        // Retrieve relevant knowledge
        let content = self
            .retrieve_relevant_knowledge(&context_query, &trigger)
            .await?;

        let push = KnowledgePush {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            trigger,
            trigger_reason,
            relevance_score,
            timestamp: chrono::Utc::now(),
            content,
            suggested_action: self.suggest_action(&trigger),
            proactive: true,
        };

        // Update session state
        if let Some(state) = self.session_states.get(session_id) {
            let mut state = state.write().await;
            state.record_push();
            state.suppress_trigger(trigger); // Don't push same trigger repeatedly
        }

        // Broadcast the push
        let _ = self.push_tx.send(push.clone());

        info!(
            push_id = %push.id,
            session_id = %session_id,
            trigger = %trigger,
            relevance = relevance_score,
            "Knowledge push generated"
        );

        Ok(())
    }

    async fn retrieve_relevant_knowledge(
        &self,
        query: &str,
        trigger: &PushTrigger,
    ) -> Result<KnowledgeContent> {
        match trigger {
            PushTrigger::StuckSession
            | PushTrigger::NewTopicDetected
            | PushTrigger::IdleRecovery => {
                // Use RAG for semantic search
                if let Some(ref rag) = self.rag_pipeline {
                    let results = rag
                        .query(&self.config.rag_collection, query, 5, None)
                        .await?;

                    let result_values: Vec<serde_json::Value> = results
                        .into_iter()
                        .map(|r| serde_json::to_value(r).unwrap_or_default())
                        .collect();

                    let sources: Vec<String> = result_values
                        .iter()
                        .filter_map(|r| r.get("repo").and_then(|s| s.as_str()).map(String::from))
                        .collect();

                    return Ok(KnowledgeContent::RagResults {
                        query: query.to_string(),
                        results: result_values,
                        sources,
                    });
                }

                // Fallback to memory search
                let memory_query = EntryQuery {
                    namespace_id: None,
                    key_pattern: Some(query.to_string()),
                    tags: None,
                    limit: Some(5),
                    offset: None,
                };

                let entries = self.memory_store.query_entries(memory_query).await?;

                Ok(KnowledgeContent::MemoryEntries {
                    namespace: "context_search".to_string(),
                    entries: entries
                        .into_iter()
                        .map(|e| serde_json::to_value(e).unwrap_or_default())
                        .collect(),
                    context: query.to_string(),
                })
            }

            PushTrigger::ErrorAssistance => {
                // Search for error-related documentation
                let error_query = format!("{} error troubleshooting solution", query);

                let memory_query = EntryQuery {
                    namespace_id: None,
                    key_pattern: Some(error_query),
                    tags: Some(vec!["error".to_string(), "troubleshoot".to_string()]),
                    limit: Some(5),
                    offset: None,
                };

                let entries = self.memory_store.query_entries(memory_query).await?;

                Ok(KnowledgeContent::ErrorAssistance {
                    error_pattern: query.to_string(),
                    resolution_steps: entries
                        .iter()
                        .filter_map(|e| e.value.as_str().map(String::from))
                        .collect(),
                    related_docs: vec!["error_handling_guide".to_string()],
                })
            }

            PushTrigger::ToolGuidance => Ok(KnowledgeContent::ToolGuidance {
                tool_name: query.to_string(),
                usage_tips: vec![
                    "Check tool documentation before use".to_string(),
                    "Verify required parameters".to_string(),
                    "Review example usage patterns".to_string(),
                ],
                example_usage: serde_json::json!({
                    "tool": query,
                    "params": "{}",
                    "notes": "See documentation for detailed examples"
                }),
            }),

            _ => {
                // Default: context summary
                Ok(KnowledgeContent::ContextSummary {
                    session_overview: format!("Recent activity focused on: {}", query),
                    related_namespaces: vec![
                        "project:op-dbus".to_string(),
                        "session:current".to_string(),
                    ],
                    suggested_next_steps: vec![
                        "Review related documentation".to_string(),
                        "Explore connected namespaces".to_string(),
                        "Check session history for patterns".to_string(),
                    ],
                })
            }
        }
    }

    fn suggest_action(&self, trigger: &PushTrigger) -> Option<String> {
        match trigger {
            PushTrigger::NewTopicDetected => Some("review_topic_documentation".to_string()),
            PushTrigger::StuckSession => Some("try_different_approach".to_string()),
            PushTrigger::ErrorAssistance => Some("follow_error_resolution_steps".to_string()),
            PushTrigger::IdleRecovery => Some("review_session_summary".to_string()),
            _ => None,
        }
    }

    /// Manually trigger a knowledge push for a session
    pub async fn request_push(&self, session_id: &str, query: &str) -> Result<KnowledgePush> {
        let content = self
            .retrieve_relevant_knowledge(query, &PushTrigger::PatternMatch)
            .await?;

        let push = KnowledgePush {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            trigger: PushTrigger::PatternMatch,
            trigger_reason: "User requested knowledge".to_string(),
            relevance_score: 1.0,
            timestamp: chrono::Utc::now(),
            content,
            suggested_action: Some("review_pushed_knowledge".to_string()),
            proactive: false,
        };

        // Broadcast
        let _ = self.push_tx.send(push.clone());

        Ok(push)
    }

    /// Get session context statistics
    pub async fn get_session_stats(&self, session_id: &str) -> Option<serde_json::Value> {
        self.session_states.get(session_id).map(|state| {
            let state = state.blocking_read();
            serde_json::json!({
                "session_id": state.session_id,
                "activity_count": state.activity_window.len(),
                "recent_error_count": state.recent_error_count,
                "is_idle": state.is_idle(),
                "can_push": state.can_push(),
                "current_topics": state.current_topics,
                "suppressed_triggers": state.suppressed_triggers.iter().map(|t| t.to_string()).collect::<Vec<_>>(),
            })
        })
    }

    /// Clear suppressed triggers for a session (e.g., after explicit user action)
    pub async fn reset_suppressions(&self, session_id: &str) {
        if let Some(state) = self.session_states.get(session_id) {
            let mut state = state.write().await;
            state.suppressed_triggers.clear();
        }
    }

    /// Get count of active tracked sessions
    pub fn get_active_session_count(&self) -> usize {
        self.session_states.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_detect_stuck_session() {
        let mut state = SessionContextState::new("test".to_string());

        // Add similar queries
        for _i in 0..5 {
            state.record_activity(
                ActivityType::Query,
                "how to fix error".to_string(),
                serde_json::json!({}),
            );
        }

        assert!(state.is_stuck());
    }

    #[test]
    fn should_not_detect_stuck_with_varied_queries() {
        let mut state = SessionContextState::new("test".to_string());

        state.record_activity(
            ActivityType::Query,
            "how to fix database error".to_string(),
            serde_json::json!({}),
        );
        state.record_activity(
            ActivityType::Query,
            "how to configure api endpoint".to_string(),
            serde_json::json!({}),
        );
        state.record_activity(
            ActivityType::Query,
            "how to deploy application".to_string(),
            serde_json::json!({}),
        );

        assert!(!state.is_stuck());
    }

    #[test]
    fn should_track_error_count() {
        let mut state = SessionContextState::new("test".to_string());

        state.record_activity(
            ActivityType::Error,
            "error 1".to_string(),
            serde_json::json!({}),
        );
        state.record_activity(
            ActivityType::Error,
            "error 2".to_string(),
            serde_json::json!({}),
        );
        state.record_activity(
            ActivityType::Query,
            "query".to_string(),
            serde_json::json!({}),
        );

        assert_eq!(state.recent_error_count, 2);
    }

    #[test]
    fn should_respect_push_rate_limit() {
        let mut state = SessionContextState::new("test".to_string());

        assert!(state.can_push()); // First push allowed

        state.record_push();
        assert!(!state.can_push()); // Too soon
    }
}
