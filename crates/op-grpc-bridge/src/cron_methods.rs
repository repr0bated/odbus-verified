//! Staged native adapter for all nine retained cron methods.
//!
//! Integration contract:
//! * generated dispatch performs exact sealed method admission and verifies the
//!   current identity has the method's exact `cron.read`/`cron.invoke` grant;
//! * the gateway bearer authenticates this loopback daemon client only;
//! * MutationEngine runs every mutation branch as an owned task through native
//!   completion and commits the returned observed state before notification;
//! * audit records digest create/update bodies and run output, never raw text.

use anyhow::{bail, ensure, Context, Result};
use async_trait::async_trait;
use op_plugins::state_plugins::cron::*;
use std::{collections::BTreeSet, sync::Arc};

pub(crate) enum ProviderMutation<T> {
    Completed(T),
    /// The request was admitted and may have reached the daemon, but no
    /// authoritative response was received. The string is already sanitized.
    Indeterminate(String),
}

#[async_trait]
pub(crate) trait CronGateway: Send + Sync {
    async fn list_jobs(&self) -> Result<Vec<CronJobRecord>>;
    async fn list_runs(&self, job_id: &str, limit: u32) -> Result<Vec<CronRunRecord>>;
    async fn create_job(&self, input: CreateJobInput) -> Result<ProviderMutation<CronJobRecord>>;
    async fn update_job(&self, input: UpdateJobInput) -> Result<ProviderMutation<CronJobRecord>>;
    async fn delete_job(&self, job_id: &str) -> Result<ProviderMutation<()>>;
    async fn run_job(&self, job_id: &str) -> Result<ProviderMutation<ManualRunResult>>;
}

pub(crate) struct CronMethods {
    gateway: Arc<dyn CronGateway>,
}

pub(crate) struct CronDispatchReceipt {
    pub(crate) output: serde_json::Value,
    pub(crate) state: Option<CronStateV2>,
}

impl CronMethods {
    pub(crate) fn new(gateway: Arc<dyn CronGateway>) -> Self {
        Self { gateway }
    }

    /// Observe the complete schema-shaped global state from the native owner.
    /// MutationEngine calls this after a mutation and commits it to SHM before
    /// returning/notifying. Failure means no new state is fabricated.
    pub(crate) async fn observe_state(&self) -> Result<CronStateV2> {
        Ok(CronStateV2 {
            scope: CronScope::GlobalDaemon,
            event_stream: CronEventStreamIntegrity::default(),
            jobs: self.list_jobs().await?,
        })
    }

    /// Dispatch and retain the exact complete observation used for SHM.
    /// The caller holds the global cron operation guard through publication.
    pub(crate) async fn dispatch_observed(
        &self,
        method: &str,
        args: &serde_json::Value,
    ) -> Result<CronDispatchReceipt> {
        if method == "get_job" {
            return self.get_job_observed(args).await;
        }
        if method == "delete_job" {
            return self.delete_observed(args).await;
        }
        let mut output = self.dispatch(method, args).await?;
        if method == "list_jobs" {
            let listed: ListJobsOutput = serde_json::from_value(output.clone())?;
            return Ok(CronDispatchReceipt {
                output,
                state: Some(CronStateV2 {
                    scope: listed.scope,
                    event_stream: CronEventStreamIntegrity::default(),
                    jobs: listed.jobs,
                }),
            });
        }
        if !matches!(
            method,
            "create_job" | "update_job" | "delete_job" | "enable_job" | "disable_job" | "run_job"
        ) || output
            .get("disposition")
            .and_then(serde_json::Value::as_str)
            == Some("indeterminate")
        {
            return Ok(CronDispatchReceipt {
                output,
                state: None,
            });
        }
        match self.observe_state().await {
            Ok(state) => Ok(CronDispatchReceipt {
                output,
                state: Some(state),
            }),
            Err(_) => {
                mark_publication_unverified(method, &mut output)?;
                Ok(CronDispatchReceipt {
                    output,
                    state: None,
                })
            }
        }
    }

