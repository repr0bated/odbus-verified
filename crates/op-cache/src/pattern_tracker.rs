//! Pattern tracking for multi-agent sequences
//!
//! Tracks frequently-used agent sequences and suggests
//! promotion to named workstacks for optimization.

use anyhow::{Context, Result};
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::info;

/// Configuration for pattern tracking
#[derive(Debug, Clone)]
pub struct PatternTrackerConfig {
    /// Minimum calls before suggesting promotion (default: 3)
    pub promotion_threshold: u32,
    /// Time window in seconds for pattern detection (default: 24 hours)
    pub detection_window_secs: i64,
    /// Enable tracking (default: true)
    pub track_enabled: bool,
}

impl Default for PatternTrackerConfig {
    fn default() -> Self {
        Self {
            promotion_threshold: 3,
            detection_window_secs: 86400,
            track_enabled: true,
        }
    }
}

/// Tracked pattern information
#[derive(Debug, Clone)]
pub struct TrackedPattern {
    pub pattern_id: String,
    pub agent_sequence: Vec<String>,
    pub call_count: u32,
    pub first_seen: i64,
    pub last_called: i64,
    pub avg_latency_ms: u64,
    pub promoted: bool,
    pub workstack_id: Option<String>,
}

impl TrackedPattern {
    pub fn sequence_description(&self) -> String {
        self.agent_sequence.join(" → ")
    }
}

/// Promotion suggestion
#[derive(Debug, Clone)]
pub struct PromotionSuggestion {
    pub pattern: TrackedPattern,
    pub estimated_time_saved_ms: u64,
    pub confidence_score: f64,
    pub suggested_name: String,
}

pub struct PatternTracker {
    db: Mutex<rusqlite::Connection>,
    config: PatternTrackerConfig,
}

impl PatternTracker {
    /// Create new pattern tracker
    pub async fn new(cache_dir: PathBuf, config: PatternTrackerConfig) -> Result<Self> {
        let db_path = cache_dir.join("patterns.db");

        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let db = rusqlite::Connection::open(&db_path)
            .context("Failed to open pattern tracker database")?;

        db.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS patterns (
                pattern_hash TEXT PRIMARY KEY,
                agent_sequence TEXT NOT NULL,
                call_count INTEGER DEFAULT 1,
                first_seen INTEGER NOT NULL,
                last_called INTEGER NOT NULL,
                total_latency_ms INTEGER DEFAULT 0,
                promoted INTEGER DEFAULT 0,
                workstack_id TEXT
            );

            CREATE TABLE IF NOT EXISTS promoted_workstacks (
                workstack_id TEXT PRIMARY KEY,
                pattern_hash TEXT NOT NULL,
                name TEXT NOT NULL,
                agent_sequence TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                execution_count INTEGER DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_patterns_count ON patterns(call_count DESC);
            CREATE INDEX IF NOT EXISTS idx_patterns_last ON patterns(last_called DESC);
            "#,
        )?;

        info!("Pattern tracker initialized at {:?}", db_path);

        Ok(Self {
            db: Mutex::new(db),
            config,
        })
    }

