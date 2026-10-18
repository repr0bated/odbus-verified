//! Private workflow execution receipts in the existing episode domain owner.
//! Execution ownership is never reconstructed from these rows after restart.

use super::*;

#[derive(Clone, Debug)]
pub struct WorkflowExecutionRecord {
    pub scope: EpisodeScope,
    pub execution_id: String,
    pub workflow_id: String,
    pub owner_epoch: String,
    pub status: String,
    pub revision: u64,
    pub document: Value,
}

fn params(scope: &EpisodeScope) -> Value {
    serde_json::json!({"principal": scope.principal_id, "session": scope.session_id,
        "genesis": scope.session_genesis})
}

fn validate(row: &WorkflowExecutionRecord) -> Result<(), CozoError> {
    for (text, max) in [
        (&row.scope.principal_id, 512),
        (&row.scope.session_id, 256),
        (&row.scope.session_genesis, 512),
        (&row.execution_id, 128),
        (&row.workflow_id, 256),
        (&row.owner_epoch, 128),
        (&row.status, 64),
    ] {
        if text.is_empty()
            || text.len() > max
            || text.trim() != text.as_str()
            || text.chars().any(char::is_control)
        {
            return Err(CozoError::Other(
                "invalid workflow execution identity or status".into(),
            ));
        }
    }
    if row.revision > i64::MAX as u64
        || !row.document.is_object()
        || serde_json::to_vec(&row.document)?.len() > 256 * 1024
    {
        return Err(CozoError::Other(
            "invalid workflow execution document".into(),
        ));
    }
    if row.document["accepted_at"]
        .as_str()
        .is_none_or(|timestamp| chrono::DateTime::parse_from_rfc3339(timestamp).is_err())
    {
        return Err(CozoError::Other(
            "invalid workflow accepted timestamp".into(),
        ));
    }
    Ok(())
}

