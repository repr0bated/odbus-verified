//! AgentService implementation. Proxies all calls into the Assistant via the
//! [`AssistantClient`] and converts JSON responses into proto messages.

use crate::client::AssistantClient;
use crate::convert::*;
use crate::proto::agent_service_server::AgentService;
use crate::proto::{
    Agent, CancelRunRequest, CreateAgentRequest, DeleteAgentRequest, Empty, GetAgentRequest,
    ListAgentsRequest, ListAgentsResponse, Run, RunEvent, StartRunRequest, StreamRunEventsRequest,
    UpdateAgentRequest,
};
use async_stream::try_stream;
use futures::Stream;
use serde_json::{json, Value};
use std::pin::Pin;
use tonic::{Request, Response, Status};

pub struct AgentServiceImpl {
    client: AssistantClient,
}

impl AgentServiceImpl {
    pub fn new(client: AssistantClient) -> Self {
        Self { client }
    }
}

type RunEventStream = Pin<Box<dyn Stream<Item = Result<RunEvent, Status>> + Send>>;

#[tonic::async_trait]
impl AgentService for AgentServiceImpl {
    type StreamRunEventsStream = RunEventStream;

    async fn list_agents(
        &self,
        req: Request<ListAgentsRequest>,
    ) -> Result<Response<ListAgentsResponse>, Status> {
        let req = req.into_inner();
        let params = json!({
            "limit": req.pagination.as_ref().map(|p| p.limit).unwrap_or(0),
            "offset": req.pagination.as_ref().map(|p| p.offset).unwrap_or(0),
            "filter": req.filter,
        });
        let result = self.client.call("agents.list", params).await?;
        let agents = result
            .get("agents")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().map(agent_from_json).collect::<Vec<_>>())
            .unwrap_or_default();
        let total = result
            .get("total")
            .and_then(|t| t.as_u64())
            .unwrap_or(agents.len() as u64) as u32;
        Ok(Response::new(ListAgentsResponse { agents, total }))
    }

    async fn get_agent(&self, req: Request<GetAgentRequest>) -> Result<Response<Agent>, Status> {
        let id = req.into_inner().id;
        if id.is_empty() {
            return Err(Status::invalid_argument("agent id is required"));
        }
        let result = self.client.call("agents.get", json!({ "id": id })).await?;
        Ok(Response::new(agent_from_json(&result)))
    }

    async fn create_agent(
        &self,
        req: Request<CreateAgentRequest>,
    ) -> Result<Response<Agent>, Status> {
        let req = req.into_inner();
        let mut params = json!({
            "name": req.name,
            "description": req.description,
            "model": req.model,
            "system_prompt": req.system_prompt,
            "tools": req.tools,
        });
        if let Some(meta) = req.metadata {
            params["metadata"] = struct_to_json(meta);
        }
        let result = self.client.call("agents.create", params).await?;
        Ok(Response::new(agent_from_json(&result)))
    }

    async fn update_agent(
        &self,
        req: Request<UpdateAgentRequest>,
    ) -> Result<Response<Agent>, Status> {
        let req = req.into_inner();
        if req.id.is_empty() {
            return Err(Status::invalid_argument("agent id is required"));
        }
        let mut params = json!({ "id": req.id, "tools": req.tools });
        if let Some(v) = req.name {
            params["name"] = json!(v);
        }
        if let Some(v) = req.description {
            params["description"] = json!(v);
        }
        if let Some(v) = req.model {
            params["model"] = json!(v);
        }
        if let Some(v) = req.system_prompt {
            params["system_prompt"] = json!(v);
        }
        if let Some(meta) = req.metadata {
            params["metadata"] = struct_to_json(meta);
        }
        let result = self.client.call("agents.update", params).await?;
        Ok(Response::new(agent_from_json(&result)))
    }

    async fn delete_agent(
        &self,
        req: Request<DeleteAgentRequest>,
    ) -> Result<Response<Empty>, Status> {
        let id = req.into_inner().id;
        if id.is_empty() {
            return Err(Status::invalid_argument("agent id is required"));
        }
        self.client
            .call("agents.delete", json!({ "id": id }))
            .await?;
        Ok(Response::new(Empty {}))
    }

    async fn start_run(&self, req: Request<StartRunRequest>) -> Result<Response<Run>, Status> {
        let req = req.into_inner();
        let mut params = json!({
            "agent_id": req.agent_id,
            "input": req.input,
        });
        if let Some(sid) = req.session_id {
            params["session_id"] = json!(sid);
        }
        if let Some(p) = req.parameters {
            params["parameters"] = struct_to_json(p);
        }
        let result = self.client.call("agents.start_run", params).await?;
        Ok(Response::new(run_from_json(&result)))
    }

    async fn stream_run_events(
        &self,
        req: Request<StreamRunEventsRequest>,
    ) -> Result<Response<Self::StreamRunEventsStream>, Status> {
        let run_id = req.into_inner().run_id;
        if run_id.is_empty() {
            return Err(Status::invalid_argument("run_id is required"));
        }
        let client = self.client.clone();
        let stream = try_stream! {
            let result = client
                .call("agents.run_events", json!({ "run_id": run_id }))
                .await
                .map_err(Status::from)?;
            let events = result
                .get("events")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            for evt in events {
                yield run_event_from_json(&run_id, &evt);
            }
        };
        Ok(Response::new(Box::pin(stream)))
    }

    async fn cancel_run(&self, req: Request<CancelRunRequest>) -> Result<Response<Empty>, Status> {
        let run_id = req.into_inner().run_id;
        if run_id.is_empty() {
            return Err(Status::invalid_argument("run_id is required"));
        }
        self.client
            .call("agents.cancel_run", json!({ "run_id": run_id }))
            .await?;
        Ok(Response::new(Empty {}))
    }
}

pub(crate) fn agent_from_json(v: &Value) -> Agent {
    Agent {
        id: str_field(v, "id"),
        name: str_field(v, "name"),
        description: str_field(v, "description"),
        model: str_field(v, "model"),
        system_prompt: str_field(v, "system_prompt"),
        tools: string_list(v, "tools"),
        created_at: ts_field(v, "created_at"),
        updated_at: ts_field(v, "updated_at"),
        metadata: opt_struct(v, "metadata"),
    }
}

pub(crate) fn run_from_json(v: &Value) -> Run {
    Run {
        id: str_field(v, "id"),
        agent_id: str_field(v, "agent_id"),
        session_id: str_field(v, "session_id"),
        status: str_field(v, "status"),
        started_at: ts_field(v, "started_at"),
        finished_at: ts_field(v, "finished_at"),
        error: opt_str(v, "error"),
    }
}

pub(crate) fn run_event_from_json(run_id: &str, v: &Value) -> RunEvent {
    RunEvent {
        run_id: run_id.to_string(),
        event_type: str_field(v, "event_type"),
        payload_json: v
            .get("payload")
            .map(|p| p.to_string())
            .unwrap_or_else(|| v.to_string()),
        timestamp: ts_field(v, "timestamp"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_agent_from_json() {
        let v = json!({
            "id": "abc",
            "name": "Agent X",
            "tools": ["search", "code"],
        });
        let a = agent_from_json(&v);
        assert_eq!(a.id, "abc");
        assert_eq!(a.name, "Agent X");
        assert_eq!(a.tools, vec!["search", "code"]);
    }
}
