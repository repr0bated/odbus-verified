//! Bridge-owned cognitive runtime.
//!
//! Single persistent backend (CozoDB) hosts every relation:
//! memory namespaces/entries, users, sessions, compliance graph, subid registry, audit log.
//! This type owns no listener; `op-grpc-bridge` dispatches authenticated calls
//! into its tool registry in process.

use crate::code_tools::register_code_tools;
use crate::cognitive_tools::CognitiveToolRegistry;
use crate::context_awareness::{ContextAwarenessConfig, ContextAwarenessEngine};
use crate::cozo_shuttle::CozoGraphShuttle;
use crate::memory_store::CognitiveMemoryStore;
use crate::qdrant_shuttle::QdrantSemanticShuttle;
use crate::quota::QuotaManager;
use crate::rag_pipeline::{default_collection_from_env, RagPipeline};
use crate::session::SessionManager;
use crate::typed_tools;
use op_mcp::tool_registry::ToolRegistry;
use std::path::PathBuf;
use std::sync::Arc;

pub struct CognitiveMcpServer {
    memory_store: Arc<CognitiveMemoryStore>,
    cozo_shuttle: Arc<CozoGraphShuttle>,
    qdrant_shuttle: Option<Arc<QdrantSemanticShuttle>>,
    tool_registry: Arc<ToolRegistry>,
    session_manager: Arc<SessionManager>,
    quota_manager: Arc<QuotaManager>,
    /// Code-RAG pipeline (Voyage + Qdrant). `None` when no Voyage key is found.
    rag_pipeline: Option<Arc<RagPipeline>>,
    /// Proactive coding-context awareness engine.
    context_engine: Arc<ContextAwarenessEngine>,
}

impl CognitiveMcpServer {
    /// Create a new in-memory Cognitive MCP server.
    pub async fn new_in_memory() -> Result<Self, Box<dyn std::error::Error>> {
        Self::new(":memory:").await
    }

    /// `db_path` is the CozoDB directory backing relations, or `":memory:"` for ephemeral mode.
    /// If opening a persistent path fails (e.g. `op-grpc-bridge` holds the RocksDB write lock),
    /// this gracefully falls back to in-memory mode rather than crashing.
    pub async fn new(db_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let cozo_shuttle = if db_path == ":memory:" || db_path.is_empty() || db_path == "memory" {
            Arc::new(CozoGraphShuttle::new_in_memory()?)
        } else {
            match CozoGraphShuttle::new_persistent(PathBuf::from(db_path)) {
                Ok(shuttle) => Arc::new(shuttle),
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        path = %db_path,
                        "Could not open persistent CozoDB (likely locked by op-grpc-bridge). Falling back to in-memory store."
                    );
                    Arc::new(CozoGraphShuttle::new_in_memory()?)
                }
            }
        };
        Self::new_with_shuttle(cozo_shuttle).await
    }

    /// Open the production durable store without an in-memory fallback.
    ///
    /// The bridge uses this constructor so a locked or unavailable Cozo path is
    /// reported to the caller as `Unavailable` instead of acknowledging writes
    /// that disappear at restart.  The process may still start and serve
    /// non-cognitive plugins because initialization is lazy at the call site.
    pub async fn new_durable(db_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        if db_path.is_empty() || db_path == ":memory:" || db_path == "memory" {
            return Err("a durable CognitiveMcpServer requires a persistent Cozo path".into());
        }
        let cozo_shuttle = Arc::new(CozoGraphShuttle::new_persistent(PathBuf::from(db_path))?);
        Self::new_with_shuttle(cozo_shuttle).await
    }

    /// Create a Cognitive MCP server with a pre-configured CozoGraphShuttle.
    pub async fn new_with_shuttle(
        cozo_shuttle: Arc<CozoGraphShuttle>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let memory_store = Arc::new(CognitiveMemoryStore::new(cozo_shuttle.clone()).await?);

        let tool_registry = Arc::new(ToolRegistry::new());

        let qdrant_shuttle = match QdrantSemanticShuttle::new().await {
            Ok(shuttle) => Some(Arc::new(shuttle)),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Qdrant Semantic Shuttle unavailable; cognitive MCP will continue without vector retrieval"
                );
                None
            }
        };

        let session_manager = Arc::new(SessionManager::with_defaults());
        let quota_manager = Arc::new(QuotaManager::with_defaults());

        CognitiveToolRegistry::register_all(
            &tool_registry,
            memory_store.clone(),
            qdrant_shuttle.clone(),
        )
        .await?;

        typed_tools::register_typed_tools(
            &tool_registry,
            memory_store.clone(),
            session_manager.clone(),
            quota_manager.clone(),
        )
        .await?;

        // Code-RAG pipeline: optional, like qdrant_shuttle. Without a Voyage key
        // (env or ~/.ssh/mongo-voyage) the cognitive MCP still serves memory tools.
        let rag_pipeline = match RagPipeline::from_env() {
            Ok(p) => Some(Arc::new(p)),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "RAG pipeline unavailable; code-context tools will not be registered"
                );
                None
            }
        };

        // Context-awareness engine drives session signals and proactive pushes.
        let rag_collection = default_collection_from_env();
        let context_engine = Arc::new(ContextAwarenessEngine::new(
            ContextAwarenessConfig {
                rag_collection: rag_collection.clone(),
                ..Default::default()
            },
            memory_store.clone(),
            rag_pipeline.clone(),
        ));
        context_engine.clone().start_monitoring();

        if let Some(rag) = &rag_pipeline {
            let n = register_code_tools(
                &tool_registry,
                rag.clone(),
                context_engine.clone(),
                rag_collection,
            )
            .await?;
            tracing::info!(registered = n, "Registered code-context tools");
        }

        Ok(Self {
            memory_store,
            cozo_shuttle,
            qdrant_shuttle,
            tool_registry,
            session_manager,
            quota_manager,
            rag_pipeline,
            context_engine,
        })
    }

    pub fn memory_store(&self) -> Arc<CognitiveMemoryStore> {
        self.memory_store.clone()
    }

    pub fn cozo_shuttle(&self) -> Arc<CozoGraphShuttle> {
        self.cozo_shuttle.clone()
    }

    pub fn tool_registry(&self) -> Arc<ToolRegistry> {
        self.tool_registry.clone()
    }

    pub fn qdrant_shuttle(&self) -> Option<Arc<QdrantSemanticShuttle>> {
        self.qdrant_shuttle.clone()
    }

    pub fn session_manager(&self) -> Arc<SessionManager> {
        self.session_manager.clone()
    }

    pub fn quota_manager(&self) -> Arc<QuotaManager> {
        self.quota_manager.clone()
    }

    pub fn rag_pipeline(&self) -> Option<Arc<RagPipeline>> {
        self.rag_pipeline.clone()
    }

    pub fn context_engine(&self) -> Arc<ContextAwarenessEngine> {
        self.context_engine.clone()
    }
}
