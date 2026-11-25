use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use anyhow::{Result, ensure};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, broadcast};
use crate::{ExecutionContext, ExecutionProof, ExecutionRecord, ExecutionStatus};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionStats {
    pub total_executions: u64,
    pub successful_executions: u64,
    pub failed_executions: u64,
    pub total_duration_ms: u64,
    pub executions_by_tool: HashMap<String, u64>,
    pub failures_by_tool: HashMap<String, u64>,
    /// Preadmission request decisions, not admitted native executions.
    pub denied_requests: u64,
    pub cancelled_requests: u64,
    pub failed_before_effect_executions: u64,
}
impl ExecutionStats {
    pub fn average_duration_ms(&self) -> f64 {
        if self.total_executions == 0 { 0.0 }
        else { self.total_duration_ms as f64 / self.total_executions as f64 }
    }
    pub fn success_rate(&self) -> f64 {
        if self.total_executions == 0 { 0.0 }
        else { self.successful_executions as f64 * 100.0 / self.total_executions as f64 }
    }
}

/// A broadcast is a wakeup only. It never leaks another scope's identifiers,
/// arguments, errors or results; recover pages from the core durable cursor.
#[derive(Clone, Debug)]
pub enum ExecutionEvent { Changed }

#[derive(Default)]
struct TrackerState {
    records: HashMap<String, ExecutionRecord>,
    terminal_order: VecDeque<String>,
    /// Complete core-owned cumulative snapshots, keyed by exact executor scope.
    /// Individual completion/recovery messages never increment counters.
    stats: HashMap<(Option<String>, Option<String>), (u64, ExecutionStats)>,
}

#[derive(Clone)]
pub struct ExecutionTracker {
    state: Arc<RwLock<TrackerState>>,
    capacity: usize,
    events: broadcast::Sender<ExecutionEvent>,
}

fn same_scope(left: &ExecutionContext, right: &ExecutionContext) -> bool {
    left.executor_principal_id == right.executor_principal_id
        && left.executor_session_id == right.executor_session_id
}

