//! SoulService — persistent agent identity.
//!
//! Production: D-Bus `PluginV1.Call` on the cognitive_mcp plugin (the Cozo
//! owner). Tests: in-process `SoulMemoryStore` over in-memory Cozo.

use crate::cognitive_client::SoulBackend;
use crate::convert::*;
use crate::proto::soul_service_server::SoulService;
use crate::proto::{
    DeleteSoulMemoryRequest, Empty, GetSoulMemoryRequest, ListSoulMemoriesRequest,
    ListSoulMemoriesResponse, SoulMemory, UpdateSoulMemoryRequest,
};
use op_cognitive_mcp::soul_memory::{SoulMemory as StoreSoul, SoulMemoryStore, SoulUpdate};
use std::sync::Arc;
use tonic::{Request, Response, Status};

pub struct SoulServiceImpl {
    backend: Arc<SoulBackend>,
}

impl SoulServiceImpl {
    /// In-process store — unit/integration tests only.
    pub fn new(store: Arc<SoulMemoryStore>) -> Self {
        Self {
            backend: Arc::new(SoulBackend::Local(store)),
        }
    }

    pub fn from_client(client: Arc<crate::cognitive_client::CognitivePluginClient>) -> Self {
        Self {
            backend: Arc::new(SoulBackend::Remote(client)),
        }
    }
}

#[tonic::async_trait]
impl SoulService for SoulServiceImpl {
    async fn get_soul_memory(
        &self,
        req: Request<GetSoulMemoryRequest>,
    ) -> Result<Response<SoulMemory>, Status> {
        let id = req.into_inner().agent_id;
        if id.is_empty() {
            return Err(Status::invalid_argument("agent_id required"));
        }
        let soul = self
            .backend
            .get_soul(&id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("soul memory not found"))?;
        Ok(Response::new(soul_to_proto(&soul)))
    }

    async fn update_soul_memory(
        &self,
        req: Request<UpdateSoulMemoryRequest>,
    ) -> Result<Response<SoulMemory>, Status> {
        let req = req.into_inner();
        if req.agent_id.is_empty() {
            return Err(Status::invalid_argument("agent_id required"));
        }
        let update = SoulUpdate {
            identity: req.identity,
            personality: req.personality,
            traits: req.traits.map(struct_to_json),
        };
        let soul = self
            .backend
            .upsert_soul(&req.agent_id, update)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(soul_to_proto(&soul)))
    }

    async fn delete_soul_memory(
        &self,
        req: Request<DeleteSoulMemoryRequest>,
    ) -> Result<Response<Empty>, Status> {
        let id = req.into_inner().agent_id;
        if id.is_empty() {
            return Err(Status::invalid_argument("agent_id required"));
        }
        self.backend
            .delete_soul(&id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(Empty {}))
    }

    async fn list_soul_memories(
        &self,
        req: Request<ListSoulMemoriesRequest>,
    ) -> Result<Response<ListSoulMemoriesResponse>, Status> {
        let req = req.into_inner();
        let limit = req
            .pagination
            .as_ref()
            .map(|p| p.limit as usize)
            .unwrap_or(0);
        let offset = req
            .pagination
            .as_ref()
            .map(|p| p.offset as usize)
            .unwrap_or(0);

        let souls = self
            .backend
            .list_souls()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let total = souls.len() as u32;
        let page: Vec<SoulMemory> = souls
            .iter()
            .skip(offset)
            .take(if limit == 0 { usize::MAX } else { limit })
            .map(soul_to_proto)
            .collect();
        Ok(Response::new(ListSoulMemoriesResponse {
            memories: page,
            total,
        }))
    }
}

fn soul_to_proto(s: &StoreSoul) -> SoulMemory {
    SoulMemory {
        agent_id: s.agent_id.clone(),
        identity: s.identity.clone(),
        personality: s.personality.clone(),
        traits: Some(json_to_struct(s.traits.clone())),
        version: s.version as u64,
        created_at: Some(prost_types::Timestamp {
            seconds: s.created_at.timestamp(),
            nanos: s.created_at.timestamp_subsec_nanos() as i32,
        }),
        updated_at: Some(prost_types::Timestamp {
            seconds: s.updated_at.timestamp(),
            nanos: s.updated_at.timestamp_subsec_nanos() as i32,
        }),
    }
}
