//! End-to-end: Voyage vectorization -> Qdrant upsert -> Qdrant semantic query.
//!
//! Exercises the real [`QdrantSemanticShuttle`] path (no mocks): live Voyage
//! embeddings, live Qdrant writes, live similarity search, plus a wholesale
//! `blob_vectors` refresh over the SHM sealed-blob catalog.
//!
//! Skipped (not failed) unless both a Voyage API key and a reachable Qdrant
//! gRPC endpoint are present, so it is safe in `cargo test --workspace`.
//!
//! Run explicitly:
//! ```sh
//! OP_E2E_QDRANT_URL=http://10.200.0.2:6334 \
//!   cargo test -p op-cognitive-mcp --test e2e_voyage_qdrant -- --nocapture
//! ```
//!
//! All writes land in per-run `e2e_voyage_*` collections that are deleted on
//! the way out; live collections are never touched.

use anyhow::{Context, Result};
use op_cognitive_mcp::QdrantSemanticShuttle;
use qdrant_client::qdrant::{CreateCollectionBuilder, Distance, VectorParamsBuilder};
use qdrant_client::Qdrant;

/// Voyage shared-space dimension (voyage-4-large / voyage-4 / voyage-4-lite).
const PROBE_DIMS: u64 = 1024;

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
async fn voyage_vectorization_lands_in_qdrant_and_is_retrievable() -> Result<()> {
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
    let memory_collection = format!("e2e_voyage_user_memory_{run_id}");
    let blob_collection = format!("e2e_voyage_blob_vectors_{run_id}");
    let episodes_collection = format!("e2e_voyage_episodes_{run_id}");

    for collection in [&memory_collection, &blob_collection, &episodes_collection] {
        admin
            .create_collection(
                CreateCollectionBuilder::new(collection)
                    .vectors_config(VectorParamsBuilder::new(PROBE_DIMS, Distance::Cosine)),
            )
            .await
            .with_context(|| format!("creating probe collection {collection}"))?;
    }

    let outcome = run_probe(
        &url,
        &memory_collection,
        &blob_collection,
        &episodes_collection,
    )
    .await;

    for collection in [&memory_collection, &blob_collection, &episodes_collection] {
        if let Err(err) = admin.delete_collection(collection).await {
            eprintln!("WARN: probe collection {collection} not deleted: {err:#}");
        }
    }

    outcome
}

