//! Workflow pattern detection and automatic promotion
//!
//! Tracks sequences of agent calls and automatically promotes
//! frequently-used patterns to first-class workflows.
//!
//! Features:
//! - Call sequence tracking with frequency counts
//! - Configurable promotion thresholds
//! - Pattern similarity detection
//! - Workflow definition export

use anyhow::{Context, Result};
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::info;

/// Configuration for workflow pattern detection
#[derive(Debug, Clone)]
pub struct WorkflowTrackerConfig {
    /// Minimum calls before considering promotion (default: 3)
    pub promotion_threshold: u32,
    /// Time window in seconds for pattern detection (default: 24 hours)
    pub detection_window_secs: i64,
    /// Minimum sequence length to track (default: 2)
    pub min_sequence_length: usize,
    /// Maximum sequence length to track (default: 10)
    pub max_sequence_length: usize,
    /// Auto-promote when threshold reached (default: false, suggest only)
    pub auto_promote: bool,
}

impl Default for WorkflowTrackerConfig {
    fn default() -> Self {
        Self {
            promotion_threshold: 3,
            detection_window_secs: 86400, // 24 hours
            min_sequence_length: 2,
            max_sequence_length: 10,
            auto_promote: false,
        }
    }
}

/// Detected workflow pattern
#[derive(Debug, Clone)]
pub struct WorkflowPattern {
    pub pattern_id: String,
    pub agent_sequence: Vec<String>,
    pub call_count: u32,
    pub first_seen: i64,
    pub last_called: i64,
    pub avg_latency_ms: u64,
    pub promoted: bool,
    pub workflow_id: Option<String>,
}

impl WorkflowPattern {
    /// Check if pattern meets promotion threshold
    pub fn meets_threshold(&self, threshold: u32) -> bool {
        self.call_count >= threshold && !self.promoted
    }

    /// Get human-readable sequence description
    pub fn sequence_description(&self) -> String {
        self.agent_sequence.join(" → ")
    }
}

/// Workflow promotion suggestion
#[derive(Debug, Clone)]
pub struct PromotionSuggestion {
    pub pattern: WorkflowPattern,
    pub estimated_time_saved_ms: u64,
    pub confidence_score: f64,
    pub suggested_name: String,
}

pub struct WorkflowTracker {
    db: Mutex<rusqlite::Connection>,
    config: WorkflowTrackerConfig,
    /// In-memory buffer for current session sequences
    session_buffer: Mutex<Vec<AgentCall>>,
}

#[derive(Debug, Clone)]
struct AgentCall {
    agent_id: String,
    #[allow(dead_code)]
    input_hash: String,
    #[allow(dead_code)]
    timestamp: i64,
    #[allow(dead_code)]
    latency_ms: u64,
}

