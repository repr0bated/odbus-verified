//! Routing-owner admission budgets. These account retained projection payloads
//! and in-flight work; they are not a disk-space probe or an audit sink.
use std::{collections::BTreeMap, sync::{Arc, Mutex}};

use anyhow::{Context, Result, ensure};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RoutingLimits {
    pub global_active: usize,
    pub owner_active: usize,
    pub global_queued: usize,
    pub owner_queued: usize,
    pub global_bytes: usize,
    pub owner_bytes: usize,
    pub global_audit_bytes: usize,
    pub owner_audit_bytes: usize,
    pub max_owners: usize,
    pub max_document_bytes: usize,
    pub max_records: usize,
    pub max_tags: usize,
    pub max_policies: usize,
    pub max_markers: usize,
    pub max_page_size: usize,
    pub max_evidence_fanout: usize,
    pub queue_wait_ms: u64,
    /// None retains bounded authoritative history. Expiry requires a separate
    /// reviewed durable retention operation; this implementation refuses it.
    pub record_expiry_seconds: Option<u64>,
}

impl Default for RoutingLimits {
    fn default() -> Self {
        Self {
            global_active: 32, owner_active: 4,
            global_queued: 64, owner_queued: 8,
            global_bytes: 64 * 1024 * 1024, owner_bytes: 16 * 1024 * 1024,
            global_audit_bytes: 64 * 1024 * 1024, owner_audit_bytes: 16 * 1024 * 1024,
            max_owners: 256, max_document_bytes: 1024 * 1024,
            max_records: 256, max_tags: 256, max_policies: 256,
            max_markers: 1024, max_page_size: 100, max_evidence_fanout: 64,
            queue_wait_ms: 10_000, record_expiry_seconds: None,
        }
    }
}

#[derive(Default)]
struct Usage { active: usize, queued: usize, bytes: usize, audit: usize, retained: usize }
#[derive(Default)]
struct Accounting {
    active: usize, queued: usize, bytes: usize, audit: usize,
    owners: BTreeMap<String, Usage>, retained_documents: BTreeMap<(String, String), usize>,
}

pub(crate) struct RoutingResources {
    limits: RoutingLimits,
    accounting: Mutex<Accounting>,
}

impl RoutingResources {
    pub fn new(limits: RoutingLimits) -> Result<Arc<Self>> {
        ensure!(limits.global_active > 0 && limits.owner_active > 0 && limits.max_owners > 0
            && limits.global_queued > 0 && limits.owner_queued > 0,
            "routing resource counts must be positive");
        ensure!(limits.max_document_bytes > 0 && limits.max_records > 0
            && limits.max_tags > 0 && limits.max_policies > 0 && limits.max_markers > 0
            && limits.max_page_size > 0 && limits.max_page_size <= 100
            && limits.max_evidence_fanout > 0 && limits.max_evidence_fanout <= 64
            && limits.queue_wait_ms > 0, "invalid routing document limits");
        ensure!(limits.record_expiry_seconds.is_none(), "routing_record_expiry_unavailable; retain bounded durable history without automatic deletion");
        ensure!(limits.owner_active <= limits.global_active
            && limits.owner_queued <= limits.global_queued
            && limits.owner_bytes <= limits.global_bytes
            && limits.owner_audit_bytes <= limits.global_audit_bytes,
            "routing owner budget exceeds global budget");
        // Reserve for bounded input, native CAS copies, state validation and
        // publication bytes, in addition to the retained committed document.
        let work = limits.max_document_bytes.checked_mul(6).context("routing byte limit overflow")?;
        ensure!(work.checked_add(limits.max_document_bytes).is_some_and(|n| n <= limits.owner_bytes),
            "routing owner budget cannot hold one document transition");
        let audit = limits.max_document_bytes.checked_mul(2).and_then(|n| n.checked_add(AUDIT_WORK_BYTES))
            .context("routing audit limit overflow")?;
        ensure!(limits.owner_audit_bytes >= audit, "routing audit budget too small");
        Ok(Arc::new(Self { limits, accounting: Mutex::new(Accounting::default()) }))
    }

