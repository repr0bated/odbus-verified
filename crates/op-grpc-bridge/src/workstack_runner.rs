//! Reactive workstack runner (op-web agent-orchestration port, OD-100 axis).
//!
//! A workstack started through the sealed admission path
//! (`workflows.start_workflow`, also exposed as `workflow_run` to the
//! chatbot door) executes here under the starting principal's exact
//! identity: every step is a sealed `cognitive_mcp` catalog method
//! dispatched by the MutationEngine with the starter's
//! `principal/session/genesis`, so grants are checked per step exactly as
//! an ordinary method call would be.
//!
//! The runner is event-driven only: the queue is fed by the admitted
//! `start_workflow` call itself. There is no timer, no journal tail, and
//! no broker poll anywhere in this module. The plugin journal keeps the
//! durable start receipt; this module appends one terminal record per run
//! and commits the run record as an episode in the starter's own
//! ctl-plane scope.
//!
//! Execution-leg grammar (carried by the existing free-form `params`,
//! no plugin schema change): a per-execution JSON string step is
//! `agent.operation|{json args}`; `agent` and `operation` resolve through
//! the canonical `cognitive_agents` catalog (`agent_method_name` /
//! `catalog_agent_method`) and the step dispatches exactly as a 3tchedFS
//! lungs `call` write does — sealed declaration lookup, sealed capability
//! from the same blob, MutationEngine admission under the identity.
//! A step that is not `agent.operation` has no executable binding in
//! this ledger and is refused honestly. The run itself proceeds only
//! under the starter's **verified identity session**, resolved from the
//! identity sled like a lungs `Bind`; fail closed without it.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use std::os::unix::fs::OpenOptionsExt;

use anyhow::{Context, Result, anyhow};
use op_cozo_store::EpisodeScope;
use op_plugins::state_plugins::ctl_plane_chatbot::{RunAction, RunRecord, RunStatus};
use op_plugins::state_plugins::ctl_plane_chatbot_methods::{
    CompletedAgentRun, build_completed_agent_run,
};
use serde_json::Value;
use tokio::sync::{Mutex, mpsc};

use crate::mutation_engine::MutationEngine;

const RUN_CAPACITY: usize = 64;
const RUN_CONCURRENCY: usize = 2;
const STEP_TIMEOUT: Duration = Duration::from_secs(120);
const STEP_NAME_MAX_BYTES: usize = 256;
const ARGS_MAX_BYTES: usize = 16 * 1024;

/// The cognitive catalog plugin id (same id `agent_workflow` dispatches
/// through; the sealed catalog owns every admitted step).
const COGNITIVE_PLUGIN: &str = "cognitive_mcp";

/// One admitted workstack execution waiting for its worker.
pub(crate) struct WorkstackRun {
    pub execution_id: String,
    pub workflow_id: String,
    /// Executable binding strings (`agent.operation|{json args}`).
    pub steps: Vec<String>,
    pub starter: EpisodeScope,
}

/// Bounded queue with a small worker pool; spawned once per bridge.
#[derive(Clone)]
pub(crate) struct WorkstackRunner {
    tx: mpsc::Sender<WorkstackRun>,
}

impl WorkstackRunner {
    fn start(engine: Arc<MutationEngine>) -> Self {
        let (tx, rx) = mpsc::channel::<WorkstackRun>(RUN_CAPACITY);
        let rx = Arc::new(Mutex::new(rx));
        for _ in 0..RUN_CONCURRENCY.max(1) {
            let rx = Arc::clone(&rx);
            let engine = Arc::clone(&engine);
            tokio::spawn(async move {
                loop {
                    let next = rx.lock().await.recv().await;
                    let Some(run) = next else { break };
                    execute(&engine, run).await;
                }
            });
        }
        Self { tx }
    }

    /// Enqueue or drop (queue full). Never blocks the caller; no polling.
    pub(crate) fn try_enqueue(&self, run: WorkstackRun) -> bool {
        match self.tx.try_send(run) {
            Ok(()) => true,
            Err(error) => {
                let id = match &error {
                    mpsc::error::TrySendError::Full(e) => &e.execution_id,
                    mpsc::error::TrySendError::Closed(e) => &e.execution_id,
                };
                tracing::warn!(execution_id = %id, "workstack run dropped: queue full");
                false
            }
        }
    }
}

static RUNNER: OnceLock<WorkstackRunner> = OnceLock::new();

/// Start the runner once with the server engine. Idempotent.
pub(crate) fn start_once(engine: Arc<MutationEngine>) {
    let _ = RUNNER.set(WorkstackRunner::start(engine));
}

