//! NamespaceMemoryService — agent → namespace mapping plus the namespace
//! itself.
//!
//! Production: D-Bus `PluginV1.Call` on the cognitive_mcp plugin (the Cozo
//! owner). Tests: in-process `CognitiveMemoryStore` + `SoulMemoryStore`.

use crate::cognitive_client::{MemoryBackend, SoulBackend};
use crate::proto::namespace_memory_service_server::NamespaceMemoryService;
use crate::proto::{
    ClearMemoryNamespaceRequest, Empty, GetMemoryNamespaceRequest, ListMemoryNamespacesRequest,
    ListMemoryNamespacesResponse, MemoryNamespace, SetMemoryNamespaceRequest,
};
use op_cognitive_mcp::memory_store::{CognitiveMemoryStore, EntryQuery, NamespaceKind};
use op_cognitive_mcp::soul_memory::{AgentNamespaceBinding, SoulMemoryStore};
use std::sync::Arc;
use tonic::{Request, Response, Status};

pub struct NamespaceMemoryServiceImpl {
    memory: Arc<MemoryBackend>,
    bindings: Arc<SoulBackend>,
}

impl NamespaceMemoryServiceImpl {
    /// In-process stores — unit/integration tests only.
    pub fn new(memory: Arc<CognitiveMemoryStore>, bindings: Arc<SoulMemoryStore>) -> Self {
        Self {
            memory: Arc::new(MemoryBackend::Local(memory)),
            bindings: Arc::new(SoulBackend::Local(bindings)),
        }
    }

    pub fn from_client(client: Arc<crate::cognitive_client::CognitivePluginClient>) -> Self {
        Self {
            memory: Arc::new(MemoryBackend::Remote(client.clone())),
            bindings: Arc::new(SoulBackend::Remote(client)),
        }
    }
}

#[tonic::async_trait]
impl NamespaceMemoryService for NamespaceMemoryServiceImpl {
    async fn get_memory_namespace(
        &self,
        req: Request<GetMemoryNamespaceRequest>,
    ) -> Result<Response<MemoryNamespace>, Status> {
        let agent = req.into_inner().agent_id;
        if agent.is_empty() {
            return Err(Status::invalid_argument("agent_id required"));
        }
        let binding = self
            .bindings
            .get_binding(&agent)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("no namespace bound for agent"))?;
        let count = count_entries(&self.memory, &binding.namespace).await;
        Ok(Response::new(binding_to_proto(&binding, count)))
    }

    async fn set_memory_namespace(
        &self,
        req: Request<SetMemoryNamespaceRequest>,
    ) -> Result<Response<MemoryNamespace>, Status> {
        let req = req.into_inner();
        if req.agent_id.is_empty() || req.namespace.is_empty() {
            return Err(Status::invalid_argument("agent_id and namespace required"));
        }
        self.memory
            .ensure_namespace(&req.namespace, NamespaceKind::Agent)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let binding = self
            .bindings
            .bind_namespace(&req.agent_id, &req.namespace)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let count = count_entries(&self.memory, &binding.namespace).await;
        Ok(Response::new(binding_to_proto(&binding, count)))
    }

    async fn clear_memory_namespace(
        &self,
        req: Request<ClearMemoryNamespaceRequest>,
    ) -> Result<Response<Empty>, Status> {
        let agent = req.into_inner().agent_id;
        if agent.is_empty() {
            return Err(Status::invalid_argument("agent_id required"));
        }
        if let Some(binding) = self
            .bindings
            .get_binding(&agent)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
        {
            self.memory
                .delete_namespace(&binding.namespace)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            self.bindings
                .clear_binding(&agent)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
        }
        Ok(Response::new(Empty {}))
    }

    async fn list_memory_namespaces(
        &self,
        _req: Request<ListMemoryNamespacesRequest>,
    ) -> Result<Response<ListMemoryNamespacesResponse>, Status> {
        let bindings = self
            .bindings
            .list_bindings()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let mut out = Vec::with_capacity(bindings.len());
        for b in &bindings {
            let count = count_entries(&self.memory, &b.namespace).await;
            out.push(binding_to_proto(b, count));
        }
        let total = out.len() as u32;
        Ok(Response::new(ListMemoryNamespacesResponse {
            namespaces: out,
            total,
        }))
    }
}

fn binding_to_proto(b: &AgentNamespaceBinding, entry_count: u64) -> MemoryNamespace {
    MemoryNamespace {
        agent_id: b.agent_id.clone(),
        namespace: b.namespace.clone(),
        entry_count,
        created_at: Some(prost_types::Timestamp {
            seconds: b.created_at.timestamp(),
            nanos: b.created_at.timestamp_subsec_nanos() as i32,
        }),
        updated_at: Some(prost_types::Timestamp {
            seconds: b.updated_at.timestamp(),
            nanos: b.updated_at.timestamp_subsec_nanos() as i32,
        }),
    }
}

async fn count_entries(store: &MemoryBackend, ns: &str) -> u64 {
    store
        .query_entries(EntryQuery {
            namespace_id: Some(ns.into()),
            ..Default::default()
        })
        .await
        .map(|e| e.len() as u64)
        .unwrap_or(0)
}
