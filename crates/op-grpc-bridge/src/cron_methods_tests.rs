use super::*;
use op_state::StatePlugin;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tokio::sync::Mutex;

fn assert_output(method: &str, value: &serde_json::Value) {
    let schema = op_plugins::state_plugins::cron::CronPlugin::new()
        .schema()
        .unwrap();
    let contract = serde_json::to_value(schema.methods[method].returns.as_ref().unwrap()).unwrap();
    jsonschema::Validator::new(&contract)
        .unwrap()
        .validate(value)
        .unwrap();
}

struct Fixture {
    jobs: Mutex<Vec<CronJobRecord>>,
    runs: Mutex<Vec<CronRunRecord>>,
    calls: AtomicUsize,
    run_delay: Duration,
    indeterminate_run: bool,
    diverge_create: bool,
    fail_list_after_delete: bool,
    deleted: AtomicUsize,
}

fn default_delivery() -> CronDelivery {
    CronDelivery {
        mode: "none".into(),
        channel: None,
        to: None,
        thread_id: None,
        best_effort: true,
    }
}

fn job(id: &str, enabled: bool) -> CronJobRecord {
    CronJobRecord {
        id: id.into(),
        expression: "*/5 * * * *".into(),
        schedule: CronSchedule::Cron {
            expr: "*/5 * * * *".into(),
            tz: Some("UTC".into()),
        },
        command: String::new(),
        prompt: Some("summarize".into()),
        name: Some("digest".into()),
        job_type: CronJobType::Agent,
        session_target: CronSessionTarget::Isolated,
        model: None,
        agent_alias: "operator".into(),
        enabled,
        delivery: CronDelivery {
            mode: "none".into(),
            channel: None,
            to: None,
            thread_id: None,
            best_effort: true,
        },
        delete_after_run: false,
        allowed_tools: Some(vec!["file_read".into()]),
        uses_memory: false,
        source: "imperative".into(),
        shell_output_format: CronShellOutputFormat::Wrapped,
        created_at: "2026-09-13T00:00:00Z".into(),
        next_run: "2026-09-13T00:05:00Z".into(),
        last_run: None,
        last_status: None,
        last_output: None,
    }
}

#[async_trait]
impl CronGateway for Fixture {
    async fn list_jobs(&self) -> Result<Vec<CronJobRecord>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_list_after_delete && self.deleted.load(Ordering::SeqCst) != 0 {
            bail!("fixture list failure containing bearer=must-not-escape")
        }
        Ok(self.jobs.lock().await.clone())
    }
    async fn list_runs(&self, job_id: &str, limit: u32) -> Result<Vec<CronRunRecord>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self
            .runs
            .lock()
            .await
            .iter()
            .filter(|r| r.job_id == job_id)
            .take(limit as usize)
            .cloned()
            .collect())
    }
    async fn create_job(&self, input: CreateJobInput) -> Result<ProviderMutation<CronJobRecord>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut created = job("created", true);
        match input {
            CreateJobInput::Agent {
                agent,
                name,
                schedule,
                tz,
                prompt,
                delivery,
                session_target,
                model,
                allowed_tools,
                delete_after_run,
                uses_memory,
            } => {
                created.agent_alias = agent;
                created.name = name;
                created.expression = schedule.clone();
                created.schedule = CronSchedule::Cron { expr: schedule, tz };
                created.command.clear();
                created.prompt = Some(prompt);
                created.delivery = delivery.unwrap_or_else(default_delivery);
                created.session_target = session_target.unwrap_or(CronSessionTarget::Isolated);
                created.model = model;
                created.allowed_tools = allowed_tools;
                created.delete_after_run = delete_after_run.unwrap_or(false);
                created.uses_memory = uses_memory.unwrap_or(true);
                created.job_type = CronJobType::Agent;
            }
            CreateJobInput::Shell {
                agent,
                name,
                schedule,
                tz,
                command,
                delivery,
                shell_output_format,
            } => {
                created.agent_alias = agent;
                created.name = name;
                created.expression = schedule.clone();
                created.schedule = CronSchedule::Cron { expr: schedule, tz };
                created.command = command;
                created.prompt = None;
                created.delivery = delivery.unwrap_or_else(default_delivery);
                created.shell_output_format =
                    shell_output_format.unwrap_or(CronShellOutputFormat::Wrapped);
                created.job_type = CronJobType::Shell;
            }
        }
        if self.diverge_create {
            created.name = Some("provider-diverged".into());
        }
        self.jobs.lock().await.push(created.clone());
        Ok(ProviderMutation::Completed(created))
    }
    async fn update_job(&self, input: UpdateJobInput) -> Result<ProviderMutation<CronJobRecord>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut jobs = self.jobs.lock().await;
        let found = jobs
            .iter_mut()
            .find(|j| j.id == input.job_id)
            .context("missing")?;
        if let Some(enabled) = input.enabled {
            found.enabled = enabled;
        }
        if let Some(name) = input.name {
            found.name = Some(name);
        }
        Ok(ProviderMutation::Completed(found.clone()))
    }
    async fn delete_job(&self, job_id: &str) -> Result<ProviderMutation<()>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut jobs = self.jobs.lock().await;
        let before = jobs.len();
        jobs.retain(|j| j.id != job_id);
        ensure!(jobs.len() + 1 == before, "missing");
        self.deleted.fetch_add(1, Ordering::SeqCst);
        Ok(ProviderMutation::Completed(()))
    }
    async fn run_job(&self, job_id: &str) -> Result<ProviderMutation<ManualRunResult>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(self.run_delay).await;
        if self.indeterminate_run {
            return Ok(ProviderMutation::Indeterminate(
                "native run may still be executing".into(),
            ));
        }
        Ok(ProviderMutation::Completed(ManualRunResult {
            status: "ok".into(),
            job_id: job_id.into(),
            success: true,
            output: "private output".into(),
            duration_ms: 1,
            started_at: "2026-09-13T00:00:00Z".into(),
            finished_at: "2026-09-13T00:00:01Z".into(),
        }))
    }
}