/// Parse one `agent.operation|{args json}` binding.
pub(crate) fn parse_step(step: &str) -> Result<(String, String, Value)> {
    let (binding, args_json) = match step.split_once('|') {
        Some((binding, args_json)) => {
            anyhow::ensure!(
                args_json.len() <= ARGS_MAX_BYTES,
                "step args exceed {ARGS_MAX_BYTES} bytes"
            );
            (binding.trim(), args_json.trim())
        }
        None => (step.trim(), ""),
    };
    anyhow::ensure!(
        binding.len() <= STEP_NAME_MAX_BYTES,
        "step binding exceeds {STEP_NAME_MAX_BYTES} bytes"
    );
    let (agent, operation) = binding
        .split_once('.')
        .context("step has no executable binding: expected 'agent.operation'")?;
    let agent = agent.trim();
    let operation = operation.trim();
    anyhow::ensure!(
        !agent.is_empty() && !operation.is_empty(),
        "empty step binding"
    );
    let args = if args_json.is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_str(args_json).context("step args are not a JSON object")?
    };
    anyhow::ensure!(args.is_object(), "step args are not a JSON object");
    Ok((agent.to_string(), operation.to_string(), args))
}

/// Enqueue one accepted execution. Returns false when the runner backlog
/// is full or the starter has no verified session (fail closed; the
/// plugin's journal start receipt stays authoritative then).
pub(crate) fn enqueue(
    execution_id: &str,
    workflow_id: &str,
    steps: Vec<String>,
    actor_id: &str,
    session_id: Option<&str>,
    session_genesis: Option<&str>,
) -> bool {
    let (Some(session_id), Some(session_genesis)) = (session_id, session_genesis) else {
        tracing::warn!(
            execution_id,
            "workstack run dropped: no verified caller session"
        );
        return false;
    };
    let Some(runner) = RUNNER.get() else {
        tracing::warn!(
            execution_id,
            "workstack runner not started; execution dropped"
        );
        return false;
    };
    runner.try_enqueue(WorkstackRun {
        execution_id: execution_id.to_string(),
        workflow_id: workflow_id.to_string(),
        steps,
        starter: EpisodeScope {
            principal_id: actor_id.to_string(),
            session_id: session_id.to_string(),
            session_genesis: session_genesis.to_string(),
        },
    })
}

