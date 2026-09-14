//! MemoryService implementation.
//!
//! Production: D-Bus `PluginV1.Call` on the cognitive_mcp plugin (the Cozo
//! owner). Tests: in-process `CognitiveMemoryStore` over in-memory Cozo.

use crate::cognitive_client::MemoryBackend;
use crate::convert::*;
use crate::proto::memory_service_server::MemoryService;
use crate::proto::{
    DeleteMemoryRequest, DeleteMemoryResponse, GetMemoryStatsRequest, MemoryEntry, MemoryStats,
    ReadMemoryRequest, ReadMemoryResponse, SearchMemoryRequest, SearchMemoryResponse,
    WriteMemoryRequest, WriteMemoryResponse,
};
use op_cognitive_mcp::memory_store::{
    CognitiveMemoryStore, EntryQuery, MemoryEntry as StoreEntry, NamespaceKind,
};
use serde_json::Value;
use std::sync::Arc;
use tonic::{Request, Response, Status};

pub struct MemoryServiceImpl {
    backend: Arc<MemoryBackend>,
}

impl MemoryServiceImpl {
    /// In-process store — unit/integration tests only.
    pub fn new(store: Arc<CognitiveMemoryStore>) -> Self {
        Self {
            backend: Arc::new(MemoryBackend::Local(store)),
        }
    }

    pub fn from_client(client: Arc<crate::cognitive_client::CognitivePluginClient>) -> Self {
        Self {
            backend: Arc::new(MemoryBackend::Remote(client)),
        }
    }
}

#[tonic::async_trait]
impl MemoryService for MemoryServiceImpl {
    async fn read_memory(
        &self,
        req: Request<ReadMemoryRequest>,
    ) -> Result<Response<ReadMemoryResponse>, Status> {
        let req = req.into_inner();
        if req.namespace.is_empty() {
            return Err(Status::invalid_argument("namespace required"));
        }

        let entries = if req.keys.is_empty() {
            let q = EntryQuery {
                namespace_id: Some(req.namespace.clone()),
                key_pattern: None,
                tags: None,
                limit: req.pagination.as_ref().map(|p| p.limit as i64),
                offset: req.pagination.as_ref().map(|p| p.offset as i64),
            };
            self.backend
                .query_entries(q)
                .await
                .map_err(|e| Status::internal(e.to_string()))?
        } else {
            let mut out = Vec::with_capacity(req.keys.len());
            for k in &req.keys {
                if let Some(e) = self
                    .backend
                    .retrieve_entry(&req.namespace, k)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?
                {
                    out.push(e);
                }
            }
            out
        };

        Ok(Response::new(ReadMemoryResponse {
            entries: entries.iter().map(entry_to_proto).collect(),
        }))
    }

    async fn write_memory(
        &self,
        req: Request<WriteMemoryRequest>,
    ) -> Result<Response<WriteMemoryResponse>, Status> {
        let req = req.into_inner();
        if req.namespace.is_empty() {
            return Err(Status::invalid_argument("namespace required"));
        }

        self.backend
            .ensure_namespace(&req.namespace, NamespaceKind::Custom)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let mut written = 0u32;
        for entry in req.entries {
            let value: Value =
                serde_json::from_str(&entry.value).unwrap_or(Value::String(entry.value));
            let tags = tags_from_metadata(&entry.metadata);
            self.backend
                .store_entry(&req.namespace, &entry.key, value, tags)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            written += 1;
        }
        Ok(Response::new(WriteMemoryResponse { written }))
    }

    async fn delete_memory(
        &self,
        req: Request<DeleteMemoryRequest>,
    ) -> Result<Response<DeleteMemoryResponse>, Status> {
        let req = req.into_inner();
        if req.namespace.is_empty() {
            return Err(Status::invalid_argument("namespace required"));
        }
        let mut deleted = 0u32;
        for k in req.keys {
            if self
                .backend
                .delete_entry(&req.namespace, &k)
                .await
                .map_err(|e| Status::internal(e.to_string()))?
            {
                deleted += 1;
            }
        }
        Ok(Response::new(DeleteMemoryResponse { deleted }))
    }

    async fn search_memory(
        &self,
        req: Request<SearchMemoryRequest>,
    ) -> Result<Response<SearchMemoryResponse>, Status> {
        let req = req.into_inner();
        if req.query.is_empty() {
            return Err(Status::invalid_argument("query required"));
        }
        let limit = if req.limit == 0 { 50 } else { req.limit as i64 };

        let namespaces = if req.namespaces.is_empty() {
            vec![None]
        } else {
            req.namespaces.iter().map(|n| Some(n.clone())).collect()
        };

        let mut out = Vec::new();
        for ns in namespaces {
            let q = EntryQuery {
                namespace_id: ns,
                key_pattern: Some(req.query.clone()),
                tags: None,
                limit: Some(limit),
                offset: None,
            };
            let rows = self
                .backend
                .query_entries(q)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            out.extend(rows.iter().map(entry_to_proto));
        }
        out.truncate(limit as usize);
        Ok(Response::new(SearchMemoryResponse { entries: out }))
    }

    async fn get_memory_stats(
        &self,
        req: Request<GetMemoryStatsRequest>,
    ) -> Result<Response<MemoryStats>, Status> {
        let ns = req.into_inner().namespace;
        if ns.is_empty() {
            return Err(Status::invalid_argument("namespace required"));
        }
        let q = EntryQuery {
            namespace_id: Some(ns.clone()),
            ..Default::default()
        };
        let entries = self
            .backend
            .query_entries(q)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let bytes_used: u64 = entries
            .iter()
            .map(|e| {
                e.key.len() as u64
                    + serde_json::to_string(&e.value)
                        .map(|s| s.len() as u64)
                        .unwrap_or(0)
            })
            .sum();
        let last_updated =
            entries
                .iter()
                .map(|e| e.updated_at)
                .max()
                .map(|t| prost_types::Timestamp {
                    seconds: t.timestamp(),
                    nanos: t.timestamp_subsec_nanos() as i32,
                });

        Ok(Response::new(MemoryStats {
            namespace: ns,
            entry_count: entries.len() as u64,
            bytes_used,
            last_updated,
        }))
    }
}

pub(crate) fn entry_to_proto(e: &StoreEntry) -> MemoryEntry {
    MemoryEntry {
        id: e.id.clone(),
        namespace: e.namespace_id.clone(),
        key: e.key.clone(),
        value: serde_json::to_string(&e.value).unwrap_or_default(),
        metadata: if e.tags.is_empty() {
            None
        } else {
            Some(json_to_struct(serde_json::json!({ "tags": e.tags })))
        },
        created_at: Some(prost_types::Timestamp {
            seconds: e.created_at.timestamp(),
            nanos: e.created_at.timestamp_subsec_nanos() as i32,
        }),
        updated_at: Some(prost_types::Timestamp {
            seconds: e.updated_at.timestamp(),
            nanos: e.updated_at.timestamp_subsec_nanos() as i32,
        }),
    }
}

fn tags_from_metadata(meta: &Option<prost_types::Struct>) -> Vec<String> {
    let Some(m) = meta else { return Vec::new() };
    let json = struct_to_json(m.clone());
    json.get("tags")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// In-process helper used by integration tests.
pub async fn ensure_namespace(
    store: &CognitiveMemoryStore,
    name: &str,
    kind: NamespaceKind,
) -> Result<(), Status> {
    crate::cognitive_client::local_ensure_namespace(store, name, kind)
        .await
        .map_err(|e| Status::internal(e.to_string()))
}
