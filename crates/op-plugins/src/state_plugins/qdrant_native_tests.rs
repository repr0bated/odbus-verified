//! Native Qdrant contract coverage.
//!
//! This test is deliberately ignored by default. The runner must provide a
//! separately selected Qdrant executable through `OP_QDRANT_NATIVE_BIN`; the
//! test never consults the host Qdrant configuration or persistent storage.

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};

use super::{dispatch_qdrant_method_with_client, QdrantHttpClient};

const COLLECTION: &str = "native_dispatch_coverage";

/// A synchronous drop fallback prevents an orphan native server if an
/// assertion panics before the normal async cleanup path runs.
struct NativeChild(Option<Child>);

impl Drop for NativeChild {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.start_kill();
        }
    }
}

impl NativeChild {
    async fn stop(&mut self) {
        let Some(mut child) = self.0.take() else {
            return;
        };
        let _ = child.kill().await;
        let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
    }

    fn as_mut(&mut self) -> Result<&mut Child> {
        self.0
            .as_mut()
            .context("native Qdrant child was already stopped")
    }
}

fn reserve_loopback_ports() -> Result<(std::net::TcpListener, std::net::TcpListener)> {
    let http = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    let grpc = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    anyhow::ensure!(
        http.local_addr()?.port() != grpc.local_addr()?.port(),
        "dynamic HTTP and gRPC ports unexpectedly collided"
    );
    Ok((http, grpc))
}

fn path_string(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .context("temporary Qdrant path is not UTF-8")
}

fn spawn_native(binary: &Path, root: &Path, http_port: u16, grpc_port: u16) -> Result<NativeChild> {
    let storage = root.join("storage");
    let snapshots = root.join("snapshots");
    let config = root.join("qdrant.yaml");
    std::fs::create_dir_all(&storage)?;
    std::fs::create_dir_all(&snapshots)?;
    let storage = path_string(&storage)?;
    let snapshots = path_string(&snapshots)?;
    let config_text = format!(
        "storage:\n  storage_path: {storage}\n  snapshots_path: {snapshots}\nservice:\n  host: 127.0.0.1\n  http_port: {http_port}\n  grpc_port: {grpc_port}\ncluster:\n  enabled: false\ntelemetry_disabled: true\nlog_level: WARN\n"
    );
    std::fs::write(&config, config_text)?;

    // Clear inherited settings first: an operator's QDRANT__* variables must
    // never redirect this fixture to a live endpoint, path, or cluster.
    let child = Command::new(binary)
        .env_clear()
        .env("QDRANT__STORAGE__STORAGE_PATH", &storage)
        .env("QDRANT__STORAGE__SNAPSHOTS_PATH", &snapshots)
        .env("QDRANT__SERVICE__HOST", "127.0.0.1")
        .env("QDRANT__SERVICE__HTTP_PORT", http_port.to_string())
        .env("QDRANT__SERVICE__GRPC_PORT", grpc_port.to_string())
        .env("QDRANT__CLUSTER__ENABLED", "false")
        .env("QDRANT__TELEMETRY_DISABLED", "true")
        .kill_on_drop(true)
        .current_dir(root)
        .arg("--config-path")
        .arg(config)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("spawn native Qdrant from {}", binary.display()))?;
    Ok(NativeChild(Some(child)))
}