/// Execute one workstack to a terminal status without polling: the run
/// proceeds only under the starter's verified identity session (the same
/// session resolution a 3tchedFS lungs `Bind` performs; fail closed
/// without it), steps run sequentially, each separately deadlined, and
/// every result is committed to the ctl-plane episode ledger before the
/// function returns.
async fn execute(engine: &MutationEngine, run: WorkstackRun) {
    let started = Instant::now();
    let started_at = chrono::Utc::now();
    let scope = match crate::agent_runs::agent_scope(engine, &run.starter.session_id).await {
        Ok(Some(scope))
            if scope.principal_id == run.starter.principal_id
                && scope.session_genesis == run.starter.session_genesis =>
        {
            scope
        }
        _ => {
            tracing::warn!(
                execution_id = %run.execution_id,
                "workstack run dropped: starter session is not a verified identity session"
            );
            return;
        }
    };
    let mut actions: Vec<RunAction> = Vec::with_capacity(run.steps.len());
    let mut status = RunStatus::Succeeded;
    let mut reason: Option<String> = None;
    let mut summary = String::new();

    for step in &run.steps {
        let (agent, operation, args) = match parse_step(step) {
            Ok(parsed) => parsed,
            Err(error) => {
                actions.push(RunAction {
                    method: truncate(step, 64),
                    ok: false,
                    duration_ms: 0,
                    error: Some(truncate(&error.to_string(), 512)),
                });
                status = RunStatus::Failed;
                reason = Some("step has no executable binding".into());
                break;
            }
        };
        let name =
            op_plugins::state_plugins::cognitive_agents::agent_method_name(&agent, &operation);
        let Some(catalog) =
            op_plugins::state_plugins::cognitive_agents::catalog_agent_method(&agent, &operation)
        else {
            actions.push(RunAction {
                method: name,
                ok: false,
                duration_ms: 0,
                error: Some(format!(
                    "{agent}.{operation} is not a sealed catalog method"
                )),
            });
            status = RunStatus::Failed;
            reason = Some("step is not a sealed catalog method".into());
            break;
        };
        let at = Instant::now();
        let outcome = tokio::time::timeout(
            STEP_TIMEOUT,
            engine.dispatch_method_call_with_identity(
                COGNITIVE_PLUGIN,
                &name,
                &args.to_string(),
                catalog.declaration.required_capability.as_deref(),
                &scope.principal_id,
                Some(&scope.session_id),
                Some(&scope.session_genesis),
            ),
        )
        .await;
        let ok = matches!(&outcome, Ok(Ok(_)));
        if !ok {
            let error = match &outcome {
                Err(_) => "deadline exceeded".to_string(),
                Ok(Err(error)) => truncate(&error.to_string(), 1024),
                Ok(Ok(_)) => String::new(),
            };
            actions.push(RunAction {
                method: name,
                ok: false,
                duration_ms: at.elapsed().as_millis() as u64,
                error: Some(error),
            });
            status = RunStatus::Failed;
            reason = Some("a step was refused or failed".into());
            break;
        }
        let envelope = outcome
            .expect("checked")
            .map_err(|error| anyhow!("{}", error));
        let result = envelope
            .ok()
            .and_then(|envelope| envelope.get("result").cloned());
        summary = truncate(
            &serde_json::to_string(&result.unwrap_or(Value::Null)).unwrap_or_default(),
            2048,
        );
        actions.push(RunAction {
            method: name,
            ok: true,
            duration_ms: at.elapsed().as_millis() as u64,
            error: None,
        });
    }

    let record = RunRecord {
        status,
        trigger_id: truncate(&run.execution_id, 128),
        topic_ref: solution_ref(&run),
        queued_ms: 0,
        duration_ms: started.elapsed().as_millis() as u64,
        tokens_in: 0,
        tokens_out: 0,
        actions,
        reason: reason.as_ref().map(|r| truncate(r, 512)),
    };
    match build_completed_agent_run(CompletedAgentRun {
        scope: scope.clone(),
        trigger_id: truncate(&run.execution_id, 128),
        goal_text: truncate(&run.workflow_id, 512),
        summary,
        tools: record.actions.iter().map(|a| a.method.clone()).collect(),
        started_at,
        ended_at: chrono::Utc::now(),
        run: record,
    }) {
        Ok(row) => {
            if let Err(error) = engine.commit_chatbot_episode(row).await {
                tracing::warn!(execution_id = %run.execution_id, %error, "workstack episode not committed");
            }
        }
        Err(error) => {
            tracing::warn!(execution_id = %run.execution_id, %error, "workstack run record invalid");
        }
    }
    append_terminal_record(&run, reason.as_deref());
}

fn solution_ref(run: &WorkstackRun) -> String {
    blake3::Hash::from(blake3::derive_key(
        "opdbus workstack execution v1",
        format!("{}:{}", run.workflow_id, run.execution_id).as_bytes(),
    ))
    .to_hex()
    .to_string()
}

fn append_terminal_record(run: &WorkstackRun, reason: Option<&str>) {
    // Same journal the start receipt went to (the plugin's own file);
    // appends are bounded, terminal-only, and never polled.
    let path = journal_path();
    let record = serde_json::json!({
        "schema_version": 1,
        "execution_id": run.execution_id,
        "workflow_id": run.workflow_id,
        "status": "terminal",
        "finished_at": chrono::Utc::now().to_rfc3339(),
        "reason": reason,
    });
    drop(tokio::task::spawn_blocking(move || {
        journal_append(&path, &record)
    }));
}

fn journal_path() -> std::path::PathBuf {
    std::env::var_os("OP_WORKFLOW_JOURNAL_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(
                op_plugins::state_plugins::workflows_plugin::DEFAULT_WORKFLOW_JOURNAL,
            )
        })
}