async fn run_probe(
    url: &str,
    memory_collection: &str,
    blob_collection: &str,
    episodes_collection: &str,
) -> Result<()> {
    std::env::set_var("COGNITIVE_MCP_QDRANT_URL", url);
    std::env::set_var("COGNITIVE_MCP_USER_MEMORY_COLLECTION", memory_collection);
    std::env::set_var("COGNITIVE_MCP_BLOB_VECTORS_COLLECTION", blob_collection);
    std::env::set_var("COGNITIVE_MCP_QDRANT_COLLECTION", episodes_collection);

    let shuttle = QdrantSemanticShuttle::new()
        .await
        .context("QdrantSemanticShuttle::new() failed")?;

    // ── 1. Voyage vectorization ──────────────────────────────────────────
    let ovs_doc = "The rovs plugin drives Open vSwitch natively over OVSDB JSON-RPC: \
                   it creates bridges, attaches ports, and installs OpenFlow rules.";
    let mail_doc = "The mail-3tched container runs Postfix and Dovecot, exposing SMTP \
                    submission on 587 and IMAPS on 993 for mailbox delivery.";

    let ovs_vector = shuttle
        .embed_document(ovs_doc)
        .await
        .context("Voyage embed_document failed for the OVS document")?;
    let mail_vector = shuttle
        .embed_document(mail_doc)
        .await
        .context("Voyage embed_document failed for the mail document")?;

    assert_eq!(
        ovs_vector.len(),
        PROBE_DIMS as usize,
        "Voyage returned {} dims, expected the shared-space {PROBE_DIMS}",
        ovs_vector.len()
    );
    assert_eq!(mail_vector.len(), PROBE_DIMS as usize);
    let norm = ovs_vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    assert!(
        norm > 0.5,
        "Voyage embedding looks degenerate (L2 norm {norm})"
    );
    println!("embed: 2 documents vectorized, {PROBE_DIMS} dims, L2 norm {norm:.4}");

    // ── 2. Qdrant upsert ─────────────────────────────────────────────────
    let container_id = format!("e2e-container-{}", uuid::Uuid::new_v4().simple());
    let ovs_point = uuid::Uuid::new_v4().to_string();
    let mail_point = uuid::Uuid::new_v4().to_string();

    shuttle
        .upsert_user_memory(&ovs_point, ovs_vector, &container_id, "ovs-note", ovs_doc)
        .await
        .context("upsert_user_memory failed for the OVS vector")?;
    shuttle
        .upsert_user_memory(
            &mail_point,
            mail_vector,
            &container_id,
            "mail-note",
            mail_doc,
        )
        .await
        .context("upsert_user_memory failed for the mail vector")?;
    println!("upsert: 2 points written to {memory_collection}");

    // ── 3. Query-side embedding + scoped semantic search ─────────────────
    let query_vector = shuttle
        .embed_query_text("how do I add a port to an Open vSwitch bridge?")
        .await
        .context("Voyage embed_query_text failed")?;
    assert_eq!(query_vector.len(), PROBE_DIMS as usize);

    let hits = shuttle
        .search_user_memory(query_vector, &container_id, 5)
        .await
        .context("search_user_memory failed")?;

    assert!(
        !hits.is_empty(),
        "scoped search returned nothing for container {container_id}"
    );
    let top = &hits[0];
    let top_key = top
        .payload
        .get("entry_key")
        .and_then(|v| v.as_str())
        .cloned()
        .unwrap_or_default();
    println!(
        "search: {} hits, top entry_key={top_key} score={:.4}",
        hits.len(),
        top.score
    );
    assert_eq!(
        top_key, "ovs-note",
        "OVS query ranked '{top_key}' first — vectors and payloads are misaligned"
    );
    assert!(
        top.score > 0.3,
        "top cosine score {} is implausibly low for a matching document",
        top.score
    );

    // Container scoping must exclude other containers' memory.
    let other_query = shuttle.embed_query_text("open vswitch bridge").await?;
    let leaked = shuttle
        .search_user_memory(other_query, "e2e-container-does-not-exist", 5)
        .await
        .context("scoped search for an unknown container failed")?;
    assert!(
        leaked.is_empty(),
        "container_id filter leaked {} points across containers",
        leaked.len()
    );

    // ── 4. blob_vectors refresh over the SHM sealed-blob catalog ─────────
    match op_blob::catalog::read_manifest_plugin_ids_shm() {
        Some(ids) if !ids.is_empty() => {
            let started = std::time::Instant::now();
            let summary = shuttle
                .refresh_blob_vectors()
                .await
                .context("refresh_blob_vectors failed")?;
            println!(
                "refresh: {} of {} catalog plugins embedded into {} in {:.1}s",
                summary.embedded,
                ids.len(),
                summary.collection,
                started.elapsed().as_secs_f64()
            );
            assert_eq!(summary.collection, blob_collection);
            assert!(
                summary.embedded > 0,
                "SHM catalog listed {} plugins but nothing was embedded",
                ids.len()
            );

            let results = shuttle
                .search_blob_vectors("open vswitch bridge ports configured over OVSDB", 5)
                .await
                .context("search_blob_vectors failed")?;
            assert!(
                !results.is_empty(),
                "blob_vectors search returned no plugins after embedding {} of them",
                summary.embedded
            );
            for point in &results {
                let plugin_id = point
                    .payload
                    .get("plugin_id")
                    .and_then(|v| v.as_str())
                    .cloned()
                    .unwrap_or_default();
                assert!(
                    !plugin_id.is_empty(),
                    "blob_vectors point is missing its plugin_id payload"
                );
                println!("  blob hit: {plugin_id} score={:.4}", point.score);
            }
        }
        _ => eprintln!("SKIP blob_vectors refresh: no SHM sealed-blob catalog on this host"),
    }

    Ok(())
}