async fn wait_until_ready(child: &mut Child, base: &str) -> Result<()> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait()? {
            bail!("native Qdrant exited before readiness: {status}");
        }
        if Instant::now() >= deadline {
            bail!("native Qdrant did not become ready before the 30 second deadline");
        }
        if let Ok(response) = http.get(format!("{base}/readyz")).send().await {
            if response.status().is_success() {
                return Ok(());
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn call(client: &QdrantHttpClient, method: &str, args: Value) -> Result<Value> {
    dispatch_qdrant_method_with_client(client, method, &args).await
}

fn assert_operation_completed(output: &Value) {
    assert_eq!(output["status"], "ok");
    assert_eq!(output["operation_status"], "completed");
    assert!(
        output["operation_id"].as_u64().is_some(),
        "missing operation id: {output}"
    );
}

fn point_id(point: &Value) -> i64 {
    point["id"]
        .as_i64()
        .expect("native Qdrant point id should be numeric")
}

fn has_collection(output: &Value) -> bool {
    output["collections"]
        .as_array()
        .expect("collection list omitted collections")
        .iter()
        .any(|name| name.as_str() == Some(COLLECTION))
}

#[tokio::test]
#[ignore = "requires OP_QDRANT_NATIVE_BIN pointing to a native Qdrant binary"]
async fn native_qdrant_round_trips_all_declared_methods() -> Result<()> {
    let binary = std::env::var_os("OP_QDRANT_NATIVE_BIN")
        .context("set OP_QDRANT_NATIVE_BIN to run native Qdrant coverage")?;
    let binary = PathBuf::from(binary);
    anyhow::ensure!(
        binary.is_file(),
        "OP_QDRANT_NATIVE_BIN is not a file: {}",
        binary.display()
    );

    let root = tempfile::tempdir()?;
    let (http_listener, grpc_listener) = reserve_loopback_ports()?;
    let http_port = http_listener.local_addr()?.port();
    let grpc_port = grpc_listener.local_addr()?.port();
    let base = format!("http://127.0.0.1:{http_port}");
    // Keep both reservations alive until the child is fully configured, then
    // release them immediately before spawn so an unrelated service cannot
    // win the port race during setup.
    drop((http_listener, grpc_listener));
    let mut native = spawn_native(&binary, root.path(), http_port, grpc_port)?;
    let result = async {
        wait_until_ready(native.as_mut()?, &base).await?;
        let mut client = QdrantHttpClient::with_base(base.clone())?;
        // with_base normally inherits QDRANT_API_KEY for the production
        // endpoint; this disposable loopback fixture must never receive a
        // host credential.
        client.api_key = None;

        // Before: the fresh storage has no fixture collection.
        let before = call(&client, "list_collections", serde_json::json!({})).await?;
        assert!(!has_collection(&before));

        let created = call(
            &client,
            "create_collection",
            serde_json::json!({
                "name": COLLECTION,
                "vectors_config": {"size": 3, "distance": "Cosine"}
            }),
        )
        .await?;
        assert_eq!(created["status"], "ok");
        assert_eq!(created["result"], true);

        // Effect/readback: creation is visible through a second native request.
        let listed = call(&client, "list_collections", serde_json::json!({})).await?;
        assert!(has_collection(&listed));

        let upserted = call(
            &client,
            "upsert_points",
            serde_json::json!({
                "collection_name": COLLECTION,
                "points": [
                    {"id": 1, "vector": [1.0, 0.0, 0.0], "payload": {"kind": "alpha", "n": 1}},
                    {"id": 2, "vector": [0.0, 1.0, 0.0], "payload": {"kind": "beta", "n": 2}}
                ]
            }),
        )
        .await?;
        assert_operation_completed(&upserted);

        let mut scrolled = call(
            &client,
            "scroll_points",
            serde_json::json!({"collection_name": COLLECTION, "limit": 10}),
        )
        .await?["points"]
            .as_array()
            .context("scroll response omitted points")?
            .clone();
        scrolled.sort_by_key(point_id);
        assert_eq!(scrolled.len(), 2);
        assert_eq!(point_id(&scrolled[0]), 1);
        assert_eq!(scrolled[0]["payload"]["kind"], "alpha");
        assert_eq!(scrolled[1]["payload"]["n"], 2);

        let search = call(
            &client,
            "search_points",
            serde_json::json!({
                "collection_name": COLLECTION,
                "vector": [1.0, 0.0, 0.0],
                "limit": 1
            }),
        )
        .await?;
        let result = search["results"]
            .as_array()
            .context("search response omitted results")?;
        assert_eq!(result.len(), 1);
        assert_eq!(point_id(&result[0]), 1);
        assert!(
            result[0]["score"].as_f64().unwrap_or_default() > 0.99,
            "unexpected search result: {}",
            result[0]
        );

        let deleted = call(
            &client,
            "delete_points",
            serde_json::json!({"collection_name": COLLECTION, "points": [1]}),
        )
        .await?;
        assert_operation_completed(&deleted);
        let after_delete = call(
            &client,
            "scroll_points",
            serde_json::json!({"collection_name": COLLECTION, "limit": 10}),
        )
        .await?;
        let remaining = after_delete["points"]
            .as_array()
            .context("delete readback omitted points")?;
        assert_eq!(remaining.len(), 1);
        assert_eq!(point_id(&remaining[0]), 2);

        let indexed = call(
            &client,
            "create_payload_index",
            serde_json::json!({
                "collection_name": COLLECTION,
                "field_name": "kind",
                "field_schema": "keyword"
            }),
        )
        .await?;
        assert_operation_completed(&indexed);

        let snapshot = call(
            &client,
            "create_snapshot",
            serde_json::json!({"collection_name": COLLECTION}),
        )
        .await?;
        assert_eq!(snapshot["status"], "ok");
        let snapshot_name = snapshot["snapshot"]["name"]
            .as_str()
            .context("snapshot omitted name")?;
        assert!(!snapshot_name.is_empty());
        assert!(snapshot["snapshot"]["size"].as_u64().is_some());
        let creation_time = snapshot["snapshot"]
            .get("creation_time")
            .context("typed snapshot output omitted creation_time")?;
        assert!(creation_time.is_null() || creation_time.is_string());
        let snapshots = call(
            &client,
            "list_snapshots",
            serde_json::json!({"collection_name": COLLECTION}),
        )
        .await?;
        assert!(snapshots["snapshots"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| name.as_str() == Some(snapshot_name)));

        // Read the actual provider description independently without changing
        // the canonical list_snapshots contract, which returns snapshot names.
        let native_snapshots = client
            .request(
                reqwest::Method::GET,
                &format!("/collections/{COLLECTION}/snapshots"),
                None,
                false,
            )
            .await?;
        let descriptions = native_snapshots["result"]
            .as_array()
            .context("native snapshot list omitted descriptions")?;
        let matching = descriptions
            .iter()
            .filter(|description| description["name"].as_str() == Some(snapshot_name))
            .collect::<Vec<_>>();
        assert_eq!(
            matching.len(),
            1,
            "created snapshot must appear exactly once"
        );
        for field in ["name", "size", "creation_time", "checksum"] {
            assert_eq!(
                snapshot["snapshot"][field], matching[0][field],
                "snapshot {field} must match the native provider readback"
            );
        }

        let removed = call(
            &client,
            "delete_collection",
            serde_json::json!({"name": COLLECTION}),
        )
        .await?;
        assert_eq!(removed["status"], "ok");
        assert_eq!(removed["result"], true);
        let final_list = call(&client, "list_collections", serde_json::json!({})).await?;
        assert!(!has_collection(&final_list));
        Ok::<(), anyhow::Error>(())
    }
    .await;
    native.stop().await;
    result
}