fn fixture_with_run(
    run_delay: Duration,
    fail_list_after_delete: bool,
    indeterminate_run: bool,
    diverge_create: bool,
) -> Arc<Fixture> {
    Arc::new(Fixture {
        jobs: Mutex::new(vec![job("one", true)]),
        runs: Mutex::new(vec![CronRunRecord {
            id: 1,
            job_id: "one".into(),
            started_at: "2026-09-13T00:00:00Z".into(),
            finished_at: "2026-09-13T00:00:01Z".into(),
            status: "ok".into(),
            output: Some("private output".into()),
            duration_ms: Some(1),
        }]),
        calls: AtomicUsize::new(0),
        run_delay,
        indeterminate_run,
        diverge_create,
        fail_list_after_delete,
        deleted: AtomicUsize::new(0),
    })
}

fn fixture(run_delay: Duration, fail_list_after_delete: bool) -> Arc<Fixture> {
    fixture_with_run(run_delay, fail_list_after_delete, false, false)
}

#[tokio::test]
async fn all_nine_methods_return_provider_observations() {
    let backend = fixture(Duration::ZERO, false);
    let methods = CronMethods::new(backend.clone());
    let list = methods
        .dispatch("list_jobs", &serde_json::json!({}))
        .await
        .unwrap();
    assert_eq!(list["scope"], "global_daemon");
    assert_eq!(list["jobs"][0]["agent_alias"], "operator");
    assert_output("list_jobs", &list);
    let got = methods
        .dispatch("get_job", &serde_json::json!({"job_id":"one"}))
        .await
        .unwrap();
    assert_eq!(got["job"]["id"], "one");
    assert_output("get_job", &got);
    let history = methods
        .dispatch(
            "get_history",
            &serde_json::json!({"job_id":"one","limit":1}),
        )
        .await
        .unwrap();
    assert_eq!(history["runs"][0]["job_id"], "one");
    assert_output("get_history", &history);
    let created = methods.dispatch("create_job", &serde_json::json!({
            "job_type":"agent", "agent":"operator", "schedule":"*/5 * * * *", "prompt":"summarize"
        })).await.unwrap();
    assert_eq!(created["job"]["id"], "created");
    assert_eq!(created["disposition"], "confirmed");
    assert_output("create_job", &created);
    let updated = methods
        .dispatch(
            "update_job",
            &serde_json::json!({"job_id":"one","name":"renamed"}),
        )
        .await
        .unwrap();
    assert_eq!(updated["job"]["name"], "renamed");
    assert_output("update_job", &updated);
    let disabled = methods
        .dispatch("disable_job", &serde_json::json!({"job_id":"one"}))
        .await
        .unwrap();
    assert_eq!(disabled["job"]["enabled"], false);
    assert_output("disable_job", &disabled);
    let enabled = methods
        .dispatch("enable_job", &serde_json::json!({"job_id":"one"}))
        .await
        .unwrap();
    assert_eq!(enabled["job"]["enabled"], true);
    assert_output("enable_job", &enabled);
    let run = methods
        .dispatch("run_job", &serde_json::json!({"job_id":"one"}))
        .await
        .unwrap();
    assert_eq!(run["disposition"], "completed");
    assert_eq!(run["result"]["success"], true);
    assert_output("run_job", &run);
    let deleted = methods
        .dispatch("delete_job", &serde_json::json!({"job_id":"created"}))
        .await
        .unwrap();
    assert_eq!(deleted["provider_completed"], true);
    assert_eq!(deleted["absent_on_readback"], true);
    assert_output("delete_job", &deleted);
}

