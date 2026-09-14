//! ModelService implementation.

use crate::client::AssistantClient;
use crate::convert::*;
use crate::proto::model_service_server::ModelService;
use crate::proto::{
    GetModelRequest, ListModelsRequest, ListModelsResponse, Model, SwitchModelRequest,
};
use serde_json::{json, Value};
use tonic::{Request, Response, Status};

pub struct ModelServiceImpl {
    client: AssistantClient,
}

impl ModelServiceImpl {
    pub fn new(client: AssistantClient) -> Self {
        Self { client }
    }
}

#[tonic::async_trait]
impl ModelService for ModelServiceImpl {
    async fn list_models(
        &self,
        req: Request<ListModelsRequest>,
    ) -> Result<Response<ListModelsResponse>, Status> {
        let result = self
            .client
            .call("models.list", json!({ "filter": req.into_inner().filter }))
            .await?;
        let models = result
            .get("models")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().map(model_from_json).collect())
            .unwrap_or_default();
        Ok(Response::new(ListModelsResponse { models }))
    }

    async fn get_model(&self, req: Request<GetModelRequest>) -> Result<Response<Model>, Status> {
        let id = req.into_inner().id;
        if id.is_empty() {
            return Err(Status::invalid_argument("model id required"));
        }
        let result = self.client.call("models.get", json!({ "id": id })).await?;
        Ok(Response::new(model_from_json(&result)))
    }

    async fn switch_model(
        &self,
        req: Request<SwitchModelRequest>,
    ) -> Result<Response<Model>, Status> {
        let req = req.into_inner();
        let mut params = json!({ "model_id": req.model_id });
        if let Some(a) = req.agent_id {
            params["agent_id"] = json!(a);
        }
        if let Some(s) = req.session_id {
            params["session_id"] = json!(s);
        }
        let result = self.client.call("models.switch", params).await?;
        Ok(Response::new(model_from_json(&result)))
    }
}

fn model_from_json(v: &Value) -> Model {
    Model {
        id: str_field(v, "id"),
        name: str_field(v, "name"),
        provider: str_field(v, "provider"),
        family: str_field(v, "family"),
        context_window: u32_field(v, "context_window"),
        active: bool_field(v, "active"),
        capabilities: opt_struct(v, "capabilities"),
    }
}
