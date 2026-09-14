//! SessionService implementation.

use crate::client::AssistantClient;
use crate::convert::*;
use crate::proto::session_service_server::SessionService;
use crate::proto::{
    CreateSessionRequest, DeleteSessionRequest, Empty, GetSessionHistoryRequest, GetSessionRequest,
    ListSessionsRequest, ListSessionsResponse, Message, SendMessageRequest, Session,
    SessionHistory,
};
use serde_json::{json, Value};
use tonic::{Request, Response, Status};

pub struct SessionServiceImpl {
    client: AssistantClient,
}

impl SessionServiceImpl {
    pub fn new(client: AssistantClient) -> Self {
        Self { client }
    }
}

#[tonic::async_trait]
impl SessionService for SessionServiceImpl {
    async fn list_sessions(
        &self,
        req: Request<ListSessionsRequest>,
    ) -> Result<Response<ListSessionsResponse>, Status> {
        let req = req.into_inner();
        let mut params = json!({
            "limit": req.pagination.as_ref().map(|p| p.limit).unwrap_or(0),
            "offset": req.pagination.as_ref().map(|p| p.offset).unwrap_or(0),
        });
        if let Some(a) = req.agent_id {
            params["agent_id"] = json!(a);
        }
        let result = self.client.call("sessions.list", params).await?;
        let sessions: Vec<Session> = result
            .get("sessions")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().map(session_from_json).collect())
            .unwrap_or_default();
        let total = result
            .get("total")
            .and_then(|t| t.as_u64())
            .unwrap_or(sessions.len() as u64) as u32;
        Ok(Response::new(ListSessionsResponse { sessions, total }))
    }

    async fn get_session(
        &self,
        req: Request<GetSessionRequest>,
    ) -> Result<Response<Session>, Status> {
        let id = req.into_inner().id;
        if id.is_empty() {
            return Err(Status::invalid_argument("session id required"));
        }
        let result = self
            .client
            .call("sessions.get", json!({ "id": id }))
            .await?;
        Ok(Response::new(session_from_json(&result)))
    }

    async fn create_session(
        &self,
        req: Request<CreateSessionRequest>,
    ) -> Result<Response<Session>, Status> {
        let req = req.into_inner();
        let mut params = json!({ "agent_id": req.agent_id });
        if let Some(t) = req.title {
            params["title"] = json!(t);
        }
        if let Some(m) = req.metadata {
            params["metadata"] = struct_to_json(m);
        }
        let result = self.client.call("sessions.create", params).await?;
        Ok(Response::new(session_from_json(&result)))
    }

    async fn delete_session(
        &self,
        req: Request<DeleteSessionRequest>,
    ) -> Result<Response<Empty>, Status> {
        let id = req.into_inner().id;
        if id.is_empty() {
            return Err(Status::invalid_argument("session id required"));
        }
        self.client
            .call("sessions.delete", json!({ "id": id }))
            .await?;
        Ok(Response::new(Empty {}))
    }

    async fn get_session_history(
        &self,
        req: Request<GetSessionHistoryRequest>,
    ) -> Result<Response<SessionHistory>, Status> {
        let req = req.into_inner();
        if req.session_id.is_empty() {
            return Err(Status::invalid_argument("session_id required"));
        }
        let params = json!({
            "session_id": req.session_id,
            "limit": req.pagination.as_ref().map(|p| p.limit).unwrap_or(0),
            "offset": req.pagination.as_ref().map(|p| p.offset).unwrap_or(0),
        });
        let result = self.client.call("sessions.history", params).await?;
        let messages = result
            .get("messages")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().map(message_from_json).collect())
            .unwrap_or_default();
        Ok(Response::new(SessionHistory {
            session_id: req.session_id,
            messages,
        }))
    }

    async fn send_message(
        &self,
        req: Request<SendMessageRequest>,
    ) -> Result<Response<Message>, Status> {
        let req = req.into_inner();
        if req.session_id.is_empty() {
            return Err(Status::invalid_argument("session_id required"));
        }
        let mut params = json!({
            "session_id": req.session_id,
            "content": req.content,
        });
        if let Some(r) = req.role {
            params["role"] = json!(r);
        }
        if let Some(m) = req.metadata {
            params["metadata"] = struct_to_json(m);
        }
        let result = self.client.call("sessions.send_message", params).await?;
        Ok(Response::new(message_from_json(&result)))
    }
}

pub(crate) fn session_from_json(v: &Value) -> Session {
    Session {
        id: str_field(v, "id"),
        agent_id: str_field(v, "agent_id"),
        title: str_field(v, "title"),
        created_at: ts_field(v, "created_at"),
        updated_at: ts_field(v, "updated_at"),
        metadata: opt_struct(v, "metadata"),
    }
}

pub(crate) fn message_from_json(v: &Value) -> Message {
    Message {
        id: str_field(v, "id"),
        session_id: str_field(v, "session_id"),
        role: str_field(v, "role"),
        content: str_field(v, "content"),
        timestamp: ts_field(v, "timestamp"),
        metadata: opt_struct(v, "metadata"),
    }
}
