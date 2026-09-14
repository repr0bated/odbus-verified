//! Pure in-memory StateStore — no SQLite, no drift.
//!
//! Used for plugin projection bootstrap and ephemeral state tracking
//! where persistence is handled externally (SHM, snowball, JSON files).

use crate::error::Result;
use crate::execution_job::ExecutionJob;
use crate::state_store::{StateStore, ToolRecord};
use crate::{CanonicalDbExport, StoredObject};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;
use tracing::{debug, info};
use uuid::Uuid;

/// Thread-safe in-memory state store.
pub struct MemoryStore {
    jobs: Mutex<HashMap<Uuid, ExecutionJob>>,
    objects: Mutex<HashMap<String, StoredObject>>,
    tools: Mutex<Vec<ToolRecord>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        info!("Initialized MemoryStore — no persistent SQLite");
        Self {
            jobs: Mutex::new(HashMap::new()),
            objects: Mutex::new(HashMap::new()),
            tools: Mutex::new(Vec::new()),
        }
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StateStore for MemoryStore {
    async fn save_job(&self, job: &ExecutionJob) -> Result<()> {
        let mut jobs = self.jobs.lock().expect("mutex should not be poisoned");
        jobs.insert(job.id, job.clone());
        debug!("Saved job {} ({}) to memory", job.id, job.tool_name);
        Ok(())
    }

    async fn get_job(&self, id: Uuid) -> Result<Option<ExecutionJob>> {
        let jobs = self.jobs.lock().expect("mutex should not be poisoned");
        Ok(jobs.get(&id).cloned())
    }

    async fn update_job(&self, job: &ExecutionJob) -> Result<()> {
        let mut jobs = self.jobs.lock().expect("mutex should not be poisoned");
        jobs.insert(job.id, job.clone());
        debug!("Updated job {} in memory", job.id);
        Ok(())
    }

    async fn get_object(&self, id: &str) -> Result<Option<StoredObject>> {
        let objects = self.objects.lock().expect("mutex should not be poisoned");
        Ok(objects.get(id).cloned())
    }

    async fn upsert_object(
        &self,
        id: &str,
        object_type: &str,
        namespace: &str,
        data: &simd_json::OwnedValue,
    ) -> Result<()> {
        let mut objects = self.objects.lock().expect("mutex should not be poisoned");
        objects.insert(
            id.to_string(),
            StoredObject {
                id: id.to_string(),
                object_type: object_type.to_string(),
                namespace: namespace.to_string(),
                data: data.clone(),
            },
        );
        debug!("Upserted object {} in memory", id);
        Ok(())
    }

    async fn export_canonical(&self) -> Result<CanonicalDbExport> {
        let objects = self.objects.lock().expect("mutex should not be poisoned");
        let jobs = self.jobs.lock().expect("mutex should not be poisoned");

        let mut executions = Vec::new();
        for job in jobs.values() {
            let mut bytes = simd_json::to_string(job)?.into_bytes();
            let value = simd_json::to_owned_value(&mut bytes)?;
            executions.push(value);
        }

        Ok(CanonicalDbExport {
            objects: objects.values().cloned().collect(),
            executions,
            snowball: Vec::new(),
        })
    }

    async fn save_tools(&self, tools: Vec<ToolRecord>) -> Result<()> {
        let mut stored = self.tools.lock().expect("mutex should not be poisoned");
        *stored = tools;
        debug!("Saved {} tools to memory", stored.len());
        Ok(())
    }

    async fn load_tools(&self) -> Result<Vec<ToolRecord>> {
        let tools = self.tools.lock().expect("mutex should not be poisoned");
        Ok(tools.clone())
    }

    async fn is_tools_empty(&self) -> Result<bool> {
        let tools = self.tools.lock().expect("mutex should not be poisoned");
        Ok(tools.is_empty())
    }

    async fn clear_tools(&self) -> Result<()> {
        let mut tools = self.tools.lock().expect("mutex should not be poisoned");
        tools.clear();
        debug!("Cleared tools from memory");
        Ok(())
    }
}
