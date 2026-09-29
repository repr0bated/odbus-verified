//! Embedded cognitive MCP runtime for `op-grpc-bridge`.
//!
//! A specialized MCP server for cognitive memory and dynamic content loading.
//! Provides tools for:
//! - Memory storage and retrieval
//! - Dynamic content loading
//! - Cognitive state management
//! - Context-aware tool discovery (code_search, code_context, code_index)
//! - Typed namespace tools for Operation D-Bus
//! - Conversation session management
//! - Quota awareness

pub mod activity_filter;
pub mod agent_tools;
pub mod blob_catalog_tool;
pub mod blob_vectors_tool;
pub mod chain_vectors;
pub mod code_tools;
pub mod cognitive_tools;
pub mod context_awareness;
pub mod cozo_shuttle;
pub mod dbus_interface;
pub mod doctor;
pub mod gemini_fallback;
pub mod grpc_service;
pub mod memory_store;
pub mod models_provider;
pub mod qdrant_shuttle;
pub mod quota;
pub mod rag_pipeline;
pub mod semantic_search;
pub mod server;
pub mod session;
pub mod soul_memory;
pub mod tool_profiles;
pub mod typed_tools;
pub mod voyage;

pub use activity_filter::{
    ActivityEvent, ActivityFilter, FilterDecision, FilterTunables, OpKind, Significance,
    SuppressReason, derive_significance, is_pii,
};
pub use chain_vectors::ChainVectorIndex;
pub use code_tools::register_code_tools;
pub use cognitive_tools::CognitiveToolRegistry;
pub use context_awareness::{
    ActivityEvent as ContextActivityEvent, ActivityType, ContextAwarenessConfig,
    ContextAwarenessEngine, KnowledgeContent, KnowledgePush, PushTrigger,
};
pub use cozo_shuttle::{CozoGraphShuttle, PolicyVerdict};
pub use grpc_service::CognitiveGrpcService;
pub use memory_store::CognitiveMemoryStore;
pub use models_provider::ModelsProvider;
pub use qdrant_shuttle::{QdrantSemanticShuttle, SessionTraceContext};
pub use quota::{QuotaManager, QuotaTier};
pub use rag_pipeline::{CodeFilter, RagPipeline, RagResult};
pub use server::CognitiveMcpServer;
pub use session::SessionManager;
pub use soul_memory::{AgentNamespaceBinding, SoulMemory, SoulMemoryStore, SoulUpdate};
pub use voyage::VoyageClient;

/// Generated protobuf types for the CognitiveToolService.
/// Compiled from proto/cognitive.proto via tonic-build.
pub mod proto {
    tonic::include_proto!("operation.cognitive.v1");

    /// Combined FileDescriptorSet for reflection.
    pub const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("cognitive_descriptor");
}
pub mod interceptor;