#[tokio::test]
async fn invalid_mutation_is_rejected_before_provider_call() {
    let backend = fixture(Duration::ZERO, false);
    let methods = CronMethods::new(backend.clone());
    assert!(
        methods
            .dispatch(
                "create_job",
                &serde_json::json!({
                    "job_type":"agent", "agent":"operator", "schedule":"*/5 * * * *", "prompt":""
                })
            )
            .await
            .is_err()
    );
    assert!(
        methods
            .dispatch(
                "update_job",
                &serde_json::json!({
                    "job_id":"one", "tz":"UTC", "clear_tz":true
                })
            )
            .await
            .is_err()
    );
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn divergent_create_readback_is_never_confirmed() {
    let backend = fixture_with_run(Duration::ZERO, false, false, true);
    let result = CronMethods::new(backend)
        .dispatch(
            "create_job",
            &serde_json::json!({
                "job_type":"agent", "agent":"operator", "name":"requested",
                "schedule":"*/5 * * * *", "prompt":"exact prompt",
                "uses_memory":false
            }),
        )
        .await
        .unwrap();
    assert_eq!(result["disposition"], "provider_completed_unverified");
}

#[tokio::test]
async fn observed_receipts_reuse_exact_readback_and_never_invent_state() {
    let backend = fixture(Duration::ZERO, false);
    let methods = CronMethods::new(backend.clone());

    let get = methods
        .dispatch_observed("get_job", &serde_json::json!({"job_id":"one"}))
        .await
        .unwrap();
    let get_state = get.state.unwrap();
    assert_eq!(get.output["job"]["id"], get_state.jobs[0].id);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);

    let created = methods
        .dispatch_observed(
            "create_job",
            &serde_json::json!({
                "job_type":"agent", "agent":"operator",
                "schedule":"*/5 * * * *", "prompt":"summarize"
            }),
        )
        .await
        .unwrap();
    let created_id = created.output["job"]["id"].as_str().unwrap().to_string();
    assert!(
        created
            .state
            .unwrap()
            .jobs
            .iter()
            .any(|job| job.id == created_id)
    );

    let deleted = methods
        .dispatch_observed("delete_job", &serde_json::json!({"job_id":"created"}))
        .await
        .unwrap();
    assert_eq!(deleted.output["absent_on_readback"], true);
    assert!(
        !deleted
            .state
            .unwrap()
            .jobs
            .iter()
            .any(|job| job.id == "created")
    );

    let indeterminate = CronMethods::new(fixture_with_run(Duration::ZERO, false, true, false))
        .dispatch_observed("run_job", &serde_json::json!({"job_id":"one"}))
        .await
        .unwrap();
    assert_eq!(indeterminate.output["disposition"], "indeterminate");
    assert!(indeterminate.state.is_none());
}

#[tokio::test]
async fn completed_delete_with_failed_readback_is_not_falsely_failed() {
    let backend = fixture(Duration::ZERO, true);
    backend.jobs.lock().await.push(job("remove-me", true));
    let result = CronMethods::new(backend)
        .dispatch("delete_job", &serde_json::json!({"job_id":"remove-me"}))
        .await
        .unwrap();
    assert_eq!(result["provider_completed"], true);
    assert!(result["absent_on_readback"].is_null());
    assert!(
        !result["readback_error"]
            .as_str()
            .unwrap()
            .contains("bearer=")
    );
}

#[tokio::test]
async fn provider_indeterminate_run_is_returned_without_retry() {
    let backend = fixture_with_run(Duration::ZERO, false, true, false);
    let methods = CronMethods::new(backend.clone());
    let result = methods
        .dispatch("run_job", &serde_json::json!({"job_id":"one"}))
        .await
        .unwrap();
    assert_eq!(result["disposition"], "indeterminate");
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
}
