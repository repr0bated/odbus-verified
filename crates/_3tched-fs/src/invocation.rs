//! One transport envelope for operation-file bytes; plugin arguments remain unchanged.
use std::time::{Duration, Instant};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const APPLICATION_BUDGET: Duration = Duration::from_secs(600);
pub const ORDINARY_BUDGET: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug)]
pub struct Deadline(Instant);
impl Deadline {
    pub fn application() -> Self { Self(Instant::now() + APPLICATION_BUDGET) }
    pub fn ordinary() -> Self { Self(Instant::now() + ORDINARY_BUDGET) }
    /// Start at the beginning of an exchange, not when its reply arrives.
    /// This conservatively charges transfer time instead of renewing the budget.
    pub fn received(started: Instant, remaining_ms: u64) -> Result<Self> {
        ensure!(remaining_ms > 0 && remaining_ms <= APPLICATION_BUDGET.as_millis() as u64,
            "invalid operation time budget");
        let value = Self(started + Duration::from_millis(remaining_ms));
        value.remaining()?;
        Ok(value)
    }
    pub fn remaining(self) -> Result<Duration> {
        let remaining = self.0.checked_duration_since(Instant::now())
            .context("operation deadline elapsed; outcome may be unknown")?;
        ensure!(remaining.as_millis() > 0, "operation deadline elapsed; outcome may be unknown");
        Ok(remaining)
    }
    pub fn remaining_ms(self) -> Result<u64> { Ok(self.remaining()?.as_millis() as u64) }
    pub fn clamp(self, authority: Self) -> Self { Self(self.0.min(authority.0)) }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub enum CallFormat {
    #[serde(rename = "3tchedfs.call/1")]
    V1,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CallMetadata {
    pub source_schema_hash: String,
    /// A live admission ticket for this operation, never an identity selector.
    pub admitted_operation_id: Option<String>,
    /// Untrusted pending-turn reference. Only kernel/turn admission can bind it.
    pub response_turn_id: Option<String>,
    pub remaining_ms: u64,
}
impl CallMetadata {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.admitted_operation_id.is_none() || self.response_turn_id.is_none(),
            "ticket and response turn metadata are mutually exclusive");
        ensure!(self.source_schema_hash.len() == 64 && self.source_schema_hash.bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "invalid operation source seal");
        if let Some(id) = &self.admitted_operation_id {
            ensure!(uuid::Uuid::parse_str(id)?.to_string() == *id, "invalid admitted operation ID");
        }
        if let Some(id) = &self.response_turn_id {
            ensure!(uuid::Uuid::parse_str(id)?.to_string() == *id, "invalid response turn ID");
        }
        let cap = if self.admitted_operation_id.is_some() || self.response_turn_id.is_some() {
            APPLICATION_BUDGET
        } else { ORDINARY_BUDGET };
        ensure!(self.remaining_ms > 0 && self.remaining_ms <= cap.as_millis() as u64,
            "operation budget exceeds its transport bound");
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CallInput {
    pub format: CallFormat,
    pub transport: CallMetadata,
    /// Validated separately against the target method's sealed input schema.
    pub arguments: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_reference_never_substitutes_for_a_ticket() {
        let mut metadata = CallMetadata {
            source_schema_hash: "a".repeat(64),
            admitted_operation_id: Some("00000000-0000-4000-8000-000000000001".into()),
            response_turn_id: Some("00000000-0000-4000-8000-000000000002".into()),
            remaining_ms: 1,
        };
        assert!(metadata.validate().is_err());
        metadata.admitted_operation_id = None;
        assert!(metadata.validate().is_ok());
        metadata.response_turn_id = Some("invented-turn".into());
        assert!(metadata.validate().is_err());
    }
}
