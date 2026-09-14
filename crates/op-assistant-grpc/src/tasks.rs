//! TaskService implementation.

use crate::client::AssistantClient;
use crate::convert::*;
use crate::proto::task_service_server::TaskService;
use crate::proto::{
    ExecuteTaskRequest, GetTaskResultRequest, ListToolsRequest, ListToolsResponse,
    StreamTaskExecutionRequest, TaskEvent, TaskResult, Tool,
};
use async_stream::try_stream;
use futures::Stream;
use serde_json::{json, Value};
use std::pin::Pin;
use tonic::{Request, Response, Status};

pub struct TaskServiceImpl {
    client: AssistantClient,
}

impl TaskServiceImpl {
    pub fn new(client: AssistantClient) -> Self {
        Self { client }
    }
}

type TaskEventStream = Pin<Box<dyn Stream<Item = Result<TaskEvent, Status>> + Send>>;

#[tonic::async_trait]
impl TaskService for TaskServiceImpl {
    type StreamTaskExecutionStream = TaskEventStream;

    async fn list_tools(
        &self,
        req: Request<ListToolsRequest>,
    ) -> Result<Response<ListToolsResponse>, Status> {
        let result = self
            .client
            .call("tools.list", json!({ "filter": req.into_inner().filter }))
            .await?;
        let tools = result
            .get("tools")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().map(tool_from_json).collect())
            .unwrap_or_default();
        Ok(Response::new(ListToolsResponse { tools }))
    }

    async fn execute_task(
        &self,
        req: Request<ExecuteTaskRequest>,
    ) -> Result<Response<TaskResult>, Status> {
        let req = req.into_inner();
        if req.tool_name.is_empty() {
            return Err(Status::invalid_argument("tool_name required"));
        }
        let mut params = json!({ "tool_name": req.tool_name });
        if let Some(a) = req.arguments {
            params["arguments"] = struct_to_json(a);
        }
        if let Some(s) = req.session_id {
            params["session_id"] = json!(s);
        }
        if let Some(a) = req.agent_id {
            params["agent_id"] = json!(a);
        }
        let result = self.client.call("tasks.execute", params).await?;
        Ok(Response::new(task_result_from_json(&result)))
    }

    async fn stream_task_execution(
        &self,
        req: Request<StreamTaskExecutionRequest>,
    ) -> Result<Response<Self::StreamTaskExecutionStream>, Status> {
        let task_id = req.into_inner().task_id;
        if task_id.is_empty() {
            return Err(Status::invalid_argument("task_id required"));
        }
        let client = self.client.clone();
        let stream = try_stream! {
            let result = client
                .call("tasks.events", json!({ "task_id": task_id }))
                .await
                .map_err(Status::from)?;
            let events = result.get("events").and_then(|v| v.as_array()).cloned().unwrap_or_default();
            for evt in events {
                yield task_event_from_json(&task_id, &evt);
            }
        };
        Ok(Response::new(Box::pin(stream)))
    }

    async fn get_task_result(
        &self,
        req: Request<GetTaskResultRequest>,
    ) -> Result<Response<TaskResult>, Status> {
        let task_id = req.into_inner().task_id;
        if task_id.is_empty() {
            return Err(Status::invalid_argument("task_id required"));
        }
        let result = self
            .client
            .call("tasks.get_result", json!({ "task_id": task_id }))
            .await?;
        Ok(Response::new(task_result_from_json(&result)))
    }
}

fn tool_from_json(v: &Value) -> Tool {
    Tool {
        name: str_field(v, "name"),
        description: str_field(v, "description"),
        version: str_field(v, "version"),
        schema: opt_struct(v, "schema"),
    }
}

fn task_result_from_json(v: &Value) -> TaskResult {
    TaskResult {
        task_id: str_field(v, "task_id"),
        status: str_field(v, "status"),
        output: opt_struct(v, "output"),
        error: opt_str(v, "error"),
        started_at: ts_field(v, "started_at"),
        finished_at: ts_field(v, "finished_at"),
    }
}

fn task_event_from_json(task_id: &str, v: &Value) -> TaskEvent {
    TaskEvent {
        task_id: task_id.to_string(),
        event_type: str_field(v, "event_type"),
        payload_json: v
            .get("payload")
            .map(|p| p.to_string())
            .unwrap_or_else(|| v.to_string()),
        timestamp: ts_field(v, "timestamp"),
    }
}