    /// Record an agent sequence execution
    pub fn record_sequence(
        &self,
        agents: &[&str],
        _input_hash: &str,
        total_latency_ms: u64,
    ) -> Result<Option<PromotionSuggestion>> {
        if !self.config.track_enabled || agents.len() < 2 {
            return Ok(None);
        }

        let pattern_hash = self.hash_sequence(agents);
        let agent_sequence_json = simd_json::to_string(agents)?;
        let now = chrono::Utc::now().timestamp();

        let db = self.db.lock().unwrap();

        // Check existing pattern
        let existing: Option<(u32, i64, i64, bool)> = db
            .query_row(
                "SELECT call_count, first_seen, total_latency_ms, promoted
                 FROM patterns WHERE pattern_hash = ?1",
                [&pattern_hash],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;

        let (call_count, first_seen, total_latency, promoted) = if let Some(existing) = existing {
            db.execute(
                "UPDATE patterns
                 SET call_count = call_count + 1,
                     last_called = ?1,
                     total_latency_ms = total_latency_ms + ?2
                 WHERE pattern_hash = ?3",
                rusqlite::params![now, total_latency_ms, pattern_hash],
            )?;
            (
                existing.0 + 1,
                existing.1,
                existing.2 + total_latency_ms as i64,
                existing.3,
            )
        } else {
            db.execute(
                "INSERT INTO patterns
                 (pattern_hash, agent_sequence, call_count, first_seen, last_called, total_latency_ms)
                 VALUES (?1, ?2, 1, ?3, ?3, ?4)",
                rusqlite::params![pattern_hash, agent_sequence_json, now, total_latency_ms],
            )?;
            (1, now, total_latency_ms as i64, false)
        };

        drop(db);

        // Check for promotion
        if call_count >= self.config.promotion_threshold && !promoted {
            let pattern = TrackedPattern {
                pattern_id: pattern_hash,
                agent_sequence: agents.iter().map(|s| s.to_string()).collect(),
                call_count,
                first_seen,
                last_called: now,
                avg_latency_ms: (total_latency / call_count as i64) as u64,
                promoted: false,
                workstack_id: None,
            };

            return Ok(Some(PromotionSuggestion {
                estimated_time_saved_ms: self.estimate_time_savings(&pattern),
                confidence_score: self.calculate_confidence(&pattern),
                suggested_name: self.generate_workstack_name(&pattern),
                pattern,
            }));
        }

        Ok(None)
    }

    /// Promote a pattern to a named workstack
    pub fn promote_pattern(&self, pattern: &TrackedPattern) -> Result<String> {
        let workstack_id = format!("WS-{}", &pattern.pattern_id[..8]);
        let now = chrono::Utc::now().timestamp();
        let agent_sequence_json = simd_json::to_string(&pattern.agent_sequence)?;

        let db = self.db.lock().unwrap();

        db.execute(
            "INSERT INTO promoted_workstacks
             (workstack_id, pattern_hash, name, agent_sequence, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                workstack_id,
                pattern.pattern_id,
                self.generate_workstack_name(pattern),
                agent_sequence_json,
                now
            ],
        )?;

        db.execute(
            "UPDATE patterns SET promoted = 1, workstack_id = ?1 WHERE pattern_hash = ?2",
            rusqlite::params![workstack_id, pattern.pattern_id],
        )?;

        info!(
            "Promoted pattern {} to workstack {}: {}",
            pattern.pattern_id,
            workstack_id,
            pattern.sequence_description()
        );

        Ok(workstack_id)
    }

    /// Get patterns eligible for promotion
    pub fn get_promotion_candidates(&self) -> Result<Vec<PromotionSuggestion>> {
        let db = self.db.lock().unwrap();
        let cutoff = chrono::Utc::now().timestamp() - self.config.detection_window_secs;

        let mut stmt = db.prepare(
            "SELECT pattern_hash, agent_sequence, call_count, first_seen, last_called, total_latency_ms
             FROM patterns
             WHERE call_count >= ?1 AND promoted = 0 AND last_called > ?2
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

                    Ok(TrackedPattern {
                        pattern_id: row.get(0)?,
                        agent_sequence,
                        call_count,
                        first_seen: row.get(3)?,
                        last_called: row.get(4)?,
                        avg_latency_ms: if call_count > 0 {
                            (total_latency / call_count as i64) as u64
                        } else {
                            0
                        },
                        promoted: false,
                        workstack_id: None,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(patterns
            .into_iter()
            .map(|pattern| PromotionSuggestion {
                estimated_time_saved_ms: self.estimate_time_savings(&pattern),
                confidence_score: self.calculate_confidence(&pattern),
                suggested_name: self.generate_workstack_name(&pattern),
                pattern,
            })
            .collect())
    }

    /// Get tracker statistics
    pub fn stats(&self) -> Result<TrackerStats> {
        let db = self.db.lock().unwrap();

        let total_patterns: u32 =
            db.query_row("SELECT COUNT(*) FROM patterns", [], |row| row.get(0))?;

        let promoted_count: u32 = db.query_row(
            "SELECT COUNT(*) FROM patterns WHERE promoted = 1",
            [],
            |row| row.get(0),
        )?;

        let pending_promotion: u32 = db.query_row(
            "SELECT COUNT(*) FROM patterns WHERE call_count >= ?1 AND promoted = 0",
            [self.config.promotion_threshold],
            |row| row.get(0),
        )?;

        Ok(TrackerStats {
            total_patterns,
            promoted_count,
            pending_promotion,
            promotion_threshold: self.config.promotion_threshold,
        })
    }

    fn hash_sequence(&self, agents: &[&str]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(agents.join("→").as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn estimate_time_savings(&self, pattern: &TrackedPattern) -> u64 {
        // Assume 40% cache hit rate, 60% latency reduction when cached
        let expected_future_calls = pattern.call_count * 2;
        let cache_hit_savings = (pattern.avg_latency_ms as f64 * 0.6) as u64;
        (expected_future_calls as f64 * cache_hit_savings as f64 * 0.4) as u64
    }

    fn calculate_confidence(&self, pattern: &TrackedPattern) -> f64 {
        let recency_days = (chrono::Utc::now().timestamp() - pattern.last_called) as f64 / 86400.0;
        let frequency_score =
            (pattern.call_count as f64 / self.config.promotion_threshold as f64).min(2.0) / 2.0;
        let recency_score = (1.0 - recency_days / 7.0).max(0.0);

        (frequency_score * 0.6 + recency_score * 0.4).min(1.0)
    }

    fn generate_workstack_name(&self, pattern: &TrackedPattern) -> String {
        if pattern.agent_sequence.is_empty() {
            return "unnamed-workstack".to_string();
        }

        let first = pattern.agent_sequence.first().unwrap();
        let last = pattern.agent_sequence.last().unwrap();

        if pattern.agent_sequence.len() == 2 {
            format!("{}-to-{}", first, last)
        } else {
            format!("{}-to-{}-{}step", first, last, pattern.agent_sequence.len())
        }
    }

    /// Cleanup old patterns
    pub fn cleanup(&self, days: i64) -> Result<usize> {
        let cutoff = chrono::Utc::now().timestamp() - (days * 86400);
        let db = self.db.lock().unwrap();

        let deleted = db.execute(
            "DELETE FROM patterns WHERE last_called < ?1 AND promoted = 0 AND call_count < ?2",
            rusqlite::params![cutoff, self.config.promotion_threshold],
        )?;

        info!("Cleaned up {} old patterns", deleted);
        Ok(deleted)
    }
}

/// Tracker statistics
#[derive(Debug, Clone)]
pub struct TrackerStats {
    pub total_patterns: u32,
    pub promoted_count: u32,
    pub pending_promotion: u32,
    pub promotion_threshold: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_pattern_tracker_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config = PatternTrackerConfig::default();
        let tracker = PatternTracker::new(temp_dir.path().to_path_buf(), config).await;
        assert!(tracker.is_ok());
    }

    #[tokio::test]
    async fn test_record_sequence() {
        let temp_dir = TempDir::new().unwrap();
        let config = PatternTrackerConfig {
            promotion_threshold: 2,
            ..Default::default()
        };
        let tracker = PatternTracker::new(temp_dir.path().to_path_buf(), config)
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
    }

    #[tokio::test]
    async fn test_promotion() {
        let temp_dir = TempDir::new().unwrap();
        let config = PatternTrackerConfig {
            promotion_threshold: 1,
            ..Default::default()
        };
        let tracker = PatternTracker::new(temp_dir.path().to_path_buf(), config)
            .await
            .unwrap();

        let result = tracker
            .record_sequence(&["a", "b", "c"], "hash1", 200)
            .unwrap();

        assert!(result.is_some());
        let suggestion = result.unwrap();

        let workstack_id = tracker.promote_pattern(&suggestion.pattern).unwrap();
        assert!(workstack_id.starts_with("WS-"));
    }
}