    pub(crate) async fn dispatch(
        &self,
        method: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        match method {
            "list_jobs" => {
                let _: ListJobsInput = serde_json::from_value(args.clone())?;
                output(ListJobsOutput {
                    scope: CronScope::GlobalDaemon,
                    jobs: self.list_jobs().await?,
                })
            }
            "get_job" => Ok(self.get_job_observed(args).await?.output),
            "get_history" => {
                let input: GetHistoryInput = serde_json::from_value(args.clone())?;
                validate_job_id(&input.job_id)?;
                ensure!(
                    (1..=MAX_CRON_HISTORY).contains(&input.limit),
                    "history limit must be between 1 and {MAX_CRON_HISTORY}"
                );
                // Native GET /runs first proves the job exists and clamps the
                // same range; this adapter rejects instead of silently clamping.
                let runs = self.gateway.list_runs(&input.job_id, input.limit).await?;
                ensure!(
                    runs.len() <= input.limit as usize,
                    "tched_router returned more history rows than requested"
                );
                ensure!(
                    runs.iter().all(|run| run.job_id == input.job_id),
                    "tched_router returned history for another job"
                );
                output(GetHistoryOutput {
                    scope: CronScope::GlobalDaemon,
                    job_id: input.job_id,
                    runs,
                })
            }
            "create_job" => {
                let input: CreateJobInput = serde_json::from_value(args.clone())?;
                validate_create(&input)?;
                let expected = input.clone();
                match self.gateway.create_job(input).await? {
                    ProviderMutation::Completed(job) => {
                        let valid = validate_job(&job).is_ok();
                        let matches = valid && create_matches(&expected, &job);
                        output(JobMutationOutput {
                            scope: CronScope::GlobalDaemon,
                            disposition: if matches {
                                MutationDisposition::Confirmed
                            } else {
                                MutationDisposition::ProviderCompletedUnverified
                            },
                            job: Some(job),
                            diagnostic: (!matches).then(|| {
                                "provider completed create but readback was invalid or owner/type diverged"
                                    .to_string()
                            }),
                        })
                    }
                    ProviderMutation::Indeterminate(diagnostic) => output(JobMutationOutput {
                        scope: CronScope::GlobalDaemon,
                        disposition: MutationDisposition::Indeterminate,
                        job: None,
                        diagnostic: Some(diagnostic),
                    }),
                }
            }
            "update_job" => {
                let input: UpdateJobInput = serde_json::from_value(args.clone())?;
                validate_update(&input)?;
                let prior = exact_job(self.list_jobs().await?, &input.job_id)?;
                validate_update_for_job(&input, &prior)?;
                let expected = input.clone();
                match self.gateway.update_job(input).await? {
                    ProviderMutation::Completed(job) => {
                        let valid = validate_job(&job).is_ok();
                        let matches = valid && update_matches(&expected, &prior, &job);
                        output(JobMutationOutput {
                            scope: CronScope::GlobalDaemon,
                            disposition: if matches {
                                MutationDisposition::Confirmed
                            } else {
                                MutationDisposition::ProviderCompletedUnverified
                            },
                            job: Some(job),
                            diagnostic: (!matches).then(|| {
                                "provider completed update but readback was invalid or diverged"
                                    .to_string()
                            }),
                        })
                    }
                    ProviderMutation::Indeterminate(diagnostic) => output(JobMutationOutput {
                        scope: CronScope::GlobalDaemon,
                        disposition: MutationDisposition::Indeterminate,
                        job: None,
                        diagnostic: Some(diagnostic),
                    }),
                }
            }
            "enable_job" | "disable_job" => {
                let input: JobIdInput = serde_json::from_value(args.clone())?;
                validate_job_id(&input.job_id)?;
                let enabled = method == "enable_job";
                let patch = UpdateJobInput {
                    job_id: input.job_id.clone(),
                    agent: None,
                    name: None,
                    schedule: None,
                    tz: None,
                    clear_tz: None,
                    command: None,
                    prompt: None,
                    enabled: Some(enabled),
                    uses_memory: None,
                    shell_output_format: None,
                };
                match self.gateway.update_job(patch).await? {
                    ProviderMutation::Completed(job) => {
                        let valid = validate_job(&job).is_ok();
                        let matches = valid && job.id == input.job_id && job.enabled == enabled;
                        output(JobMutationOutput {
                            scope: CronScope::GlobalDaemon,
                            disposition: if matches {
                                MutationDisposition::Confirmed
                            } else {
                                MutationDisposition::ProviderCompletedUnverified
                            },
                            job: Some(job),
                            diagnostic: (!matches).then(|| {
                                "provider completed toggle but readback was invalid or diverged"
                                    .to_string()
                            }),
                        })
                    }
                    ProviderMutation::Indeterminate(diagnostic) => output(JobMutationOutput {
                        scope: CronScope::GlobalDaemon,
                        disposition: MutationDisposition::Indeterminate,
                        job: None,
                        diagnostic: Some(diagnostic),
                    }),
                }
            }
            "delete_job" => Ok(self.delete_observed(args).await?.output),
            "run_job" => {
                let input: JobIdInput = serde_json::from_value(args.clone())?;
                validate_job_id(&input.job_id)?;
                let id = input.job_id;
                // MutationEngine already owns the complete admitted operation.
                // Await the shared transport's bounded long-running request so
                // its global operation guard is not released while a detached
                // provider mutation is still completing.
                match self.gateway.run_job(&id).await? {
                    ProviderMutation::Completed(result) => {
                        if result.job_id == id {
                            output(RunJobOutput {
                                scope: CronScope::GlobalDaemon,
                                job_id: id,
                                disposition: RunDisposition::Completed,
                                result: Some(result),
                                diagnostic: None,
                            })
                        } else {
                            output(RunJobOutput {
                                scope: CronScope::GlobalDaemon,
                                job_id: id,
                                disposition: RunDisposition::Indeterminate,
                                result: None,
                                diagnostic: Some("native cron run response identified another job; inspect history before retrying".into()),
                            })
                        }
                    }
                    ProviderMutation::Indeterminate(diagnostic) => output(RunJobOutput {
                        scope: CronScope::GlobalDaemon,
                        job_id: id,
                        disposition: RunDisposition::Indeterminate,
                        result: None,
                        diagnostic: Some(diagnostic),
                    }),
                }
            }
            _ => bail!("unsupported cron method: {method}"),
        }
    }

