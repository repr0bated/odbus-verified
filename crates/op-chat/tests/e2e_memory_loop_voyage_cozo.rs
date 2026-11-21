//! Opt-in Voyage integration for chat memory in an isolated Cozo index.

use anyhow::{Context, Result, ensure};
use op_chat::memory_loop::MemoryLoop;
use op_cognitive::memory_store::{CognitiveMemoryStore, NamespaceKind};
use op_cognitive::{
    CozoGraphShuttle, CozoSemanticShuttle, semantic_index::SemanticIndex, voyage::VoyageClient,
};
use serde_json::json;
use std::sync::Arc;

const KEY_VARS: [&str; 3] = [
    "COGNITIVE_VOYAGE_API_KEY",
    "VOYAGE_API_KEY",
    "VOYAGE_API_KEY_RUST",
];

fn voyage_key_present() -> bool {
    KEY_VARS
        .iter()
        .any(|var| std::env::var(var).is_ok_and(|value| !value.trim().is_empty()))
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "uses live Voyage credits"]
async fn post_turn_memory_is_vectorized_and_semantically_reinjected() -> Result<()> {
    if !voyage_key_present() {
        return Ok(());
    }
    let cozo =
        Arc::new(CozoGraphShuttle::new_in_memory().context("creating in-memory CozoDB for probe")?);
    let store = Arc::new(CognitiveMemoryStore::new(cozo).await?);
    let container_id = format!("memory-loop-{}", uuid::Uuid::new_v4());
    let namespace = format!("container:{container_id}");
    store
        .upsert_namespace(
            &namespace,
            NamespaceKind::Custom,
            Some("Voyage/Cozo memory-loop e2e probe"),
            None,
            None,
            json!({"temporary": true}),
        )
        .await?;

    let semantic = Arc::new(CozoSemanticShuttle::with_index(
        SemanticIndex::in_memory()?,
        VoyageClient::new()?,
    ));
    let memory_loop = MemoryLoop::new(Arc::clone(&store)).with_semantic_index(semantic);

    memory_loop
        .spawn_post_turn_memory_task(
            container_id.clone(),
            "I always prefer Rust with the Tokio async runtime.".to_string(),
            "Understood.".to_string(),
            Vec::new(),
        )
        .await
        .context("joining Rust preference memory task")?
        .context("persisting Rust preference memory")?;

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
        .context("joining mail project memory task")?
        .context("persisting mail project memory")?;

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
        "memory-loop: 2 turns persisted to CozoDB, vectorized by Voyage, and semantically reordered by Cozo"
    );

    Ok(())
}
