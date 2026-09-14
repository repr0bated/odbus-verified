//! Exact v0.8.4 cron route mapping over the existing hardened tched-router
//! runtime client. No second reqwest client or bearer authority is introduced.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use op_plugins::state_plugins::cron::*;
use serde::Deserialize;

use crate::cron_methods::{CronGateway, ProviderMutation};
use crate::tched_router_runtime::{
    GatewayJsonRequest, GatewayMethod, GatewayProfile, GatewayRequestError,
    TchedRouterRuntimeClient,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JobsResponse {
    jobs: Vec<CronJobRecord>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunsResponse {
    runs: Vec<CronRunRecord>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JobResponse {
    status: String,
    job: CronJobRecord,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusResponse {
    status: String,
}

pub(crate) struct TchedRouterCronGateway {
    client: TchedRouterRuntimeClient,
}
impl TchedRouterCronGateway {
    pub(crate) fn production() -> Self {
        Self {
            client: TchedRouterRuntimeClient::from_env(),
        }
    }

    pub(crate) fn from_client(client: TchedRouterRuntimeClient) -> Self {
        Self { client }
    }
}

fn encoded_segment(raw: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[(byte >> 4) as usize]));
            out.push(char::from(HEX[(byte & 0x0f) as usize]));
        }
    }
    out
}

fn read_error(error: GatewayRequestError) -> anyhow::Error {
    match error {
        GatewayRequestError::BeforeEffect(_) => {
            anyhow!("tched_router cron request failed before a provider result")
        }
        GatewayRequestError::InvalidResponse(_) => {
            anyhow!("tched_router cron response was unavailable or invalid")
        }
        GatewayRequestError::IndeterminateMutation(_) => {
            anyhow!("unexpected indeterminate classification for cron read")
        }
        GatewayRequestError::ProviderRejected { status, .. } => {
            anyhow!("tched_router rejected cron request with HTTP {status}")
        }
    }
}

fn mutation<T>(result: std::result::Result<T, GatewayRequestError>) -> Result<ProviderMutation<T>> {
    match result {
        Ok(value) => Ok(ProviderMutation::Completed(value)),
        Err(GatewayRequestError::IndeterminateMutation(_)) => Ok(ProviderMutation::Indeterminate(
            "native cron request may have taken effect; observe current state before retrying".into(),
        )),
        Err(GatewayRequestError::BeforeEffect(_)) => {
            Err(anyhow!("tched_router cron mutation failed before submission"))
        }
        Err(GatewayRequestError::ProviderRejected { status, .. }) => {
            Err(anyhow!("tched_router rejected cron mutation with HTTP {status}"))
        }
        // The shared transport promises this only for reads. Fail closed if a
        // future implementation violates that classification.
        Err(GatewayRequestError::InvalidResponse(_)) => Ok(ProviderMutation::Indeterminate(
            "native cron response was invalid after mutation submission; observe current state before retrying".into(),
        )),
    }
}

fn completed_job(
    result: std::result::Result<JobResponse, GatewayRequestError>,
) -> Result<ProviderMutation<CronJobRecord>> {
    match mutation(result)? {
        ProviderMutation::Completed(response) => {
            if response.status == "ok" {
                Ok(ProviderMutation::Completed(response.job))
            } else {
                Ok(ProviderMutation::Indeterminate(
                    "native cron response did not establish mutation completion; observe current state before retrying".into(),
                ))
            }
        }
        ProviderMutation::Indeterminate(detail) => Ok(ProviderMutation::Indeterminate(detail)),
    }
}

#[async_trait]
impl CronGateway for TchedRouterCronGateway {
    async fn list_jobs(&self) -> Result<Vec<CronJobRecord>> {
        let response: JobsResponse = self
            .client
            .send_gateway_json(GatewayJsonRequest {
                method: GatewayMethod::Get,
                relative_path: "/api/cron",
                query: &[],
                body: None,
                operation: "list cron jobs",
                profile: GatewayProfile::Config,
                mutation: false,
            })
            .await
            .map_err(read_error)?;
        Ok(response.jobs)
    }

    async fn list_runs(&self, job_id: &str, limit: u32) -> Result<Vec<CronRunRecord>> {
        let path = format!("/api/cron/{}/runs", encoded_segment(job_id));
        let limit = limit.to_string();
        let response: RunsResponse = self
            .client
            .send_gateway_json(GatewayJsonRequest {
                method: GatewayMethod::Get,
                relative_path: &path,
                query: &[("limit", limit.as_str())],
                body: None,
                operation: "list cron history",
                profile: GatewayProfile::Config,
                mutation: false,
            })
            .await
            .map_err(read_error)?;
        Ok(response.runs)
    }

    async fn create_job(&self, input: CreateJobInput) -> Result<ProviderMutation<CronJobRecord>> {
        let body = serde_json::to_value(input)?;
        completed_job(
            self.client
                .send_gateway_json(GatewayJsonRequest {
                    method: GatewayMethod::Post,
                    relative_path: "/api/cron",
                    query: &[],
                    body: Some(&body),
                    operation: "create cron job",
                    profile: GatewayProfile::Config,
                    mutation: true,
                })
                .await,
        )
    }

    async fn update_job(&self, input: UpdateJobInput) -> Result<ProviderMutation<CronJobRecord>> {
        let path = format!("/api/cron/{}", encoded_segment(&input.job_id));
        let mut body = serde_json::to_value(&input)?;
        let object = body
            .as_object_mut()
            .expect("typed update serializes as object");
        object.remove("job_id");
        object.retain(|_, value| !value.is_null());
        completed_job(
            self.client
                .send_gateway_json(GatewayJsonRequest {
                    method: GatewayMethod::Patch,
                    relative_path: &path,
                    query: &[],
                    body: Some(&body),
                    operation: "update cron job",
                    profile: GatewayProfile::Config,
                    mutation: true,
                })
                .await,
        )
    }

    async fn delete_job(&self, job_id: &str) -> Result<ProviderMutation<()>> {
        let path = format!("/api/cron/{}", encoded_segment(job_id));
        match mutation::<StatusResponse>(
            self.client
                .send_gateway_json(GatewayJsonRequest {
                    method: GatewayMethod::Delete,
                    relative_path: &path,
                    query: &[],
                    body: None,
                    operation: "delete cron job",
                    profile: GatewayProfile::Config,
                    mutation: true,
                })
                .await,
        )? {
            ProviderMutation::Completed(response) => {
                if response.status == "ok" {
                    Ok(ProviderMutation::Completed(()))
                } else {
                    Ok(ProviderMutation::Indeterminate(
                        "native cron response did not establish delete completion; observe current state before retrying".into(),
                    ))
                }
            }
            ProviderMutation::Indeterminate(detail) => Ok(ProviderMutation::Indeterminate(detail)),
        }
    }

    async fn run_job(&self, job_id: &str) -> Result<ProviderMutation<ManualRunResult>> {
        let path = format!("/api/cron/{}/run", encoded_segment(job_id));
        mutation(
            self.client
                .send_gateway_json(GatewayJsonRequest {
                    method: GatewayMethod::Post,
                    relative_path: &path,
                    query: &[],
                    body: None,
                    operation: "run cron job",
                    profile: GatewayProfile::LongRunning,
                    mutation: true,
                })
                .await,
        )
    }
}

#[cfg(test)]
#[path = "cron_gateway_tests.rs"]
mod tests;
