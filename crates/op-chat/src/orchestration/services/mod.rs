//! Orchestration gRPC Services
//!
//! Implements the generated gRPC service traits for agent orchestration.
//! All state is backed by in-memory data structures behind `Arc<RwLock<..>>`.

pub mod agent_execution;
pub mod agent_lifecycle;
pub mod backend_architect;
pub mod context_manager;
pub mod memory_service;
pub mod rust_pro;
pub mod sequential_thinking;
pub mod workstack;

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{broadcast, RwLock};
use tonic_reflection;
use tonic_web;

use super::grpc_pool::GrpcAgentPool;
use super::proto::op_chat_orchestration::AgentStatusEvent;
use super::workstack_executor::WorkstackExecutor;

// ============================================================================
// SUPPORTING TYPES
// ============================================================================

/// In-memory entry for the memory service
#[derive(Debug, Clone)]
pub struct MemoryEntry {
    pub key: String,
    pub value: String,
    pub tags: Vec<String>,
    pub created_at: i64,
    pub accessed_at: i64,
    pub access_count: i32,
    pub expires_at: Option<i64>,
}

/// A thinking chain for the sequential thinking service
#[derive(Debug, Clone)]
pub struct ThinkingChain {
    pub id: String,
    pub problem: String,
    pub thoughts: Vec<ThoughtEntry>,
    pub status: ChainStatusInternal,
    pub max_steps: i32,
    pub conclusion: Option<String>,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub context: String,
    pub style: i32,
    /// Broadcast sender for streaming thought events on this chain
    pub event_tx: broadcast::Sender<super::proto::op_chat_orchestration::ThoughtEvent>,
}

/// A single thought within a chain
#[derive(Debug, Clone)]
pub struct ThoughtEntry {
    pub step: i32,
    pub thought: String,
    pub thought_type: i32,
    pub confidence: f32,
    pub timestamp_ms: i64,
}

/// Internal chain status (mirrors proto but as a Rust enum)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainStatusInternal {
    Thinking,
    Analyzing,
    Concluding,
    Complete,
    Stuck,
    Error,
}

impl ChainStatusInternal {
    pub fn to_proto(self) -> i32 {
        match self {
            ChainStatusInternal::Thinking => 1,
            ChainStatusInternal::Analyzing => 2,
            ChainStatusInternal::Concluding => 3,
            ChainStatusInternal::Complete => 4,
            ChainStatusInternal::Stuck => 5,
            ChainStatusInternal::Error => 6,
        }
    }
}

/// Context entry for context management
#[derive(Debug, Clone)]
pub struct ContextEntry {
    pub name: String,
    pub content: String,
    pub tags: Vec<String>,
    pub context_type: i32,
    pub created_at: i64,
    pub updated_at: i64,
    pub version: String,
}

/// Session tracking info
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_id: String,
    pub client_name: String,
    pub started_at: i64,
    pub agent_ids: Vec<String>,
    pub metadata: HashMap<String, String>,
}

// ============================================================================
// ORCHESTRATION SERVER
// ============================================================================

/// Shared server state for all orchestration gRPC services.
///
/// A single `OrchestrationServer` instance is shared (via `Arc`) across the
/// `AgentLifecycle`, `AgentExecution`, `MemoryService`, and
/// `SequentialThinkingService` trait impls so that they can coordinate
/// through shared in-memory state.
pub struct OrchestrationServer {
    pub pool: Arc<GrpcAgentPool>,
    pub workstack_executor: Arc<WorkstackExecutor>,
    pub memory: Arc<RwLock<HashMap<String, MemoryEntry>>>,
    pub chains: Arc<RwLock<HashMap<String, ThinkingChain>>>,
    pub contexts: Arc<RwLock<HashMap<String, ContextEntry>>>,
    pub sessions: Arc<RwLock<HashMap<String, SessionInfo>>>,
    pub cancellation_tokens: Arc<RwLock<HashMap<String, tokio::sync::watch::Sender<bool>>>>,
    pub agent_events: broadcast::Sender<AgentStatusEvent>,
}

impl OrchestrationServer {
    /// Create a new orchestration server with the given pool and executor.
    pub fn new(pool: Arc<GrpcAgentPool>, workstack_executor: Arc<WorkstackExecutor>) -> Self {
        let (agent_events, _) = broadcast::channel(256);

        Self {
            pool,
            workstack_executor,
            memory: Arc::new(RwLock::new(HashMap::new())),
            chains: Arc::new(RwLock::new(HashMap::new())),
            contexts: Arc::new(RwLock::new(HashMap::new())),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            cancellation_tokens: Arc::new(RwLock::new(HashMap::new())),
            agent_events,
        }
    }
}

/// Start the orchestration gRPC server on the given address.
///
/// Registers all 4 services on a single `tonic::Server`.
pub async fn run_orchestration_server(
    server: Arc<OrchestrationServer>,
    addr: std::net::SocketAddr,
) -> Result<(), tonic::transport::Error> {
    use super::proto::op_chat_orchestration::{
        agent_execution_server::AgentExecutionServer, agent_lifecycle_server::AgentLifecycleServer,
        backend_architect_service_server::BackendArchitectServiceServer,
        context_manager_service_server::ContextManagerServiceServer,
        memory_service_server::MemoryServiceServer, rust_pro_service_server::RustProServiceServer,
        sequential_thinking_service_server::SequentialThinkingServiceServer,
        workstack_service_server::WorkstackServiceServer,
    };

    tracing::info!(%addr, "Starting orchestration gRPC server");

    const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("op_chat_descriptor");

    let reflection = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
        .build_v1()
        .expect("failed to build reflection service");

    tonic::transport::Server::builder()
        .accept_http1(true)
        .add_service(tonic_web::enable(AgentLifecycleServer::from_arc(
            server.clone(),
        )))
        .add_service(tonic_web::enable(AgentExecutionServer::from_arc(
            server.clone(),
        )))
        .add_service(tonic_web::enable(MemoryServiceServer::from_arc(
            server.clone(),
        )))
        .add_service(tonic_web::enable(
            SequentialThinkingServiceServer::from_arc(server.clone()),
        ))
        .add_service(tonic_web::enable(ContextManagerServiceServer::from_arc(
            server.clone(),
        )))
        .add_service(tonic_web::enable(RustProServiceServer::from_arc(
            server.clone(),
        )))
        .add_service(tonic_web::enable(BackendArchitectServiceServer::from_arc(
            server.clone(),
        )))
        .add_service(tonic_web::enable(WorkstackServiceServer::from_arc(
            server.clone(),
        )))
        .add_service(tonic_web::enable(reflection))
        .serve(addr)
        .await
}
