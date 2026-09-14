//! Snapshot retention policies with rolling windows

use anyhow::Result;
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;

/// Snapshot retention policy with rolling windows
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// Keep last N hourly snapshots
    pub hourly: usize,
    /// Keep last N daily snapshots
    pub daily: usize,
    /// Keep last N weekly snapshots
    pub weekly: usize,
    /// Keep last N quarterly snapshots
    pub quarterly: usize,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            hourly: 5,
            daily: 5,
            weekly: 5,
            quarterly: 5,
        }
    }
}

impl RetentionPolicy {
    /// Create a new retention policy with explicit values
    pub fn new(hourly: usize, daily: usize, weekly: usize, quarterly: usize) -> Self {
        Self {
            hourly,
            daily,
            weekly,
            quarterly,
        }
    }

    /// Create a minimal retention policy (for testing)
    pub fn minimal() -> Self {
        Self {
            hourly: 2,
            daily: 2,
            weekly: 2,
            quarterly: 2,
        }
    }

    /// Create a comprehensive retention policy (for production)
    pub fn comprehensive() -> Self {
        Self {
            hourly: 24,
            daily: 30,
            weekly: 12,
            quarterly: 8,
        }
    }

    /// Parse from environment variables or use defaults
    pub fn from_env() -> Self {
        Self {
            hourly: std::env::var("OPDBUS_RETAIN_HOURLY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5),
            daily: std::env::var("OPDBUS_RETAIN_DAILY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5),
            weekly: std::env::var("OPDBUS_RETAIN_WEEKLY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5),
            quarterly: std::env::var("OPDBUS_RETAIN_QUARTERLY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5),
        }
    }

    /// Load from JSON value (for config files)
    pub fn from_json(value: &simd_json::OwnedValue) -> Result<Self> {
        Ok(Self {
            hourly: value.get("hourly").and_then(|v| v.as_u64()).unwrap_or(5) as usize,
            daily: value.get("daily").and_then(|v| v.as_u64()).unwrap_or(5) as usize,
            weekly: value.get("weekly").and_then(|v| v.as_u64()).unwrap_or(5) as usize,
            quarterly: value.get("quarterly").and_then(|v| v.as_u64()).unwrap_or(5) as usize,
        })
    }

    /// Total maximum snapshots that could be retained
    pub fn max_snapshots(&self) -> usize {
        self.hourly + self.daily + self.weekly + self.quarterly
    }

    /// Builder-style methods
    pub fn with_hourly(mut self, count: usize) -> Self {
        self.hourly = count;
        self
    }

    pub fn with_daily(mut self, count: usize) -> Self {
        self.daily = count;
        self
    }

    pub fn with_weekly(mut self, count: usize) -> Self {
        self.weekly = count;
        self
    }

    pub fn with_quarterly(mut self, count: usize) -> Self {
        self.quarterly = count;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_policy() {
        let policy = RetentionPolicy::default();
        assert_eq!(policy.hourly, 5);
        assert_eq!(policy.daily, 5);
        assert_eq!(policy.weekly, 5);
        assert_eq!(policy.quarterly, 5);
    }

    #[test]
    fn test_from_json() {
        let json = simd_json::json!({
            "hourly": 10,
            "daily": 7,
            "weekly": 4,
            "quarterly": 2
        });

        let policy = RetentionPolicy::from_json(&json).unwrap();
        assert_eq!(policy.hourly, 10);
        assert_eq!(policy.daily, 7);
    }
}
