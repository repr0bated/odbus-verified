//! Partitioned_Store (spec task 7.3): opaque handles, memory only.
//! A handle is created only by `open` with the bind-receipt principal;
//! code holding no handle cannot address a partition. A restart drops
//! everything, including pending calls.

use crate::types::{ChatSessionId, PrincipalId, SecurityEvent};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartitionError {
    pub message: String,
    pub security_event: SecurityEvent,
}
impl std::fmt::Display for PartitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for PartitionError {}

/// Opaque: fields are private and there is no constructor outside `open`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PartitionHandle {
    id: u64,
    principal_id: PrincipalId,
}

impl PartitionHandle {
    pub fn principal_id(&self) -> &PrincipalId {
        &self.principal_id
    }
}

#[derive(Default)]
struct Partition {
    entries: HashMap<(String, String), serde_json::Value>,
    by_session: HashMap<String, HashSet<(String, String)>>,
    active_sessions: HashSet<String>,
}

#[derive(Default)]
pub struct PartitionedStore {
    next_id: u64,
    partitions: HashMap<u64, Partition>,
}

impl PartitionedStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(&mut self, principal_id: PrincipalId) -> PartitionHandle {
        self.next_id += 1;
        let handle = PartitionHandle {
            id: self.next_id,
            principal_id,
        };
        self.partitions.insert(handle.id, Partition::default());
        handle
    }

    pub fn begin_session(&mut self, handle: &PartitionHandle, session: &ChatSessionId) {
        if let Some(p) = self.partitions.get_mut(&handle.id) {
            p.active_sessions.insert(session.to_string());
        }
    }

    fn partition_mut(
        &mut self,
        handle: &PartitionHandle,
        session: &ChatSessionId,
        at: &str,
    ) -> Result<&mut Partition, PartitionError> {
        let err = |store: &Self| PartitionError {
            message: "partition unavailable".to_string(),
            security_event: SecurityEvent {
                at: at.to_string(),
                kind: "partition-error".to_string(),
                channel: "partitioned-store".to_string(),
                credential_class: None,
            },
        };
        match self.partitions.get_mut(&handle.id) {
            Some(p) if p.active_sessions.contains(session.as_str()) => Ok(p),
            _ => Err(err(self)),
        }
    }

    pub fn put(
        &mut self,
        handle: &PartitionHandle,
        session: &ChatSessionId,
        kind: &str,
        id: &str,
        value: serde_json::Value,
        at: &str,
    ) -> Result<(), PartitionError> {
        let p = self.partition_mut(handle, session, at)?;
        let key = (kind.to_string(), id.to_string());
        p.by_session
            .entry(session.to_string())
            .or_default()
            .insert(key.clone());
        p.entries.insert(key, value);
        Ok(())
    }

    pub fn get(
        &mut self,
        handle: &PartitionHandle,
        session: &ChatSessionId,
        kind: &str,
        id: &str,
        at: &str,
    ) -> Result<Option<serde_json::Value>, PartitionError> {
        let p = self.partition_mut(handle, session, at)?;
        Ok(p.entries.get(&(kind.to_string(), id.to_string())).cloned())
    }

    /// Removes that session's entries only.
    pub fn drop_session(&mut self, handle: &PartitionHandle, session: &ChatSessionId) {
        if let Some(p) = self.partitions.get_mut(&handle.id) {
            if let Some(keys) = p.by_session.remove(session.as_str()) {
                for key in keys {
                    p.entries.remove(&key);
                }
            }
            p.active_sessions.remove(session.as_str());
        }
    }
}