    async fn list_jobs(&self) -> Result<Vec<CronJobRecord>> {
        let jobs = self.gateway.list_jobs().await?;
        ensure!(
            jobs.len() <= MAX_CRON_JOBS,
            "tched_router returned more than {MAX_CRON_JOBS} cron jobs"
        );
        let mut ids = BTreeSet::new();
        for job in &jobs {
            validate_job(job)?;
            ensure!(
                ids.insert(job.id.as_str()),
                "tched_router returned duplicate cron job IDs"
            );
        }
        Ok(jobs)
    }

    async fn get_job_observed(&self, args: &serde_json::Value) -> Result<CronDispatchReceipt> {
        let input: JobIdInput = serde_json::from_value(args.clone())?;
        validate_job_id(&input.job_id)?;
        let jobs = self.list_jobs().await?;
        let job = exact_job(jobs.clone(), &input.job_id)?;
        Ok(CronDispatchReceipt {
            output: output(GetJobOutput {
                scope: CronScope::GlobalDaemon,
                job,
            })?,
            state: Some(CronStateV2 {
                scope: CronScope::GlobalDaemon,
                event_stream: CronEventStreamIntegrity::default(),
                jobs,
            }),
        })
    }

    async fn delete_observed(&self, args: &serde_json::Value) -> Result<CronDispatchReceipt> {
        let input: JobIdInput = serde_json::from_value(args.clone())?;
        validate_job_id(&input.job_id)?;
        if let ProviderMutation::Indeterminate(diagnostic) =
            self.gateway.delete_job(&input.job_id).await?
        {
            return Ok(CronDispatchReceipt {
                output: output(DeleteJobOutput {
                    scope: CronScope::GlobalDaemon,
                    job_id: input.job_id,
                    provider_completed: false,
                    disposition: MutationDisposition::Indeterminate,
                    absent_on_readback: None,
                    readback_error: Some(diagnostic),
                })?,
                state: None,
            });
        }
        // This exact list is both deletion evidence and the SHM candidate.
        // No second observation can disagree with the returned disposition.
        match self.list_jobs().await {
            Ok(jobs) => {
                let absent = !jobs.iter().any(|job| job.id == input.job_id);
                Ok(CronDispatchReceipt {
                    output: output(DeleteJobOutput {
                        scope: CronScope::GlobalDaemon,
                        job_id: input.job_id,
                        provider_completed: true,
                        disposition: if absent {
                            MutationDisposition::Confirmed
                        } else {
                            MutationDisposition::ProviderCompletedUnverified
                        },
                        absent_on_readback: Some(absent),
                        readback_error: (!absent).then(|| {
                            "provider reported deletion but readback retained the job".to_string()
                        }),
                    })?,
                    state: Some(CronStateV2 {
                        scope: CronScope::GlobalDaemon,
                        event_stream: CronEventStreamIntegrity::default(),
                        jobs,
                    }),
                })
            }
            Err(_) => Ok(CronDispatchReceipt {
                output: output(DeleteJobOutput {
                    scope: CronScope::GlobalDaemon,
                    job_id: input.job_id,
                    provider_completed: true,
                    disposition: MutationDisposition::ProviderCompletedUnverified,
                    absent_on_readback: None,
                    readback_error: Some(
                        "global cron readback unavailable after provider completed deletion"
                            .to_string(),
                    ),
                })?,
                state: None,
            }),
        }
    }
}