fn journal_append(path: &std::path::Path, record: &serde_json::Value) -> Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(
        serde_json::to_string(record)
            .context("terminal record")?
            .as_bytes(),
    )?;
    file.write_all(b"\n")?;
    file.sync_data()?;
    Ok(())
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        text.to_string()
    } else {
        let mut cut = max;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}…", &text[..cut])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::human_principal_dispatch::tests::{temp_cozo, temp_shm_with_schemas, test_engine};
    use op_plugins::state_plugins::identity_sled::IdentitySledPlugin;
    use op_state::StatePlugin as _;

    #[test]
    fn workstack_parses_binding_with_inline_args() {
        let (agent, operation, args) =
            parse_step(r#" rust-pro . build | {"path": "crates/op-web", "note": "a|b"} "#)
                .expect("step parse");
        assert_eq!(agent, "rust-pro");
        assert_eq!(operation, "build");
        assert_eq!(
            args,
            serde_json::json!({"path": "crates/op-web", "note": "a|b"})
        );
        for step in ["rust-pro.build", "rust-pro.build|", "rust-pro.build| {}"] {
            let (agent, operation, args) = parse_step(step).unwrap();
            assert_eq!((agent.as_str(), operation.as_str()), ("rust-pro", "build"));
            assert_eq!(args, serde_json::json!({}));
        }
    }

    #[test]
    fn workstack_refuses_missing_bindings_and_invalid_arguments() {
        for step in [
            "",
            "build",
            ".build",
            "rust-pro.",
            " . |{}",
            "rust-pro.build|{",
            "rust-pro.build|[]",
            "rust-pro.build|null",
            "rust-pro.build|true",
            "rust-pro.build|42",
            r#"rust-pro.build|"text""#,
        ] {
            assert!(parse_step(step).is_err(), "accepted {step:?}");
        }
        // Parsing does not assert catalog membership; execution does.
        assert!(parse_step("deploy.spill|{}").is_ok());
    }

    #[test]
    fn workstack_bounds_binding_and_arguments_independently() {
        let binding = format!("a.{}", "b".repeat(STEP_NAME_MAX_BYTES - 2));
        assert!(parse_step(&binding).is_ok());
        assert!(parse_step(&format!("{binding}b")).is_err());
        let args = format!(r#"{{"text":"{}"}}"#, "x".repeat(ARGS_MAX_BYTES - 11));
        assert_eq!(args.len(), ARGS_MAX_BYTES);
        assert!(parse_step(&format!("rust-pro.build|{args}")).is_ok());
        assert!(parse_step(&format!("rust-pro.build|{args} ")).is_err());
    }

    #[test]
    fn workstack_enqueue_drops_without_session_or_genesis() {
        for (session, genesis) in [
            (None, None),
            (None, Some("genesis")),
            (Some("session"), None),
        ] {
            assert!(!enqueue(
                "execution",
                "workflow",
                vec![],
                "principal",
                session,
                genesis
            ));
        }
    }

    fn run(scope: EpisodeScope) -> WorkstackRun {
        WorkstackRun {
            execution_id: "execution".into(),
            workflow_id: "workflow".into(),
            steps: vec!["rust-pro.build|{}".into()],
            starter: scope,
        }
    }

    #[tokio::test]
    async fn workstack_queue_accepts_then_drops_when_full_or_closed() {
        let (tx, mut rx) = mpsc::channel(1);
        let runner = WorkstackRunner { tx };
        let scope = EpisodeScope {
            principal_id: "principal".into(),
            session_id: "session".into(),
            session_genesis: "genesis".into(),
        };
        assert!(runner.try_enqueue(run(scope.clone())));
        assert!(!runner.try_enqueue(run(scope.clone())));
        let queued = rx.recv().await.unwrap();
        assert_eq!(queued.starter, scope);
        assert_eq!(queued.execution_id, "execution");
        drop(rx);
        assert!(!runner.try_enqueue(run(scope)));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn workstack_no_verified_session_means_no_dispatch_or_episode() {
        let _cozo = temp_cozo();
        let _shm = temp_shm_with_schemas(&[IdentitySledPlugin::new().schema().unwrap()]);
        let engine = test_engine();
        let identities = crate::identity_sled_dispatch::IdentitySledStore::new(
            op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
        );
        engine
            .install_identity_sled_store_for_test(identities.clone())
            .unwrap();
        crate::identity_sled_dispatch::recover_identity_startup_with_store(&engine, &identities)
            .await
            .unwrap();
        let store = Arc::new(op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap());
        engine
            .configure_chatbot_episode_store(store.clone())
            .unwrap();
        let scope = EpisodeScope {
            principal_id: "principal".into(),
            session_id: "no-such-session".into(),
            session_genesis: "genesis".into(),
        };
        assert!(
            crate::agent_runs::agent_scope(&engine, &scope.session_id)
                .await
                .unwrap()
                .is_none(),
            "fixture exercises the None branch rather than a resolution error"
        );
        // Invalid binding would produce a failed episode if execution advanced
        // beyond session resolution. The scoped store must remain empty.
        let mut unverified = run(scope.clone());
        unverified.steps = vec!["no-executable-binding".into()];
        execute(&engine, unverified).await;
        assert!(
            store
                .list_scoped_episodes(&scope, None, 10)
                .unwrap()
                .is_empty()
        );
    }
}