impl WorkflowTracker {
    /// Create new workflow tracker with SQLite persistence
    pub async fn new(cache_dir: PathBuf, config: WorkflowTrackerConfig) -> Result<Self> {
        let db_path = cache_dir.join("workflows/tracker.db");

        // Ensure directory exists
        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let db = rusqlite::Connection::open(&db_path)
            .context("Failed to open workflow tracker database")?;

        // Create tables
        db.execute_batch(
            r#"
            -- Pattern tracking table
            CREATE TABLE IF NOT EXISTS workflow_patterns (
                pattern_hash TEXT PRIMARY KEY,
                agent_sequence TEXT NOT NULL,
                call_count INTEGER DEFAULT 1,
                first_seen INTEGER NOT NULL,
                last_called INTEGER NOT NULL,
                total_latency_ms INTEGER DEFAULT 0,
                promoted INTEGER DEFAULT 0,
                workflow_id TEXT
            );

            -- Individual call log for analysis
            CREATE TABLE IF NOT EXISTS agent_calls (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                input_hash TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                latency_ms INTEGER NOT NULL
            );

            -- Detected sequences (sliding window analysis)
            CREATE TABLE IF NOT EXISTS detected_sequences (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                sequence_hash TEXT NOT NULL,
                agent_sequence TEXT NOT NULL,
                detected_at INTEGER NOT NULL
            );

            -- Promoted workflows
            CREATE TABLE IF NOT EXISTS promoted_workflows (
                workflow_id TEXT PRIMARY KEY,
                pattern_hash TEXT NOT NULL,
                name TEXT NOT NULL,
                description TEXT,
                agent_sequence TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                execution_count INTEGER DEFAULT 0,
                FOREIGN KEY (pattern_hash) REFERENCES workflow_patterns(pattern_hash)
            );

            -- Indexes for efficient queries
            CREATE INDEX IF NOT EXISTS idx_patterns_count ON workflow_patterns(call_count DESC);
            CREATE INDEX IF NOT EXISTS idx_patterns_last_called ON workflow_patterns(last_called DESC);
            CREATE INDEX IF NOT EXISTS idx_calls_session ON agent_calls(session_id, timestamp);
            CREATE INDEX IF NOT EXISTS idx_calls_timestamp ON agent_calls(timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_sequences_hash ON detected_sequences(sequence_hash);
            "#,
        )?;

        info!("Workflow tracker initialized at {:?}", db_path);

        Ok(Self {
            db: Mutex::new(db),
            config,
            session_buffer: Mutex::new(Vec::new()),
        })
    }

    /// Record an agent call
    pub fn record_call(
        &self,
        session_id: &str,
        agent_id: &str,
        input_hash: &str,
        latency_ms: u64,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();

        // Add to session buffer
        {
            let mut buffer = self.session_buffer.lock().unwrap();
            buffer.push(AgentCall {
                agent_id: agent_id.to_string(),
                input_hash: input_hash.to_string(),
                timestamp: now,
                latency_ms,
            });

            // Trim buffer if too large
            if buffer.len() > self.config.max_sequence_length * 2 {
                buffer.drain(0..self.config.max_sequence_length);
            }
        }

        // Persist to database
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO agent_calls (session_id, agent_id, input_hash, timestamp, latency_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![session_id, agent_id, input_hash, now, latency_ms],
        )?;

        drop(db);

        // Analyze for patterns after each call
        self.analyze_session_patterns(session_id)?;

        Ok(())
    }

    /// Record a complete agent sequence (batch recording)
    pub fn record_sequence(
        &self,
        agents: &[&str],
        _input_hash: &str,
        total_latency_ms: u64,
    ) -> Result<Option<PromotionSuggestion>> {
        if agents.len() < self.config.min_sequence_length {
            return Ok(None);
        }

        let sequence_hash = self.hash_sequence(agents);
        let agent_sequence_json = simd_json::to_string(agents)?;
        let now = chrono::Utc::now().timestamp();
        let _avg_latency = total_latency_ms / agents.len() as u64;

        let db = self.db.lock().unwrap();

        // Check if pattern exists
        let existing: Option<(u32, i64, i64, bool)> = db
            .query_row(
                "SELECT call_count, first_seen, total_latency_ms, promoted
                 FROM workflow_patterns WHERE pattern_hash = ?1",
                [&sequence_hash],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;

        let (call_count, first_seen, total_latency, promoted) = if let Some(existing) = existing {
            // Update existing pattern
            db.execute(
                "UPDATE workflow_patterns
                 SET call_count = call_count + 1,
                     last_called = ?1,
                     total_latency_ms = total_latency_ms + ?2
                 WHERE pattern_hash = ?3",
                rusqlite::params![now, total_latency_ms, sequence_hash],
            )?;
            (
                existing.0 + 1,
                existing.1,
                existing.2 + total_latency_ms as i64,
                existing.3,
            )
        } else {
            // Insert new pattern
            db.execute(
                "INSERT INTO workflow_patterns
                 (pattern_hash, agent_sequence, call_count, first_seen, last_called, total_latency_ms)
                 VALUES (?1, ?2, 1, ?3, ?3, ?4)",
                rusqlite::params![sequence_hash, agent_sequence_json, now, total_latency_ms],
            )?;
            (1, now, total_latency_ms as i64, false)
        };

        drop(db);

        // Check for promotion
        if call_count >= self.config.promotion_threshold && !promoted {
            let pattern = WorkflowPattern {
                pattern_id: sequence_hash.clone(),
                agent_sequence: agents.iter().map(|s| s.to_string()).collect(),
                call_count,
                first_seen,
                last_called: now,
                avg_latency_ms: (total_latency / call_count as i64) as u64,
                promoted: false,
                workflow_id: None,
            };

            let suggestion = PromotionSuggestion {
                estimated_time_saved_ms: self.estimate_time_savings(&pattern),
                confidence_score: self.calculate_confidence(&pattern),
                suggested_name: self.generate_workflow_name(&pattern),
                pattern,
            };

            if self.config.auto_promote {
                self.promote_pattern(&suggestion.pattern)?;
            }

            return Ok(Some(suggestion));
        }

        Ok(None)
    }

    /// Analyze session for emerging patterns (sliding window)
    fn analyze_session_patterns(&self, session_id: &str) -> Result<()> {
        let buffer = self.session_buffer.lock().unwrap();

        if buffer.len() < self.config.min_sequence_length {
            return Ok(());
        }

        // Extract sequences of various lengths
        for window_size in self.config.min_sequence_length..=self.config.max_sequence_length {
            if buffer.len() < window_size {
                break;
            }

            let start = buffer.len() - window_size;
            let window = &buffer[start..];

            let agents: Vec<&str> = window.iter().map(|c| c.agent_id.as_str()).collect();
            let sequence_hash = self.hash_sequence(&agents);
            let agent_sequence_json =
                simd_json::to_string(&agents).unwrap_or_else(|_| "[]".to_string());
            let now = chrono::Utc::now().timestamp();

            // Record detected sequence
            let db = self.db.lock().unwrap();
            db.execute(
                "INSERT INTO detected_sequences (session_id, sequence_hash, agent_sequence, detected_at)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![session_id, sequence_hash, agent_sequence_json, now],
            )?;
        }

        Ok(())
    }

    /// Promote a pattern to a first-class workflow
    pub fn promote_pattern(&self, pattern: &WorkflowPattern) -> Result<String> {
        let workflow_id = format!("WF-{}", &pattern.pattern_id[..8]);
        let now = chrono::Utc::now().timestamp();
        let agent_sequence_json = simd_json::to_string(&pattern.agent_sequence)?;

        let db = self.db.lock().unwrap();

        // Create workflow entry
        db.execute(
            "INSERT INTO promoted_workflows
             (workflow_id, pattern_hash, name, description, agent_sequence, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                workflow_id,
                pattern.pattern_id,
                self.generate_workflow_name(pattern),
                format!(
                    "Auto-promoted workflow from pattern detected {} times",
                    pattern.call_count
                ),
                agent_sequence_json,
                now
            ],
        )?;

        // Mark pattern as promoted
        db.execute(
            "UPDATE workflow_patterns
             SET promoted = 1, workflow_id = ?1
             WHERE pattern_hash = ?2",
            rusqlite::params![workflow_id, pattern.pattern_id],
        )?;

        info!(
            "Promoted pattern {} to workflow {}: {}",
            pattern.pattern_id,
            workflow_id,
            pattern.sequence_description()
        );

        Ok(workflow_id)
    }

    /// Get patterns eligible for promotion
    pub fn get_promotion_candidates(&self) -> Result<Vec<PromotionSuggestion>> {
        let db = self.db.lock().unwrap();
        let cutoff = chrono::Utc::now().timestamp() - self.config.detection_window_secs;

        let mut stmt = db.prepare(
            "SELECT pattern_hash, agent_sequence, call_count, first_seen, last_called, total_latency_ms
             FROM workflow_patterns
             WHERE call_count >= ?1
               AND promoted = 0
               AND last_called > ?2
             ORDER BY call_count DESC",
        )?;

        let patterns = stmt
            .query_map(
                rusqlite::params![self.config.promotion_threshold, cutoff],
                |row| {
                    let agent_sequence_json: String = row.get(1)?;
                    let agent_sequence: Vec<String> =
                        serde_json::from_str(&agent_sequence_json).unwrap_or_default();
                    let call_count: u32 = row.get(2)?;
                    let total_latency: i64 = row.get(5)?;

                    Ok(WorkflowPattern {
                        pattern_id: row.get(0)?,
                        agent_sequence,
                        call_count,
                        first_seen: row.get(3)?,
                        last_called: row.get(4)?,
                        avg_latency_ms: (total_latency / call_count as i64) as u64,
                        promoted: false,
                        workflow_id: None,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(patterns
            .into_iter()
            .map(|pattern| PromotionSuggestion {
                estimated_time_saved_ms: self.estimate_time_savings(&pattern),
                confidence_score: self.calculate_confidence(&pattern),
                suggested_name: self.generate_workflow_name(&pattern),
                pattern,
            })
            .collect())
    }

    /// Get all promoted workflows
    pub fn get_promoted_workflows(&self) -> Result<Vec<PromotedWorkflow>> {
        let db = self.db.lock().unwrap();

        let mut stmt = db.prepare(
            "SELECT workflow_id, pattern_hash, name, description, agent_sequence, created_at, execution_count
             FROM promoted_workflows
             ORDER BY created_at DESC",
        )?;

        let workflows = stmt
            .query_map([], |row| {
                let agent_sequence_json: String = row.get(4)?;
                let agent_sequence: Vec<String> =
                    serde_json::from_str(&agent_sequence_json).unwrap_or_default();

                Ok(PromotedWorkflow {
                    workflow_id: row.get(0)?,
                    pattern_hash: row.get(1)?,
                    name: row.get(2)?,
                    description: row.get(3)?,
                    agent_sequence,
                    created_at: row.get(5)?,
                    execution_count: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(workflows)
    }

    /// Get a specific workflow by ID
    pub fn get_workflow(&self, workflow_id: &str) -> Result<Option<PromotedWorkflow>> {
        let db = self.db.lock().unwrap();

        let workflow = db
            .query_row(
                "SELECT workflow_id, pattern_hash, name, description, agent_sequence, created_at, execution_count
                 FROM promoted_workflows
                 WHERE workflow_id = ?1",
                [workflow_id],
                |row| {
                    let agent_sequence_json: String = row.get(4)?;
                    let agent_sequence: Vec<String> =
                        serde_json::from_str(&agent_sequence_json).unwrap_or_default();

                    Ok(PromotedWorkflow {
                        workflow_id: row.get(0)?,
                        pattern_hash: row.get(1)?,
                        name: row.get(2)?,
                        description: row.get(3)?,
                        agent_sequence,
                        created_at: row.get(5)?,
                        execution_count: row.get(6)?,
                    })
                },
            )
            .optional()?;

        Ok(workflow)
    }

    /// Record workflow execution
    pub fn record_execution(&self, workflow_id: &str) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE promoted_workflows SET execution_count = execution_count + 1 WHERE workflow_id = ?1",
            [workflow_id],
        )?;
        Ok(())
    }

    /// Get tracker statistics
    pub fn stats(&self) -> Result<TrackerStats> {
        let db = self.db.lock().unwrap();

        let total_patterns: u32 =
            db.query_row("SELECT COUNT(*) FROM workflow_patterns", [], |row| {
                row.get(0)
            })?;

        let promoted_count: u32 = db.query_row(
            "SELECT COUNT(*) FROM workflow_patterns WHERE promoted = 1",
            [],
            |row| row.get(0),
        )?;

        let pending_promotion: u32 = db.query_row(
            "SELECT COUNT(*) FROM workflow_patterns WHERE call_count >= ?1 AND promoted = 0",
            [self.config.promotion_threshold],
            |row| row.get(0),
        )?;

        let total_calls: u64 =
            db.query_row("SELECT COUNT(*) FROM agent_calls", [], |row| row.get(0))?;

        let total_workflow_executions: u64 = db.query_row(
            "SELECT COALESCE(SUM(execution_count), 0) FROM promoted_workflows",
            [],
            |row| row.get(0),
        )?;

        Ok(TrackerStats {
            total_patterns,
            promoted_count,
            pending_promotion,
            total_calls,
            total_workflow_executions,
            promotion_threshold: self.config.promotion_threshold,
        })
    }

    /// Hash a sequence of agents for pattern identification
    fn hash_sequence(&self, agents: &[&str]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(agents.join("→").as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Estimate time savings from caching this workflow
    fn estimate_time_savings(&self, pattern: &WorkflowPattern) -> u64 {
        // Assume 40% cache hit rate on subsequent executions
        // and 60% latency reduction when cached
        let expected_future_calls = pattern.call_count * 2; // Extrapolate
        let cache_hit_savings = (pattern.avg_latency_ms as f64 * 0.6) as u64;
        let hit_rate = 0.4;

        (expected_future_calls as f64 * cache_hit_savings as f64 * hit_rate) as u64
    }

    /// Calculate confidence score for promotion
    fn calculate_confidence(&self, pattern: &WorkflowPattern) -> f64 {
        let recency_days = (chrono::Utc::now().timestamp() - pattern.last_called) as f64 / 86400.0;
        let frequency_score =
            (pattern.call_count as f64 / self.config.promotion_threshold as f64).min(2.0) / 2.0;
        let recency_score = (1.0 - recency_days / 7.0).max(0.0);
        let length_score = if pattern.agent_sequence.len() >= 2 && pattern.agent_sequence.len() <= 5
        {
            1.0
        } else {
            0.7
        };

        (frequency_score * 0.4 + recency_score * 0.3 + length_score * 0.3).min(1.0)
    }

    /// Generate a suggested workflow name
    fn generate_workflow_name(&self, pattern: &WorkflowPattern) -> String {
        if pattern.agent_sequence.is_empty() {
            return "unnamed-workflow".to_string();
        }

        let first = pattern.agent_sequence.first().unwrap();
        let last = pattern.agent_sequence.last().unwrap();

        if pattern.agent_sequence.len() == 2 {
            format!("{}-to-{}", first, last)
        } else {
            format!("{}-to-{}-{}step", first, last, pattern.agent_sequence.len())
        }
    }

    /// Clear session buffer (call at session end)
    pub fn clear_session(&self) {
        let mut buffer = self.session_buffer.lock().unwrap();
        buffer.clear();
    }

    /// Cleanup old data
    pub fn cleanup(&self, days: i64) -> Result<CleanupStats> {
        let cutoff = chrono::Utc::now().timestamp() - (days * 86400);
        let db = self.db.lock().unwrap();

        let calls_deleted = db.execute("DELETE FROM agent_calls WHERE timestamp < ?1", [cutoff])?;

        let sequences_deleted = db.execute(
            "DELETE FROM detected_sequences WHERE detected_at < ?1",
            [cutoff],
        )?;

        let patterns_deleted = db.execute(
            "DELETE FROM workflow_patterns WHERE last_called < ?1 AND promoted = 0 AND call_count < ?2",
            rusqlite::params![cutoff, self.config.promotion_threshold],
        )?;

        info!(
            "Cleanup complete: {} calls, {} sequences, {} patterns removed",
            calls_deleted, sequences_deleted, patterns_deleted
        );

        Ok(CleanupStats {
            calls_deleted,
            sequences_deleted,
            patterns_deleted,
        })
    }
}

/// Promoted workflow definition
#[derive(Debug, Clone)]
pub struct PromotedWorkflow {
    pub workflow_id: String,
    pub pattern_hash: String,
    pub name: String,
    pub description: Option<String>,
    pub agent_sequence: Vec<String>,
    pub created_at: i64,
    pub execution_count: u64,
}

/// Tracker statistics
#[derive(Debug, Clone)]
pub struct TrackerStats {
    pub total_patterns: u32,
    pub promoted_count: u32,
    pub pending_promotion: u32,
    pub total_calls: u64,
    pub total_workflow_executions: u64,
    pub promotion_threshold: u32,
}

/// Cleanup statistics
#[derive(Debug, Clone)]
pub struct CleanupStats {
    pub calls_deleted: usize,
    pub sequences_deleted: usize,
    pub patterns_deleted: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_workflow_tracker_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config = WorkflowTrackerConfig::default();
        let tracker = WorkflowTracker::new(temp_dir.path().to_path_buf(), config).await;
        assert!(tracker.is_ok());
    }

    #[tokio::test]
    async fn test_record_sequence() {
        let temp_dir = TempDir::new().unwrap();
        let config = WorkflowTrackerConfig {
            promotion_threshold: 2,
            ..Default::default()
        };
        let tracker = WorkflowTracker::new(temp_dir.path().to_path_buf(), config)
            .await
            .unwrap();

        // First call - no promotion
        let result = tracker
            .record_sequence(&["agent_a", "agent_b"], "hash1", 100)
            .unwrap();
        assert!(result.is_none());

        // Second call - should suggest promotion
        let result = tracker
            .record_sequence(&["agent_a", "agent_b"], "hash2", 150)
            .unwrap();
        assert!(result.is_some());

        let suggestion = result.unwrap();
        assert_eq!(suggestion.pattern.call_count, 2);
        assert_eq!(
            suggestion.pattern.agent_sequence,
            vec!["agent_a", "agent_b"]
        );
    }

    #[tokio::test]
    async fn test_pattern_promotion() {
        let temp_dir = TempDir::new().unwrap();
        let config = WorkflowTrackerConfig {
            promotion_threshold: 1,
            ..Default::default()
        };
        let tracker = WorkflowTracker::new(temp_dir.path().to_path_buf(), config)
            .await
            .unwrap();

        let result = tracker
            .record_sequence(&["agent_a", "agent_b", "agent_c"], "hash1", 200)
            .unwrap();

        assert!(result.is_some());
        let suggestion = result.unwrap();

        let workflow_id = tracker.promote_pattern(&suggestion.pattern).unwrap();
        assert!(workflow_id.starts_with("WF-"));

        let workflow = tracker.get_workflow(&workflow_id).unwrap();
        assert!(workflow.is_some());
    }

    #[tokio::test]
    async fn test_stats() {
        let temp_dir = TempDir::new().unwrap();
        let config = WorkflowTrackerConfig::default();
        let tracker = WorkflowTracker::new(temp_dir.path().to_path_buf(), config)
            .await
            .unwrap();

        let stats = tracker.stats().unwrap();
        assert_eq!(stats.total_patterns, 0);
        assert_eq!(stats.promotion_threshold, 3);
    }
}