pub(crate) fn mark_publication_unverified(
    method: &str,
    output: &mut serde_json::Value,
) -> Result<()> {
    let object = output
        .as_object_mut()
        .context("cron mutation output is not an object")?;
    let message = "provider operation completed but the complete global cron state could not be observed for publication";
    match method {
        "create_job" | "update_job" | "enable_job" | "disable_job" | "delete_job" => {
            object.insert(
                "disposition".into(),
                serde_json::Value::String("provider_completed_unverified".into()),
            );
            let field = if method == "delete_job" {
                "readback_error"
            } else {
                "diagnostic"
            };
            object.insert(field.into(), serde_json::Value::String(message.into()));
        }
        "run_job" => {
            // The run result itself is complete; only state publication evidence
            // is unavailable.
            object.insert(
                "diagnostic".into(),
                serde_json::Value::String(message.into()),
            );
        }
        _ => bail!("state-readback annotation requested for non-mutation"),
    }
    Ok(())
}

fn output(value: impl serde::Serialize) -> Result<serde_json::Value> {
    let value = serde_json::to_value(value)?;
    let bytes = serde_json::to_vec(&value)?;
    ensure!(
        bytes.len() <= 8 * 1024 * 1024,
        "cron method output exceeds 8 MiB"
    );
    Ok(value)
}