    pub fn limits(&self) -> &RoutingLimits { &self.limits }

    /// Input/result payload ceilings plus bounded linked audit metadata.
    pub fn audit_work_bytes(&self) -> usize {
        // Checked by new(); the immutable configuration cannot change later.
        self.limits.max_document_bytes * 2 + AUDIT_WORK_BYTES
    }

    /// Fail before queueing or persistence. All capacity is retained by the
    /// returned token until owned worker cleanup; no waiters escape this cap.
    pub fn reserve(self: &Arc<Self>, owner: &str, bytes: usize, audit: usize) -> Result<RoutingReservation> {
        ensure!(!owner.is_empty() && owner.len() <= 192, "invalid routing budget owner");
        let mut state = self.accounting.lock().map_err(|_| anyhow::anyhow!("routing budget poisoned"))?;
        let existing = state.owners.get(owner);
        ensure!(existing.is_some() || state.owners.len() < self.limits.max_owners, "routing_owner_capacity");
        let owner_queued = existing.map_or(0, |usage| usage.queued);
        let owner_bytes = existing.map_or(0, |usage| usage.bytes);
        let owner_audit = existing.map_or(0, |usage| usage.audit);
        let total_bytes = state.bytes.checked_add(bytes).context("routing byte overflow")?;
        let total_audit = state.audit.checked_add(audit).context("routing audit overflow")?;
        ensure!(state.queued < self.limits.global_queued && owner_queued < self.limits.owner_queued,
            "routing_queue_capacity");
        ensure!(total_bytes <= self.limits.global_bytes
            && owner_bytes.checked_add(bytes).is_some_and(|n| n <= self.limits.owner_bytes),
            "routing_byte_capacity");
        ensure!(total_audit <= self.limits.global_audit_bytes
            && owner_audit.checked_add(audit).is_some_and(|n| n <= self.limits.owner_audit_bytes),
            "routing_audit_capacity");
        state.queued += 1; state.bytes = total_bytes; state.audit = total_audit;
        let usage = state.owners.entry(owner.to_owned()).or_default();
        usage.queued += 1; usage.bytes += bytes; usage.audit += audit;
        Ok(RoutingReservation { resources: self.clone(), owner: owner.into(), bytes, audit, active: false })
    }
}

/// Extra work space for linked attempt/terminal metadata, in addition to the
/// bounded input/result payloads. Canonical MutationEngine
/// must still confirm its real durable audit sink before any backend effect.
pub(crate) const AUDIT_WORK_BYTES: usize = 32 * 1024;

pub(crate) struct RoutingReservation {
    resources: Arc<RoutingResources>, owner: String, bytes: usize, audit: usize, active: bool,
}

impl RoutingReservation {
    pub fn require_owner(&self, resources: &Arc<RoutingResources>, owner: &str, bytes: usize, audit: usize) -> Result<()> {
        ensure!(Arc::ptr_eq(&self.resources, resources) && self.owner == owner && !self.active
            && self.bytes >= bytes && self.audit >= audit, "routing input reservation differs from operation scope/bounds");
        Ok(())
    }

    pub fn begin_work(&mut self) -> Result<()> {
        ensure!(!self.active, "routing work already admitted");
        let mut state = self.resources.accounting.lock().map_err(|_| anyhow::anyhow!("routing budget poisoned"))?;
        let usage = state.owners.get(&self.owner).context("routing budget owner absent")?;
        ensure!(state.active < self.resources.limits.global_active && usage.active < self.resources.limits.owner_active,
            "routing_active_capacity");
        state.queued -= 1; state.active += 1;
        let usage = state.owners.get_mut(&self.owner).expect("active owner checked");
        usage.queued -= 1; usage.active += 1; self.active = true;
        Ok(())
    }

