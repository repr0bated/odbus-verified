//! CronService implementation.

use crate::client::AssistantClient;
use crate::convert::*;
use crate::proto::cron_service_server::CronService;
use crate::proto::{
    CreateCronJobRequest, CronJob, DeleteCronJobRequest, Empty, ListCronJobsRequest,
    ListCronJobsResponse, TriggerCronJobRequest,
};
use serde_json::{json, Value};
use tonic::{Request, Response, Status};

pub struct CronServiceImpl {
    client: AssistantClient,
}

impl CronServiceImpl {
    pub fn new(client: AssistantClient) -> Self {
        Self { client }
    }
}

#[tonic::async_trait]
impl CronService for CronServiceImpl {
    async fn list_cron_jobs(
        &self,
        req: Request<ListCronJobsRequest>,
    ) -> Result<Response<ListCronJobsResponse>, Status> {
        let mut params = json!({});
        if let Some(a) = req.into_inner().agent_id {
            params["agent_id"] = json!(a);
        }
        let result = self.client.call("cron.list", params).await?;
        let jobs = result
            .get("jobs")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().map(cron_job_from_json).collect())
            .unwrap_or_default();
        Ok(Response::new(ListCronJobsResponse { jobs }))
    }

    async fn create_cron_job(
        &self,
        req: Request<CreateCronJobRequest>,
    ) -> Result<Response<CronJob>, Status> {
        let req = req.into_inner();
        let mut params = json!({
            "name": req.name,
            "schedule": req.schedule,
            "agent_id": req.agent_id,
            "task_name": req.task_name,
            "enabled": req.enabled,
        });
        if let Some(p) = req.parameters {
            params["parameters"] = struct_to_json(p);
        }
        let result = self.client.call("cron.create", params).await?;
        Ok(Response::new(cron_job_from_json(&result)))
    }

    async fn delete_cron_job(
        &self,
        req: Request<DeleteCronJobRequest>,
    ) -> Result<Response<Empty>, Status> {
        let id = req.into_inner().id;
        if id.is_empty() {
            return Err(Status::invalid_argument("cron id required"));
        }
        self.client.call("cron.delete", json!({ "id": id })).await?;
        Ok(Response::new(Empty {}))
    }

    async fn trigger_cron_job(
        &self,
        req: Request<TriggerCronJobRequest>,
    ) -> Result<Response<CronJob>, Status> {
        let id = req.into_inner().id;
        if id.is_empty() {
            return Err(Status::invalid_argument("cron id required"));
        }
        let result = self
            .client
            .call("cron.trigger", json!({ "id": id }))
            .await?;
        Ok(Response::new(cron_job_from_json(&result)))
    }
}

fn cron_job_from_json(v: &Value) -> CronJob {
    CronJob {
        id: str_field(v, "id"),
        name: str_field(v, "name"),
        schedule: str_field(v, "schedule"),
        agent_id: str_field(v, "agent_id"),
        task_name: str_field(v, "task_name"),
        enabled: bool_field(v, "enabled"),
        created_at: ts_field(v, "created_at"),
        last_run: ts_field(v, "last_run"),
        next_run: ts_field(v, "next_run"),
        parameters: opt_struct(v, "parameters"),
    }
}
