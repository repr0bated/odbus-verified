//! End-to-end probe for the op-chat memory loop's semantic path.
//!
//! Uses live Voyage embeddings and a live Qdrant endpoint, but an in-memory
//! CozoDB and a per-run Qdrant collection. The temporary collection is deleted
//! after the probe, and the test skips cleanly when either service is absent.

use anyhow::{ensure, Context, Result};
use op_chat::memory_loop::MemoryLoop;
use op_cognitive_mcp::memory_store::{CognitiveMemoryStore, NamespaceKind};
use op_cognitive_mcp::{CozoGraphShuttle, QdrantSemanticShuttle};
use qdrant_client::qdrant::{CreateCollectionBuilder, Distance, VectorParamsBuilder};
use qdrant_client::Qdrant;
use serde_json::json;
use std::sync::Arc;

const VOYAGE_DIMS: u64 = 1024;
const KEY_VARS: [&str; 3] = [
    "COGNITIVE_MCP_VOYAGE_API_KEY",
    "VOYAGE_API_KEY",
    "VOYAGE_API_KEY_RUST",
];

fn voyage_key_present() -> bool {
    KEY_VARS
        .iter()
        .any(|var| std::env::var(var).is_ok_and(|value| !value.trim().is_empty()))
}

fn qdrant_url() -> String {
    std::env::var("OP_E2E_QDRANT_URL")
        .or_else(|_| std::env::var("COGNITIVE_MCP_QDRANT_URL"))
        .unwrap_or_else(|_| "http://127.0.0.1:6334".to_string())
}

#[tokio::test(flavor = "multi_thread")]
async fn post_turn_memory_is_vectorized_and_semantically_reinjected() -> Result<()> {
    if !voyage_key_present() {
        eprintln!("SKIP: no Voyage API key in env ({})", KEY_VARS.join(" / "));
        return Ok(());
    }

    let url = qdrant_url();
    let admin = Qdrant::from_url(&url)
        .build()
        .with_context(|| format!("building probe Qdrant client for {url}"))?;
    if admin.health_check().await.is_err() {
        eprintln!("SKIP: no Qdrant gRPC endpoint reachable at {url}");
        return Ok(());
    }

    let run_id = uuid::Uuid::new_v4().simple().to_string();
    let collection = format!("e2e_memory_loop_{run_id}");
    admin
        .create_collection(
            CreateCollectionBuilder::new(&collection)
                .vectors_config(VectorParamsBuilder::new(VOYAGE_DIMS, Distance::Cosine)),
        )
        .await
        .with_context(|| format!("creating probe collection {collection}"))?;

    let outcome = run_probe(&url, &collection).await;
    if let Err(error) = admin.delete_collection(&collection).await {
        eprintln!("WARN: probe collection {collection} not deleted: {error:#}");
    }
    outcome
}

async fn run_probe(url: &str, collection: &str) -> Result<()> {
    std::env::set_var("COGNITIVE_MCP_QDRANT_URL", url);
    std::env::set_var("COGNITIVE_MCP_USER_MEMORY_COLLECTION", collection);

    let cozo =
        Arc::new(CozoGraphShuttle::new_in_memory().context("creating in-memory CozoDB for probe")?);
    let store = Arc::new(CognitiveMemoryStore::new(cozo).await?);
    let container_id = format!("memory-loop-{collection}");
    let namespace = format!("container:{container_id}");
    store
        .upsert_namespace(
            &namespace,
            NamespaceKind::Custom,
            Some("Voyage/Qdrant memory-loop e2e probe"),
            None,
            None,
            json!({"temporary": true}),
        )
        .await?;

    let qdrant = Arc::new(
        QdrantSemanticShuttle::new()
            .await
            .context("creating semantic shuttle for memory-loop probe")?,
    );
    let memory_loop = MemoryLoop::new(Arc::clone(&store)).with_qdrant(qdrant);

    memory_loop
        .spawn_post_turn_memory_task(
            container_id.clone(),
            "I always prefer Rust with the Tokio async runtime.".to_string(),
            "Understood.".to_string(),
            Vec::new(),
        )
        .await
        .context("joining Rust preference memory task")?;

    // This newer entry is first in Cozo's recency order. A successful semantic
    // query must move the older Rust preference ahead of it during injection.
    memory_loop
        .spawn_post_turn_memory_task(
            container_id.clone(),
            "Our project routes email through Postfix and Dovecot.".to_string(),
            "Noted.".to_string(),
            Vec::new(),
        )
        .await
        .context("joining mail project memory task")?;

    let injected = memory_loop
        .inject_session_memory(
            "assistant",
            &container_id,
            "Which programming language and async runtime do I prefer?",
        )
        .await
        .context("injecting semantically ranked session memory")?;
    let first_memory = injected.domain_block.lines().next().unwrap_or_default();

    ensure!(
        first_memory.contains("Rust with the Tokio async runtime"),
        "semantic memory did not rank the Rust preference first: {first_memory}"
    );
    ensure!(
        injected.domain_block.contains("Postfix and Dovecot"),
        "semantic boost hid an unmatched durable memory"
    );
    println!(
        "memory-loop: 2 turns persisted to CozoDB, vectorized by Voyage, and semantically reordered from {collection}"
    );

    Ok(())
}
