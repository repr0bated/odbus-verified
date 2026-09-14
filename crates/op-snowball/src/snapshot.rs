//! Snapshot interval configuration

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::warn;

/// Snapshot interval options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SnapshotInterval {
    /// Snapshot after every operation
    PerOperation,
    /// Snapshot every minute
    EveryMinute,
    /// Snapshot every 5 minutes
    Every5Minutes,
    /// Snapshot every 15 minutes
    #[default]
    Every15Minutes,
    /// Snapshot every 30 minutes
    Every30Minutes,
    /// Snapshot every hour
    Hourly,
    /// Snapshot every day
    Daily,
    /// Snapshot every week
    Weekly,
}

impl SnapshotInterval {
    /// Parse from environment variable
    pub fn from_env() -> Self {
        match std::env::var("OPDBUS_SNAPSHOT_INTERVAL")
            .unwrap_or_else(|_| "every-15-minutes".to_string())
            .to_lowercase()
            .as_str()
        {
            "per-op" | "per-operation" | "per_operation" => SnapshotInterval::PerOperation,
            "every-minute" | "1-minute" | "1min" => SnapshotInterval::EveryMinute,
            "every-5-minutes" | "5-minutes" | "5min" => SnapshotInterval::Every5Minutes,
            "every-15-minutes" | "15-minutes" | "15min" => SnapshotInterval::Every15Minutes,
            "every-30-minutes" | "30-minutes" | "30min" => SnapshotInterval::Every30Minutes,
            "hourly" | "1-hour" | "1h" => SnapshotInterval::Hourly,
            "daily" | "1-day" | "1d" => SnapshotInterval::Daily,
            "weekly" | "1-week" | "1w" => SnapshotInterval::Weekly,
            _ => {
                warn!("Invalid OPDBUS_SNAPSHOT_INTERVAL, defaulting to every-15-minutes");
                SnapshotInterval::Every15Minutes
            }
        }
    }

    /// Parse from string
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "per-op" | "per-operation" | "per_operation" => Some(SnapshotInterval::PerOperation),
            "every-minute" | "1-minute" | "1min" | "minute" => Some(SnapshotInterval::EveryMinute),
            "every-5-minutes" | "5-minutes" | "5min" => Some(SnapshotInterval::Every5Minutes),
            "every-15-minutes" | "15-minutes" | "15min" => Some(SnapshotInterval::Every15Minutes),
            "every-30-minutes" | "30-minutes" | "30min" => Some(SnapshotInterval::Every30Minutes),
            "hourly" | "1-hour" | "1h" | "hour" => Some(SnapshotInterval::Hourly),
            "daily" | "1-day" | "1d" | "day" => Some(SnapshotInterval::Daily),
            "weekly" | "1-week" | "1w" | "week" => Some(SnapshotInterval::Weekly),
            _ => None,
        }
    }

    /// Get the duration for this interval
    /// Returns None for PerOperation (snapshot on every change)
    pub fn as_duration(&self) -> Option<Duration> {
        match self {
            SnapshotInterval::PerOperation => None,
            SnapshotInterval::EveryMinute => Some(Duration::from_secs(60)),
            SnapshotInterval::Every5Minutes => Some(Duration::from_secs(5 * 60)),
            SnapshotInterval::Every15Minutes => Some(Duration::from_secs(15 * 60)),
            SnapshotInterval::Every30Minutes => Some(Duration::from_secs(30 * 60)),
            SnapshotInterval::Hourly => Some(Duration::from_secs(60 * 60)),
            SnapshotInterval::Daily => Some(Duration::from_secs(24 * 60 * 60)),
            SnapshotInterval::Weekly => Some(Duration::from_secs(7 * 24 * 60 * 60)),
        }
    }

    /// Check if enough time has passed since the last snapshot
    pub fn should_snapshot(&self, elapsed: Duration) -> bool {
        match self.as_duration() {
            None => true, // PerOperation always snapshots
            Some(interval) => elapsed >= interval,
        }
    }

    /// Human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            SnapshotInterval::PerOperation => "after every operation",
            SnapshotInterval::EveryMinute => "every minute",
            SnapshotInterval::Every5Minutes => "every 5 minutes",
            SnapshotInterval::Every15Minutes => "every 15 minutes",
            SnapshotInterval::Every30Minutes => "every 30 minutes",
            SnapshotInterval::Hourly => "every hour",
            SnapshotInterval::Daily => "every day",
            SnapshotInterval::Weekly => "every week",
        }
    }
}

impl std::fmt::Display for SnapshotInterval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duration() {
        assert!(SnapshotInterval::PerOperation.as_duration().is_none());
        assert_eq!(
            SnapshotInterval::EveryMinute.as_duration(),
            Some(Duration::from_secs(60))
        );
        assert_eq!(
            SnapshotInterval::Hourly.as_duration(),
            Some(Duration::from_secs(3600))
        );
    }

    #[test]
    fn test_from_str() {
        assert_eq!(
            SnapshotInterval::parse("hourly"),
            Some(SnapshotInterval::Hourly)
        );
        assert_eq!(
            SnapshotInterval::parse("15min"),
            Some(SnapshotInterval::Every15Minutes)
        );
        assert_eq!(SnapshotInterval::parse("invalid"), None);
    }
}