impl CozoGraphShuttle {
    pub fn initialize_workflow_execution_store(&self) -> Result<(), CozoError> {
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| CozoError::Other("workflow owner poisoned".into()))?;
        self.runtime_document_preflight()?;
        // A generic Cozo handle cannot opt itself into the private workflow
        // owner. The bridge must first initialize the isolated episode domain.
        self.run_query(
            "?[principal_id] := *ctl_plane_chatbot_episode_scopes{principal_id} :limit 0",
            None,
        )?;
        if let Err(error) = self.run_query(
            ":create workflow_executions { principal_id: String, session_id: String, session_genesis: String, execution_id: String => workflow_id: String, owner_epoch: String, status: String, revision: Int, document: String, accepted_at: String }", None) {
            let message = error.to_string();
            if !message.contains("already exists") && !message.contains("AlreadyExists")
                && !message.contains("conflicts with an existing") { return Err(error); }
        }
        self.run_query("?[execution_id, workflow_id, owner_epoch, status, revision, document] := *workflow_executions{execution_id, workflow_id, owner_epoch, status, revision, document} :limit 0", None)?;
        self.sync_runtime_document_store()
    }

    /// Bounded exact-session history. No other principal/session is readable.
    pub fn list_workflow_executions(
        &self,
        scope: &EpisodeScope,
        workflow: Option<&str>,
        limit: usize,
    ) -> Result<Vec<WorkflowExecutionRecord>, CozoError> {
        self.read_workflow_executions(scope, workflow, None, limit)
    }

    pub fn get_workflow_execution(
        &self,
        scope: &EpisodeScope,
        execution: &str,
    ) -> Result<Option<WorkflowExecutionRecord>, CozoError> {
        Ok(self
            .read_workflow_executions(scope, None, Some(execution), 1)?
            .pop())
    }

    fn read_workflow_executions(
        &self,
        scope: &EpisodeScope,
        workflow: Option<&str>,
        execution: Option<&str>,
        limit: usize,
    ) -> Result<Vec<WorkflowExecutionRecord>, CozoError> {
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| CozoError::Other("workflow owner poisoned".into()))?;
        self.runtime_document_preflight()?;
        let limit = limit.clamp(1, 200);
        let filter = format!(
            "{}{}",
            if workflow.is_some() {
                ", workflow_id = $workflow"
            } else {
                ""
            },
            if execution.is_some() {
                ", execution_id = $execution"
            } else {
                ""
            }
        );
        let mut p = params(scope);
        p["workflow"] = serde_json::json!(workflow);
        p["execution"] = serde_json::json!(execution);
        let result = self.run_query(&format!("?[execution_id, workflow_id, owner_epoch, status, revision, document, accepted_at] := *workflow_executions{{principal_id, session_id, session_genesis, execution_id, workflow_id, owner_epoch, status, revision, document, accepted_at}}, principal_id = $principal, session_id = $session, session_genesis = $genesis{filter} :order -accepted_at, -execution_id :limit {limit}"), Some(p))?;
        result
            .as_array()
            .ok_or_else(|| CozoError::Other("invalid workflow rows".into()))?
            .iter()
            .map(|row| {
                let text = |field: &str| {
                    row[field]
                        .as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| CozoError::Other("invalid workflow column".into()))
                };
                let record = WorkflowExecutionRecord {
                    scope: scope.clone(),
                    execution_id: text("execution_id")?,
                    workflow_id: text("workflow_id")?,
                    owner_epoch: text("owner_epoch")?,
                    status: text("status")?,
                    revision: row["revision"]
                        .as_u64()
                        .ok_or_else(|| CozoError::Other("invalid workflow revision".into()))?,
                    document: serde_json::from_str(&text("document")?)?,
                };
                validate(&record)?;
                Ok(record)
            })
            .collect()
    }

    /// Compare-and-set under the domain owner's shared transaction lock.
    /// `None` creates a new execution; an existing id is never overwritten.
    pub fn put_workflow_execution(
        &self,
        row: &WorkflowExecutionRecord,
        expected: Option<u64>,
    ) -> Result<(), CozoError> {
        validate(row)?;
        let _guard = self
            .episode_transaction
            .lock()
            .map_err(|_| CozoError::Other("workflow owner poisoned".into()))?;
        self.runtime_document_preflight()?;
        let mut p = params(&row.scope);
        p["execution"] = serde_json::json!(row.execution_id);
        let old = self.run_query("?[revision, owner_epoch] := *workflow_executions{principal_id, session_id, session_genesis, execution_id, revision, owner_epoch}, principal_id = $principal, session_id = $session, session_genesis = $genesis, execution_id = $execution", Some(p.clone()))?;
        let rows = old
            .as_array()
            .ok_or_else(|| CozoError::Other("invalid workflow rows".into()))?;
        let matches = match (expected, rows.as_slice()) {
            (None, []) => row.revision == 0,
            (Some(revision), [old]) => {
                old["revision"].as_u64() == Some(revision)
                    && old["owner_epoch"].as_str() == Some(row.owner_epoch.as_str())
                    && row.revision == revision.saturating_add(1)
            }
            _ => false,
        };
        if !matches {
            return Err(CozoError::Other(
                "workflow execution revision/owner conflict".into(),
            ));
        }
        p["workflow"] = serde_json::json!(row.workflow_id);
        p["owner"] = serde_json::json!(row.owner_epoch);
        p["status"] = serde_json::json!(row.status);
        p["revision"] = serde_json::json!(row.revision);
        p["document"] = serde_json::json!(serde_json::to_string(&row.document)?);
        p["accepted_at"] = row
            .document
            .get("accepted_at")
            .cloned()
            .ok_or_else(|| CozoError::Other("workflow accepted timestamp missing".into()))?;
        self.run_query("?[principal_id, session_id, session_genesis, execution_id, workflow_id, owner_epoch, status, revision, document, accepted_at] <- [[$principal, $session, $genesis, $execution, $workflow, $owner, $status, $revision, $document, $accepted_at]] :put workflow_executions {principal_id, session_id, session_genesis, execution_id => workflow_id, owner_epoch, status, revision, document, accepted_at}", Some(p))?;
        self.sync_runtime_document_store().map_err(|error| CozoError::Other(format!(
            "workflow row committed but durability barrier failed; outcome uncertain, never replay or assume rollback: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, accepted: &str) -> WorkflowExecutionRecord {
        WorkflowExecutionRecord {
            scope: EpisodeScope {
                principal_id: "principal".into(),
                session_id: "session".into(),
                session_genesis: "genesis".into(),
            },
            execution_id: id.into(),
            workflow_id: "deploy".into(),
            owner_epoch: "owner".into(),
            status: "queued".into(),
            revision: 0,
            document: serde_json::json!({"accepted_at": accepted, "in_flight": false, "completed_steps": 0}),
        }
    }

    fn store() -> CozoGraphShuttle {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        store.initialize_chatbot_episode_store().unwrap();
        store.initialize_workflow_execution_store().unwrap();
        store
    }

    #[test]
    fn workflow_durable_cas_preserves_scope_and_owner() {
        let store = store();
        let mut record = row("id", "2026-10-05T00:00:00.000000000Z");
        store.put_workflow_execution(&record, None).unwrap();
        assert!(store.put_workflow_execution(&record, None).is_err());
        record.revision = 1;
        record.status = "paused".into();
        store.put_workflow_execution(&record, Some(0)).unwrap();
        assert!(store.put_workflow_execution(&record, Some(0)).is_err());
        let mut wrong_owner = record.clone();
        wrong_owner.revision = 2;
        wrong_owner.owner_epoch = "other-owner".into();
        assert!(store.put_workflow_execution(&wrong_owner, Some(1)).is_err());
        assert_eq!(
            store
                .get_workflow_execution(&record.scope, "id")
                .unwrap()
                .unwrap()
                .status,
            "paused"
        );
        for (principal, session, genesis) in [
            ("other", "session", "genesis"),
            ("principal", "other", "genesis"),
            ("principal", "session", "other"),
        ] {
            let scope = EpisodeScope {
                principal_id: principal.into(),
                session_id: session.into(),
                session_genesis: genesis.into(),
            };
            assert!(
                store
                    .get_workflow_execution(&scope, "id")
                    .unwrap()
                    .is_none()
            );
            assert!(
                store
                    .list_workflow_executions(&scope, None, 10)
                    .unwrap()
                    .is_empty()
            );
        }
    }

    #[test]
    fn workflow_history_orders_by_time_and_exact_lookup_survives_page_bound() {
        let store = store();
        let old = row("zzzz", "2026-10-05T00:00:00.000000000Z");
        let newer = row("aaaa", "2026-10-05T00:01:00.000000000Z");
        store.put_workflow_execution(&old, None).unwrap();
        store.put_workflow_execution(&newer, None).unwrap();
        let page = store
            .list_workflow_executions(&old.scope, Some("deploy"), 1)
            .unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].execution_id, "aaaa");
        assert_eq!(
            store
                .get_workflow_execution(&old.scope, "zzzz")
                .unwrap()
                .unwrap()
                .execution_id,
            "zzzz"
        );
    }

    #[test]
    fn workflow_relation_requires_private_episode_owner() {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        assert!(store.initialize_workflow_execution_store().is_err());
    }
}