fn validate_job_id(value: &str) -> Result<()> {
    validate_atom("job_id", value, 256)
}
fn validate_agent(value: &str) -> Result<()> {
    validate_atom("agent", value, 128)
}
fn validate_atom(field: &str, value: &str, max: usize) -> Result<()> {
    ensure!(
        !value.is_empty() && value.len() <= max,
        "{field} must be 1..={max} bytes"
    );
    ensure!(
        !value.chars().any(char::is_control),
        "{field} contains control characters"
    );
    Ok(())
}
fn validate_text(field: &str, value: &str, max: usize) -> Result<()> {
    ensure!(
        !value.trim().is_empty() && value.len() <= max,
        "{field} must be non-empty and at most {max} bytes"
    );
    Ok(())
}
fn validate_optional_text(field: &str, value: Option<&str>, max: usize) -> Result<()> {
    if let Some(value) = value {
        validate_text(field, value, max)?;
    }
    Ok(())
}
fn validate_delivery(delivery: Option<&CronDelivery>) -> Result<()> {
    let Some(delivery) = delivery else {
        return Ok(());
    };
    ensure!(
        delivery.mode.eq_ignore_ascii_case("none")
            || delivery.mode.eq_ignore_ascii_case("announce"),
        "delivery.mode must be none or announce"
    );
    if delivery.mode.eq_ignore_ascii_case("announce") {
        validate_optional_text("delivery.channel", delivery.channel.as_deref(), 128)?;
        validate_optional_text("delivery.to", delivery.to.as_deref(), 1024)?;
        ensure!(
            delivery.channel.is_some() && delivery.to.is_some(),
            "announce delivery requires channel and to"
        );
        validate_optional_text("delivery.thread_id", delivery.thread_id.as_deref(), 1024)?;
    }
    Ok(())
}
fn validate_tools(tools: Option<&Vec<String>>) -> Result<()> {
    let Some(tools) = tools else {
        return Ok(());
    };
    ensure!(
        tools.len() <= MAX_CRON_TOOL_NAMES,
        "allowed_tools exceeds {MAX_CRON_TOOL_NAMES} entries"
    );
    let mut unique = BTreeSet::new();
    for tool in tools {
        validate_atom("allowed_tools entry", tool, 128)?;
        ensure!(
            unique.insert(tool.as_str()),
            "allowed_tools contains duplicates"
        );
    }
    Ok(())
}
fn validate_schedule(expr: &str, tz: Option<&str>) -> Result<()> {
    validate_text("schedule", expr, 1024)?;
    validate_optional_text("tz", tz, 128)
}
fn validate_create(input: &CreateJobInput) -> Result<()> {
    match input {
        CreateJobInput::Agent {
            agent,
            name,
            schedule,
            tz,
            prompt,
            delivery,
            session_target: _,
            model,
            allowed_tools,
            delete_after_run: _,
            uses_memory: _,
        } => {
            validate_agent(agent)?;
            validate_optional_text("name", name.as_deref(), 256)?;
            validate_schedule(schedule, tz.as_deref())?;
            validate_text("prompt", prompt, MAX_CRON_TEXT_BYTES)?;
            validate_delivery(delivery.as_ref())?;
            validate_optional_text("model", model.as_deref(), 256)?;
            validate_tools(allowed_tools.as_ref())?;
        }
        CreateJobInput::Shell {
            agent,
            name,
            schedule,
            tz,
            command,
            delivery,
            shell_output_format: _,
        } => {
            validate_agent(agent)?;
            validate_optional_text("name", name.as_deref(), 256)?;
            validate_schedule(schedule, tz.as_deref())?;
            validate_text("command", command, MAX_CRON_TEXT_BYTES)?;
            validate_delivery(delivery.as_ref())?;
        }
    }
    Ok(())
}
fn schedule_matches(job: &CronJobRecord, expression: &str, timezone: Option<&str>) -> bool {
    // v0.8.4 validates a Cron { expr, tz } without expanding five-field
    // expressions or adding a seconds field. Preserve that exact expression;
    // only the API's documented timezone trim is accepted.
    job.expression == expression
        && matches!(
            &job.schedule,
            CronSchedule::Cron { expr, tz }
                if expr == expression
                    && timezone.is_none_or(|expected| tz.as_deref() == Some(expected.trim()))
        )
}

fn create_matches(input: &CreateJobInput, job: &CronJobRecord) -> bool {
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
            job.job_type == CronJobType::Agent
                && job.agent_alias == agent.as_str()
                && job.name.as_ref() == name.as_ref()
                && schedule_matches(job, schedule, tz.as_deref())
                && job.command.is_empty()
                && job.prompt.as_deref() == Some(prompt.as_str())
                && delivery.as_ref().is_none_or(|value| &job.delivery == value)
                && session_target
                    .as_ref()
                    .is_none_or(|value| &job.session_target == value)
                && model
                    .as_ref()
                    .is_none_or(|value| job.model.as_ref() == Some(value))
                && allowed_tools
                    .as_ref()
                    .is_none_or(|value| job.allowed_tools.as_ref() == Some(value))
                && delete_after_run.is_none_or(|value| job.delete_after_run == value)
                && uses_memory.is_none_or(|value| job.uses_memory == value)
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
            job.job_type == CronJobType::Shell
                && job.agent_alias == agent.as_str()
                && job.name.as_ref() == name.as_ref()
                && schedule_matches(job, schedule, tz.as_deref())
                && job.command == command.as_str()
                && job.prompt.is_none()
                && delivery.as_ref().is_none_or(|value| &job.delivery == value)
                && shell_output_format
                    .as_ref()
                    .is_none_or(|value| &job.shell_output_format == value)
        }
    }
}