impl ExecutionTracker {
    pub fn new(capacity: usize) -> Self {
        let (events, _) = broadcast::channel(64);
        Self { state: Arc::new(RwLock::new(TrackerState::default())), capacity, events }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ExecutionEvent> { self.events.subscribe() }

    /// Atomically reserve space before admission. Only proved terminal history
    /// can leave memory; active, interrupted, review and unknown references stay.
    pub async fn reserve(&self, context: ExecutionContext) -> Result<ExecutionRecord> {
        let record = ExecutionRecord::new(context)?;
        let mut state = self.state.write().await;
        ensure!(!state.records.contains_key(&record.context.operation_id), "operation already tracked; recover instead of replay");
        while state.records.len() >= self.capacity {
            let Some(id) = state.terminal_order.pop_front() else {
                anyhow::bail!("execution tracker capacity exhausted by active or unresolved operations");
            };
            state.records.remove(&id);
        }
        state.records.insert(record.context.operation_id.clone(), record.clone());
        let _ = self.events.send(ExecutionEvent::Changed);
        Ok(record)
    }

    /// Update the actual stored record. Payloads and arbitrary error strings
    /// deliberately have no representation in this API.
    pub async fn finish(&self, operation_id: &str, status: ExecutionStatus,
        proof: Option<ExecutionProof>) -> Result<()> {
        let mut state = self.state.write().await;
        let previous = state.records.get(operation_id).ok_or_else(|| anyhow::anyhow!("tracked operation absent"))?;
        if previous.status.is_terminal() {
            ensure!(previous.status == status && previous.proof == proof, "terminal execution is immutable");
            return Ok(());
        }
        validate_transition(previous, status, proof.as_ref())?;
        let record = state.records.get_mut(operation_id).expect("checked record");
        record.status = status;
        record.updated_at = Utc::now();
        record.duration_ms = (record.updated_at - record.created_at).num_milliseconds().max(0) as u64;
        record.proof = proof;
        if status.is_terminal() {
            state.terminal_order.push_back(operation_id.to_owned());
        }
        let _ = self.events.send(ExecutionEvent::Changed);
        Ok(())
    }

    /// Reconcile a core-owned event page. A cache miss never creates an effect
    /// attempt, and a cursor cannot replace an existing terminal outcome.
    pub async fn apply_committed(&self, record: ExecutionRecord) -> Result<()> {
        record.context.validate()?;
        validate_transition(&ExecutionRecord::new(record.context.clone())?, record.status, record.proof.as_ref())?;
        ensure!(record.cursor.is_some_and(|cursor| cursor > 0), "durable execution cursor missing");
        let mut state = self.state.write().await;
        let id = &record.context.operation_id;
        if let Some(previous) = state.records.get(id) {
            ensure!(previous.context == record.context, "execution attribution changed");
            if previous.cursor >= record.cursor { return Ok(()); }
            if previous.status.is_terminal() {
                ensure!(previous.status == record.status && previous.proof == record.proof,
                    "committed terminal execution changed");
            } else {
                validate_transition(previous, record.status, record.proof.as_ref())?;
            }
        } else {
            while state.records.len() >= self.capacity {
                let Some(evicted) = state.terminal_order.pop_front() else {
                    anyhow::bail!("execution tracker capacity exhausted; page durable history without dropping active references");
                };
                state.records.remove(&evicted);
            }
        }
        if record.status.is_terminal() && !state.terminal_order.contains(id) {
            state.terminal_order.push_back(id.clone());
        }
        state.records.insert(id.clone(), record);
        let _ = self.events.send(ExecutionEvent::Changed);
        Ok(())
    }

    pub async fn get_execution(&self, scope: &ExecutionContext, id: &str) -> Option<ExecutionRecord> {
        self.state.read().await.records.get(id).filter(|record| same_scope(&record.context, scope)).cloned()
    }

    pub async fn snapshot(&self, scope: &ExecutionContext, limit: usize) -> Vec<ExecutionRecord> {
        let state = self.state.read().await;
        let mut records: Vec<_> = state.records.values()
            .filter(|record| same_scope(&record.context, scope)).cloned().collect();
        records.sort_unstable_by(|left, right| right.updated_at.cmp(&left.updated_at)
            .then_with(|| left.context.operation_id.cmp(&right.context.operation_id)));
        records.truncate(limit.min(self.capacity));
        records
    }

    pub async fn recent_completed(&self, scope: &ExecutionContext, limit: usize) -> Vec<ExecutionRecord> {
        let state = self.state.read().await;
        state.terminal_order.iter().rev().filter_map(|id| state.records.get(id))
            .filter(|record| record.status.is_terminal() && same_scope(&record.context, scope))
            .take(limit.min(self.capacity)).cloned().collect()
    }

    pub async fn get_stats(&self, scope: &ExecutionContext) -> Option<(u64, ExecutionStats)> {
        self.state.read().await.stats
            .get(&(scope.executor_principal_id.clone(), scope.executor_session_id.clone()))
            .cloned()
    }

    /// `through_cursor` covers the COMPLETE ordered core fold, not one event.
    /// Ignoring an older aggregate cannot lose unseen lower events.
    pub async fn materialize_stats(&self, scope: &ExecutionContext, through_cursor: u64, stats: ExecutionStats) {
        let mut state = self.state.write().await;
        let key = (scope.executor_principal_id.clone(), scope.executor_session_id.clone());
        if state.stats.get(&key).is_none_or(|(cursor, _)| *cursor < through_cursor) {
            state.stats.insert(key, (through_cursor, stats));
        }
    }
}

fn validate_transition(previous: &ExecutionRecord, status: ExecutionStatus, proof: Option<&ExecutionProof>) -> Result<()> {
    if let Some(proof) = proof { proof.validate(&previous.context.operation_id)?; }
    if matches!(status, ExecutionStatus::ReceiptCompleted | ExecutionStatus::Completed | ExecutionStatus::DomainFailed | ExecutionStatus::FailedBeforeEffect) {
        ensure!(proof.is_some_and(|proof| proof.terminal.is_some()), "terminal execution requires its original core proof");
    }
    if matches!(status, ExecutionStatus::Admitted | ExecutionStatus::Running) {
        ensure!(proof.is_some(), "admitted execution requires durable attempt proof");
    }
    if matches!(status, ExecutionStatus::Denied | ExecutionStatus::Cancelled) {
        ensure!(previous.proof.is_none() && proof.is_none(), "possible dispatched effect cannot become a no-effect denial or cancellation");
    }
    if previous.proof.is_some() {
        ensure!(!matches!(status, ExecutionStatus::Proposed | ExecutionStatus::Checking | ExecutionStatus::ReadyForAdmission),
            "an admitted attempt cannot return to pre-dispatch scheduling");
        ensure!(proof.is_some(), "execution transition lost its original attempt");
        ensure!(previous.proof.as_ref().map(|proof| &proof.attempt) == proof.map(|proof| &proof.attempt),
            "execution transition replaced its original attempt");
    }
    Ok(())
}


impl Default for ExecutionTracker {
    fn default() -> Self { Self::new(1024) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn unresolved_operations_are_never_evicted() {
        let tracker = ExecutionTracker::new(1);
        let context = ExecutionContext::new("plugin.read");
        let id = context.operation_id.clone();
        tracker.reserve(context.clone()).await.unwrap();
        tracker.finish(&id, ExecutionStatus::OutcomeUnknown, None).await.unwrap();
        assert!(tracker.reserve(ExecutionContext::new("plugin.other")).await.is_err());
        assert!(tracker.get_execution(&context, &id).await.is_some());
        assert!(tracker.recent_completed(&context, 10).await.is_empty());
    }
    #[tokio::test]
    async fn preadmission_observation_never_invents_native_execution_counters() {
        let tracker = ExecutionTracker::new(1);
        let context = ExecutionContext::new("plugin.read");
        let id = context.operation_id.clone();
        tracker.reserve(context.clone()).await.unwrap();
        tracker.finish(&id, ExecutionStatus::Denied, None).await.unwrap();
        tracker.finish(&id, ExecutionStatus::Denied, None).await.unwrap();
        assert!(tracker.get_stats(&context).await.is_none());
        assert_eq!(tracker.recent_completed(&context, 10).await.len(), 1);
    }

    #[tokio::test]
    async fn out_of_order_complete_aggregate_snapshots_do_not_double_count_cache_recovery() {
        let tracker = ExecutionTracker::new(1);
        let first = ExecutionContext::new("plugin.first");
        tracker.reserve(first.clone()).await.unwrap();
        tracker.finish(&first.operation_id, ExecutionStatus::Denied, None).await.unwrap();
        let mut recovered = tracker.get_execution(&first, &first.operation_id).await.unwrap();
        recovered.cursor = Some(10);
        let second = ExecutionContext::new("plugin.second");
        tracker.reserve(second.clone()).await.unwrap();
        tracker.finish(&second.operation_id, ExecutionStatus::Denied, None).await.unwrap();
        tracker.materialize_stats(&first, 20, ExecutionStats { total_executions: 1, ..Default::default() }).await;
        tracker.apply_committed(recovered).await.unwrap();
        tracker.materialize_stats(&first, 10, ExecutionStats::default()).await;
        assert_eq!(tracker.get_stats(&first).await.unwrap().1.total_executions, 1);
    }

    #[tokio::test]
    async fn method_receipt_is_not_final_response_success() {
        let tracker = ExecutionTracker::new(1);
        let context = ExecutionContext::new("plugin.method");
        tracker.reserve(context.clone()).await.unwrap();
        let proof = ExecutionProof {
            operation_id: context.operation_id.clone(),
            attempt: op_state_store::method_receipt::MethodEventProof { event_id: 1, event_hash: "a".repeat(64) },
            terminal: Some(op_state_store::method_receipt::MethodEventProof { event_id: 2, event_hash: "b".repeat(64) }),
        };
        tracker.finish(&context.operation_id, ExecutionStatus::ReceiptCompleted, Some(proof.clone())).await.unwrap();
        tracker.finish(&context.operation_id, ExecutionStatus::NeedsReview, Some(proof)).await.unwrap();
        assert!(tracker.recent_completed(&context, 10).await.is_empty());
        assert!(tracker.get_stats(&context).await.is_none());
    }
}
