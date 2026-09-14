//! Bounded asynchronous NotebookLM queries behind canonical method admission.
//! IDs are private to the exact verified principal and session. Results are
//! process-local, expire after 30 minutes, and never enter public projections.
use anyhow::{Context, Result};
use op_plugins::state_plugins::notebooklm::{
    outputs::{
        NotebookQueryFailure, NotebookQueryStartOutput, NotebookQueryState,
        NotebookQueryStatusOutput,
    },
    provider_outputs::NotebookQueryOutput,
    NotebookQueryInput,
};
use std::{
    collections::HashMap,
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{oneshot, Mutex, Semaphore};

const RETENTION: Duration = Duration::from_secs(1800);
const MAX_JOBS: usize = 64;
const MAX_SESSION_JOBS: usize = 8;
const MAX_RESULT_BYTES: usize = 1024 * 1024;

type QueryResult = std::result::Result<NotebookQueryOutput, NotebookQueryFailure>;

struct Job {
    principal: String,
    session: String,
    notebook_id: String,
    expires: Instant,
    expires_at: i64,
    receiver: Option<oneshot::Receiver<QueryResult>>,
    result: Option<QueryResult>,
}

pub(crate) struct NotebookQueries {
    jobs: Mutex<HashMap<String, Job>>,
    active: Arc<Semaphore>,
}

impl Default for NotebookQueries {
    fn default() -> Self {
        Self {
            jobs: Mutex::new(HashMap::new()),
            active: Arc::new(Semaphore::new(8)),
        }
    }
}

impl NotebookQueries {
    pub(crate) fn prepare(
        input: &NotebookQueryInput,
        selected: Option<&str>,
    ) -> Result<(String, Duration)> {
        anyhow::ensure!(
            !input.question.trim().is_empty() && input.question.len() <= 64 * 1024,
            "question must contain 1 to 65536 bytes"
        );
        let notebook = input
            .notebook_id
            .as_deref()
            .or(selected)
            .context("notebook_id or a selected notebook is required")?;
        anyhow::ensure!(
            uuid::Uuid::parse_str(notebook)?.to_string() == notebook,
            "notebook_id must be a canonical UUID"
        );
        let timeout = input.timeout.unwrap_or(660);
        anyhow::ensure!(
            (1..=900).contains(&timeout),
            "query timeout must be between 1 and 900 seconds"
        );
        Ok((notebook.to_owned(), Duration::from_secs(timeout)))
    }

    pub(crate) async fn start<F>(
        &self,
        principal: &str,
        session: &str,
        notebook_id: String,
        timeout: Duration,
        query: F,
    ) -> Result<NotebookQueryStartOutput>
    where
        F: Future<Output = Result<serde_json::Value>> + Send + 'static,
    {
        anyhow::ensure!(
            !principal.is_empty() && !session.is_empty(),
            "verified query owner is required"
        );
        let permit = self
            .active
            .clone()
            .try_acquire_owned()
            .context("NotebookLM query capacity is exhausted")?;
        let mut jobs = self.jobs.lock().await;
        jobs.retain(|_, job| job.expires > Instant::now());
        anyhow::ensure!(
            jobs.len() < MAX_JOBS,
            "NotebookLM result capacity is exhausted; wait for retained results to expire"
        );
        anyhow::ensure!(
            jobs.values()
                .filter(|job| job.principal == principal && job.session == session)
                .count()
                < MAX_SESSION_JOBS,
            "NotebookLM session query capacity is exhausted; wait for retained results to expire"
        );
        let query_id = uuid::Uuid::new_v4().to_string();
        let expires_at = chrono::Utc::now().timestamp() + RETENTION.as_secs() as i64;
        let (sender, receiver) = oneshot::channel();
        jobs.insert(
            query_id.clone(),
            Job {
                principal: principal.into(),
                session: session.into(),
                notebook_id: notebook_id.clone(),
                expires: Instant::now() + RETENTION,
                expires_at,
                receiver: Some(receiver),
                result: None,
            },
        );
        // No await between publication and spawn: cancellation cannot leave a
        // published job whose worker was never started. A panic closes sender.
        tokio::spawn(async move {
            let _permit = permit;
            let result = match tokio::time::timeout(timeout, query).await {
                Err(_) => Err(NotebookQueryFailure::TimedOut),
                Ok(Err(_)) => Err(NotebookQueryFailure::ProviderFailed),
                Ok(Ok(value)) => {
                    let bounded = serde_json::to_vec(&value)
                        .is_ok_and(|bytes| bytes.len() <= MAX_RESULT_BYTES);
                    if bounded {
                        serde_json::from_value(value)
                            .map_err(|_| NotebookQueryFailure::InvalidResponse)
                    } else {
                        Err(NotebookQueryFailure::InvalidResponse)
                    }
                }
            };
            let _ = sender.send(result);
        });
        Ok(NotebookQueryStartOutput {
            query_id,
            notebook_id,
            expires_at,
        })
    }

    pub(crate) async fn status(
        &self,
        principal: &str,
        session: &str,
        query_id: &str,
    ) -> Result<NotebookQueryStatusOutput> {
        anyhow::ensure!(
            uuid::Uuid::parse_str(query_id).is_ok_and(|id| id.to_string() == query_id),
            "query is unavailable"
        );
        let mut jobs = self.jobs.lock().await;
        jobs.retain(|_, job| job.expires > Instant::now());
        let job = jobs
            .get_mut(query_id)
            .filter(|job| job.principal == principal && job.session == session)
            .context("query is unavailable")?;
        if let Some(receiver) = job.receiver.as_mut() {
            match receiver.try_recv() {
                Ok(result) => {
                    job.result = Some(result);
                    job.receiver = None;
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    job.result = Some(Err(NotebookQueryFailure::WorkerFailed));
                    job.receiver = None;
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
            }
        }
        let (state, result, failure) = match &job.result {
            None => (NotebookQueryState::Running, None, None),
            Some(Ok(result)) => (NotebookQueryState::Completed, Some(result.clone()), None),
            Some(Err(failure)) => (NotebookQueryState::Failed, None, Some(failure.clone())),
        };
        Ok(NotebookQueryStatusOutput {
            query_id: query_id.into(),
            notebook_id: job.notebook_id.clone(),
            expires_at: job.expires_at,
            state,
            result,
            failure,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn settle(queries: &NotebookQueries, id: &str) -> NotebookQueryStatusOutput {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let status = queries.status("principal", "session", id).await.unwrap();
                if !matches!(status.state, NotebookQueryState::Running) {
                    return status;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn pending_result_is_private_then_returns_actual_unicode_and_citations() {
        let queries = NotebookQueries::default();
        let (sender, receiver) = oneshot::channel();
        let job = queries
            .start(
                "principal",
                "session",
                "notebook".into(),
                Duration::from_secs(2),
                async { Ok(receiver.await?) },
            )
            .await
            .unwrap();
        assert!(matches!(
            queries
                .status("principal", "session", &job.query_id)
                .await
                .unwrap()
                .state,
            NotebookQueryState::Running
        ));
        assert!(queries
            .status("other", "session", &job.query_id)
            .await
            .is_err());
        assert!(queries
            .status("principal", "other", &job.query_id)
            .await
            .is_err());
        sender.send(json!({"answer":"資料 🌊", "question":"q", "conversation_id":null, "sources_used":[{"id":"s"}], "citations":{"1":{"source":"s"}}, "references":[]})).unwrap();
        let result = settle(&queries, &job.query_id).await;
        assert!(matches!(result.state, NotebookQueryState::Completed));
        assert_eq!(result.result.unwrap().answer, "資料 🌊");
        assert!(NotebookQueries::default()
            .status("principal", "session", &job.query_id)
            .await
            .is_err());
        queries
            .jobs
            .lock()
            .await
            .get_mut(&job.query_id)
            .unwrap()
            .expires = Instant::now();
        assert!(queries
            .status("principal", "session", &job.query_id)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn errors_timeouts_and_bad_responses_do_not_become_success_or_echo_secrets() {
        for (value, expected) in [
            (
                Err(anyhow::anyhow!("private token")),
                NotebookQueryFailure::ProviderFailed,
            ),
            (
                Ok(json!({"success":true})),
                NotebookQueryFailure::InvalidResponse,
            ),
            (
                Ok(json!({"answer":"x".repeat(MAX_RESULT_BYTES+1)})),
                NotebookQueryFailure::InvalidResponse,
            ),
        ] {
            let queries = NotebookQueries::default();
            let job = queries
                .start(
                    "principal",
                    "session",
                    "n".into(),
                    Duration::from_secs(1),
                    async { value },
                )
                .await
                .unwrap();
            let status = settle(&queries, &job.query_id).await;
            assert_eq!(
                serde_json::to_value(status.failure).unwrap(),
                serde_json::to_value(Some(expected)).unwrap()
            );
            assert!(status.result.is_none());
        }
        let queries = NotebookQueries::default();
        let job = queries
            .start(
                "principal",
                "session",
                "n".into(),
                Duration::from_millis(1),
                std::future::pending(),
            )
            .await
            .unwrap();
        assert!(matches!(
            settle(&queries, &job.query_id).await.failure,
            Some(NotebookQueryFailure::TimedOut)
        ));
    }

    #[tokio::test]
    async fn capacity_rejection_does_not_start_another_provider() {
        let queries = NotebookQueries::default();
        let permits = queries.active.clone().acquire_many_owned(8).await.unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = calls.clone();
        assert!(queries
            .start(
                "principal",
                "session",
                "n".into(),
                Duration::from_secs(1),
                async move {
                    observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(json!({}))
                }
            )
            .await
            .is_err());
        drop(permits);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        let input = NotebookQueryInput {
            question: "q".into(),
            notebook_id: Some(uuid::Uuid::new_v4().to_string()),
            timeout: Some(901),
        };
        assert!(NotebookQueries::prepare(&input, None).is_err());
    }
}