    /// Account a reconstructed/committed private projection separately from
    /// transient work. The retained charge survives token drop. It describes
    /// observed projections in this process, not all historical Cozo rows.
    /// Keep a high-water charge: a failed replacement can leave the previous
    /// bytes in SHM, so preparing a smaller candidate must not release them.
    pub fn retain_document(&mut self, document: &str, bytes: usize) -> Result<()> {
        ensure!(bytes <= self.resources.limits.max_document_bytes, "routing_document_capacity");
        ensure!(!document.is_empty() && document.len() <= 192, "invalid routing retained document key");
        let mut state = self.resources.accounting.lock().map_err(|_| anyhow::anyhow!("routing budget poisoned"))?;
        let key = (self.owner.clone(), document.to_owned());
        let previous = state.retained_documents.get(&key).copied().unwrap_or(0);
        let bytes = bytes.max(previous);
        ensure!(state.retained_documents.contains_key(&key) || state.retained_documents.len() < self.resources.limits.max_owners,
            "routing_retained_document_capacity");
        let usage = state.owners.get(&self.owner).context("routing budget owner absent")?;
        let global = state.bytes - previous;
        let owner = usage.bytes - previous;
        ensure!(global.checked_add(bytes).is_some_and(|n| n <= self.resources.limits.global_bytes)
            && owner.checked_add(bytes).is_some_and(|n| n <= self.resources.limits.owner_bytes),
            "routing_retained_capacity");
        state.bytes = global + bytes;
        let usage = state.owners.get_mut(&self.owner).expect("retained owner checked");
        usage.bytes = owner + bytes; usage.retained = usage.retained - previous + bytes;
        if bytes == 0 { state.retained_documents.remove(&key); } else { state.retained_documents.insert(key, bytes); }
        Ok(())
    }
}

impl Drop for RoutingReservation {
    fn drop(&mut self) {
        // Poisoning is fail-closed for admission; cleanup still releases owned
        // transient charges instead of leaking them after an unrelated panic.
        let mut state = self.resources.accounting.lock().unwrap_or_else(|p| p.into_inner());
        if self.active { state.active -= 1; } else { state.queued -= 1; }
        state.bytes -= self.bytes; state.audit -= self.audit;
        if let Some(usage) = state.owners.get_mut(&self.owner) {
            if self.active { usage.active -= 1; } else { usage.queued -= 1; }
            usage.bytes -= self.bytes; usage.audit -= self.audit;
            if usage.active == 0 && usage.queued == 0 && usage.retained == 0 { state.owners.remove(&self.owner); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owner_admission_and_retained_bytes_survive_worker_token_lifetime() {
        let resources = RoutingResources::new(RoutingLimits { owner_active: 1, ..Default::default() }).unwrap();
        let mut first = resources.reserve("owner", 1024, AUDIT_WORK_BYTES).unwrap();
        first.begin_work().unwrap();
        let mut queued = resources.reserve("owner", 1, 1).unwrap();
        assert!(queued.begin_work().is_err()); drop(queued);
        first.retain_document("first-session", 512).unwrap();
        first.retain_document("second-session", 128).unwrap();
        drop(first);
        let state = resources.accounting.lock().unwrap();
        assert_eq!(state.active, 0); assert_eq!(state.bytes, 640); assert_eq!(state.audit, 0);
        drop(state);
        assert!(resources.reserve("owner", 1024, AUDIT_WORK_BYTES).is_ok());
    }
    #[test]
    fn rejected_budget_does_not_consume_an_owner_slot() {
        let resources = RoutingResources::new(RoutingLimits { max_owners: 1, ..Default::default() }).unwrap();
        assert!(resources.reserve("failed", usize::MAX, 0).is_err());
        assert!(resources.reserve("accepted", 1, 1).is_ok());
    }
    #[test]
    fn configurable_limits_round_trip_and_unsupported_expiry_fails_closed() {
        let mut limits: RoutingLimits = serde_json::from_value(serde_json::to_value(RoutingLimits::default()).unwrap()).unwrap();
        limits.record_expiry_seconds = Some(60);
        assert!(RoutingResources::new(limits).is_err());
    }
}