fn validate_update(input: &UpdateJobInput) -> Result<()> {
    validate_job_id(&input.job_id)?;
    if let Some(agent) = &input.agent {
        validate_agent(agent)?;
    }
    validate_optional_text("name", input.name.as_deref(), 256)?;
    if let Some(schedule) = &input.schedule {
        validate_schedule(schedule, input.tz.as_deref())?;
    } else {
        validate_optional_text("tz", input.tz.as_deref(), 128)?;
    }
    ensure!(
        !(input.clear_tz == Some(true) && input.tz.is_some()),
        "provide either tz or clear_tz=true, not both"
    );
    ensure!(
        !(input.command.is_some() && input.prompt.is_some()),
        "update cannot set command and prompt together"
    );
    validate_optional_text("command", input.command.as_deref(), MAX_CRON_TEXT_BYTES)?;
    validate_optional_text("prompt", input.prompt.as_deref(), MAX_CRON_TEXT_BYTES)?;
    ensure!(
        input.name.is_some()
            || input.schedule.is_some()
            || input.tz.is_some()
            || input.clear_tz == Some(true)
            || input.command.is_some()
            || input.prompt.is_some()
            || input.enabled.is_some()
            || input.uses_memory.is_some()
            || input.shell_output_format.is_some(),
        "update_job requires at least one change"
    );
    Ok(())
}

fn validate_update_for_job(input: &UpdateJobInput, prior: &CronJobRecord) -> Result<()> {
    if prior.job_type == CronJobType::Shell && (input.command.is_some() || input.prompt.is_some()) {
        ensure!(
            input.agent.is_some(),
            "agent is required when changing shell job content"
        );
    }
    if prior.job_type == CronJobType::Agent {
        ensure!(
            input.shell_output_format.is_none(),
            "shell_output_format is not applicable to agent jobs"
        );
    }
    Ok(())
}

fn update_matches(input: &UpdateJobInput, prior: &CronJobRecord, job: &CronJobRecord) -> bool {
    if job.id != input.job_id
        || input
            .name
            .as_ref()
            .is_some_and(|value| job.name.as_ref() != Some(value))
        || input.enabled.is_some_and(|value| job.enabled != value)
        || input
            .uses_memory
            .is_some_and(|value| job.uses_memory != value)
        || input
            .shell_output_format
            .as_ref()
            .is_some_and(|value| &job.shell_output_format != value)
    {
        return false;
    }

    if input.schedule.is_some() || input.tz.is_some() || input.clear_tz == Some(true) {
        let CronSchedule::Cron {
            expr: prior_expr,
            tz: prior_tz,
        } = &prior.schedule
        else {
            return false;
        };
        // PATCH trims a supplied expression before constructing Schedule;
        // preserving an existing expression does not rewrite it.
        let expression = input
            .schedule
            .as_deref()
            .map(str::trim)
            .unwrap_or(prior_expr);
        let timezone = if input.clear_tz == Some(true) {
            None
        } else {
            input.tz.as_deref().or(prior_tz.as_deref())
        };
        if !schedule_matches(job, expression, timezone) {
            return false;
        }
    }

    let content = input.command.as_deref().or(input.prompt.as_deref());
    match (prior.job_type.clone(), content) {
        (CronJobType::Agent, Some(expected)) => job.prompt.as_deref() == Some(expected),
        (CronJobType::Shell, Some(expected)) => job.command == expected,
        (_, None) => true,
    }
}
fn validate_job(job: &CronJobRecord) -> Result<()> {
    validate_job_id(&job.id)?;
    validate_agent(&job.agent_alias)?;
    validate_text("source", &job.source, 32)?;
    ensure!(
        job.allowed_tools
            .as_ref()
            .is_none_or(|tools| tools.len() <= MAX_CRON_TOOL_NAMES),
        "provider job allowed_tools exceeds bound"
    );
    Ok(())
}
fn exact_job(jobs: Vec<CronJobRecord>, id: &str) -> Result<CronJobRecord> {
    let mut matches = jobs.into_iter().filter(|job| job.id == id);
    let job = matches.next().context("cron job not found")?;
    ensure!(
        matches.next().is_none(),
        "tched_router returned duplicate cron job IDs"
    );
    Ok(job)
}

#[cfg(test)]
#[path = "cron_methods_tests.rs"]
mod tests;
