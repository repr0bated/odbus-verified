//! Native MongoDB MCP owner coverage.
//!
//! This ignored test starts only binaries explicitly supplied by the runner:
//! a disposable mongod and the pinned MongoDB MCP provider.  It never uses
//! the host provider, credentials, sockets, database path, or service ports.

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};

use super::{dispatch_mongodb_mcp_owner, SupervisedMcpProvider};
use op_plugins::state_plugins::mongodb_mcp::MongoDbMcpPlugin;
use op_state::StatePlugin;
use op_state_store::PluginSchema;

const DATABASE: &str = "native_mcp_fixture";
const COLLECTION: &str = "records";

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
            .context("native MongoDB child was already stopped")
    }
}

fn reserve_loopback_ports(count: usize) -> Result<Vec<std::net::TcpListener>> {
    let listeners = (0..count)
        .map(|_| std::net::TcpListener::bind(("127.0.0.1", 0)))
        .collect::<std::io::Result<Vec<_>>>()?;
    let mut ports = listeners
        .iter()
        .map(|listener| listener.local_addr().map(|address| address.port()))
        .collect::<std::io::Result<Vec<_>>>()?;
    ports.sort_unstable();
    ports.dedup();
    anyhow::ensure!(
        ports.len() == listeners.len(),
        "native fixture ports collided"
    );
    Ok(listeners)
}

fn required_path(name: &str) -> Result<PathBuf> {
    let value = std::env::var_os(name).with_context(|| format!("set {name}"))?;
    let path = PathBuf::from(value);
    anyhow::ensure!(path.is_file(), "{name} is not a file: {}", path.display());
    Ok(path)
}

fn required_dir(name: &str) -> Result<PathBuf> {
    let value = std::env::var_os(name).with_context(|| format!("set {name}"))?;
    let path = PathBuf::from(value);
    anyhow::ensure!(
        path.is_dir(),
        "{name} is not a directory: {}",
        path.display()
    );
    Ok(path)
}

async fn wait_tcp(child: &mut Child, host: &str, port: u16) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait()? {
            bail!("native MongoDB process exited before readiness: {status}");
        }
        if Instant::now() >= deadline {
            bail!("native MongoDB process did not bind 127.0.0.1:{port} before deadline");
        }
        if TcpStream::connect((host, port)).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn seed_database(node: &Path, provider_root: &Path, connection_string: &str) -> Result<()> {
    // The driver is the pinned provider's own dependency.  This one-shot
    // seeder writes only the temporary fixture database before read-only MCP
    // calls begin; it never touches the host MongoDB provider.
    let script = r#"
import { MongoClient } from "mongodb";
const client = new MongoClient(process.env.MONGO_FIXTURE_URL);
await client.connect();
const collection = client.db("native_mcp_fixture").collection("records");
await collection.insertMany([
  { _id: 1, kind: "alpha", amount: 7, tags: ["one", "two"] },
  { _id: 2, kind: "beta", amount: 11, tags: ["two"] }
]);
await collection.createIndex({ kind: 1 }, { name: "kind_1" });
await client.close();
"#;
    let mut seeder = NativeChild(Some(
        Command::new(node)
            .env_clear()
            .env("USER", "fixture")
            .env("PATH", "/usr/bin:/bin")
            .env("MONGO_FIXTURE_URL", connection_string)
            .current_dir(provider_root)
            .arg("--input-type=module")
            .arg("-e")
            .arg(script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("spawn disposable MongoDB seed")?,
    ));
    let status = tokio::time::timeout(Duration::from_secs(30), seeder.as_mut()?.wait()).await;
    seeder.stop().await;
    let status = status.context("disposable MongoDB seed exceeded its deadline")??;
    anyhow::ensure!(
        status.success(),
        "disposable MongoDB seed exited with {status}"
    );
    Ok(())
}

fn spawn_mongod(binary: &Path, root: &Path, port: u16) -> Result<NativeChild> {
    let dbpath = root.join("db");
    std::fs::create_dir_all(&dbpath)?;
    let logpath = root.join("mongod.log");
    let child = Command::new(binary)
        .env_clear()
        .env("USER", "fixture")
        .env("PATH", "/usr/bin:/bin")
        .current_dir(root)
        .kill_on_drop(true)
        .arg("--dbpath")
        .arg(dbpath)
        .arg("--bind_ip")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .arg("--logpath")
        .arg(logpath)
        .arg("--logappend")
        .arg("--quiet")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("spawn mongod from {}", binary.display()))?;
    Ok(NativeChild(Some(child)))
}

fn spawn_provider(
    node: &Path,
    entry: &Path,
    provider_root: &Path,
    root: &Path,
    mongo_url: &str,
    http_port: u16,
    monitoring_port: u16,
) -> Result<NativeChild> {
    let log_path = root.join("provider-logs");
    let exports_path = root.join("provider-exports");
    std::fs::create_dir_all(&log_path)?;
    std::fs::create_dir_all(&exports_path)?;
    let child = Command::new(node)
        .env_clear()
        .env("USER", "fixture")
        .env("PATH", "/usr/bin:/bin")
        .env("MDB_MCP_CONNECTION_STRING", mongo_url)
        .env("MDB_MCP_READ_ONLY", "true")
        .env("MDB_MCP_DISABLE_SERVER_SIDE_JS", "true")
        .env("MDB_MCP_ALLOW_REQUEST_OVERRIDES", "false")
        .env("MDB_MCP_TELEMETRY", "disabled")
        .env("MDB_MCP_LOG_PATH", &log_path)
        .env("MDB_MCP_EXPORTS_PATH", &exports_path)
        .env("MDB_MCP_TRANSPORT", "http")
        .env("MDB_MCP_HTTP_HOST", "127.0.0.1")
        .env("MDB_MCP_HTTP_PORT", http_port.to_string())
        .env("MDB_MCP_HTTP_RESPONSE_TYPE", "json")
        .env("MDB_MCP_MONITORING_SERVER_HOST", "127.0.0.1")
        .env(
            "MDB_MCP_MONITORING_SERVER_PORT",
            monitoring_port.to_string(),
        )
        .current_dir(provider_root)
        .kill_on_drop(true)
        .arg(entry)
        .arg("--transport")
        .arg("http")
        .arg("--httpHost=127.0.0.1")
        .arg(format!("--httpPort={http_port}"))
        .arg("--readOnly")
        .arg("--loggers")
        .arg("stderr,mcp")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("spawn MongoDB MCP provider from {}", entry.display()))?;
    Ok(NativeChild(Some(child)))
}

async fn call(provider: &SupervisedMcpProvider, method: &str, arguments: Value) -> Result<Value> {
    let result = dispatch_mongodb_mcp_owner(provider, method, arguments).await?;
    Ok(serde_json::to_value(result)?)
}

fn structured(output: Value) -> Value {
    assert!(
        output["isError"].as_bool() != Some(true),
        "MCP tool returned an error: {output}"
    );
    output
        .get("structuredContent")
        .cloned()
        .expect("MCP result omitted structuredContent")
}

async fn close_provider(provider: &SupervisedMcpProvider) {
    let mut client = provider.client.lock().await;
    provider.discard_client(&mut client).await;
}

fn validate_method(schema: &PluginSchema, method: &str, arguments: &Value, output: &Value) {
    let declaration = schema
        .methods
        .get(method)
        .unwrap_or_else(|| panic!("MongoDB PluginSchema omitted {method}"));
    let args_schema = serde_json::to_value(&declaration.args)
        .expect("MongoDB method argument schema must serialize to JSON");
    jsonschema::validate(&args_schema, arguments)
        .unwrap_or_else(|error| panic!("{method} input violates PluginSchema: {error}"));
    if let Some(returns) = declaration.returns.as_ref() {
        let returns_schema = serde_json::to_value(returns)
            .expect("MongoDB method return schema must serialize to JSON");
        jsonschema::validate(&returns_schema, output)
            .unwrap_or_else(|error| panic!("{method} output violates PluginSchema: {error}"));
    }
}

async fn call_checked(
    provider: &SupervisedMcpProvider,
    schema: &PluginSchema,
    method: &str,
    arguments: Value,
) -> Result<Value> {
    let output = call(provider, method, arguments.clone()).await?;
    validate_method(schema, method, &arguments, &output);
    Ok(output)
}

#[tokio::test]
#[ignore = "requires OP_MONGOD_NATIVE_BIN, OP_MONGODB_MCP_PROVIDER_ENTRY, and OP_MONGODB_MCP_PROVIDER_ROOT"]
async fn native_mongodb_mcp_round_trips_all_database_methods() -> Result<()> {
    let mongod = required_path("OP_MONGOD_NATIVE_BIN")?;
    let entry = required_path("OP_MONGODB_MCP_PROVIDER_ENTRY")?;
    let provider_root = required_dir("OP_MONGODB_MCP_PROVIDER_ROOT")?;
    let node = required_path("OP_NODE_BIN")?;
    let root = tempfile::tempdir()?;
    let schema = MongoDbMcpPlugin::new()
        .schema()
        .context("MongoDB plugin has no owning PluginSchema")?;
    let listeners = reserve_loopback_ports(4)?;
    let mongo_port = listeners[0].local_addr()?.port();
    let provider_port = listeners[1].local_addr()?.port();
    let monitoring_port = listeners[2].local_addr()?.port();
    let _reserved_extra = listeners[3].local_addr()?.port();
    drop(listeners);

    let mongo_url = format!("mongodb://127.0.0.1:{mongo_port}/{DATABASE}?directConnection=true");
    let mut mongo = spawn_mongod(&mongod, root.path(), mongo_port)?;
    wait_tcp(mongo.as_mut()?, "127.0.0.1", mongo_port).await?;
    seed_database(&node, &provider_root, &mongo_url).await?;

    let mut provider = spawn_provider(
        &node,
        &entry,
        &provider_root,
        root.path(),
        &mongo_url,
        provider_port,
        monitoring_port,
    )?;
    wait_tcp(provider.as_mut()?, "127.0.0.1", provider_port).await?;
    let owner =
        SupervisedMcpProvider::new("mongodb", format!("http://127.0.0.1:{provider_port}/mcp"));
    let result = tokio::time::timeout(Duration::from_secs(180), async {
        let connections_before = structured(call_checked(&owner, &schema, "list_connections", json!({})).await?);
        assert!(connections_before["connections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|connection| connection["connectionId"] == "preconfigured"));

        let databases = structured(call_checked(&owner, &schema, "list_databases", json!({"connectionId":"preconfigured"})).await?);
        assert!(databases["databases"].as_array().unwrap().iter().any(|db| db["name"] == DATABASE));

        let connections_after = structured(call_checked(&owner, &schema, "list_connections", json!({})).await?);
        assert!(connections_after["connections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|connection| {
                connection["connectionId"] == "preconfigured"
                    && connection["state"] == "connected"
            }));

        let scoped = json!({"connectionId":"preconfigured","database":DATABASE});
        let collections = structured(call_checked(&owner, &schema, "list_collections", scoped).await?);
        assert!(collections["collections"].as_array().unwrap().iter().any(|collection| collection["name"] == COLLECTION));

        let stats = structured(call_checked(&owner, &schema, "db_stats", json!({"connectionId":"preconfigured","database":DATABASE})).await?);
        assert_eq!(stats["stats"]["db"], DATABASE);

        let collection_args = json!({"connectionId":"preconfigured","database":DATABASE,"collection":COLLECTION});
        let collection_schema = structured(call_checked(&owner, &schema, "collection_schema", json!({"connectionId":"preconfigured","database":DATABASE,"collection":COLLECTION,"sampleSize":10})).await?);
        assert!(collection_schema["fieldsCount"].as_u64().unwrap_or_default() >= 4);
        assert!(collection_schema["schema"].get("kind").is_some());

        let indexes = structured(call_checked(&owner, &schema, "collection_indexes", collection_args.clone()).await?);
        assert!(indexes["classicIndexes"].as_array().unwrap().iter().any(|index| index["name"] == "kind_1"));

        let storage = structured(call_checked(&owner, &schema, "collection_storage_size", collection_args.clone()).await?);
        assert!(storage["size"].as_f64().unwrap_or_default() > 0.0);
        assert!(!storage["units"].as_str().unwrap_or_default().is_empty());

        let found = structured(call_checked(&owner, &schema, "find", json!({"connectionId":"preconfigured","database":DATABASE,"collection":COLLECTION,"filter":{"kind":"alpha"},"limit":10})).await?);
        let documents = found["documents"].as_array().context("find omitted documents")?;
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0]["_id"], 1);
        assert_eq!(documents[0]["amount"], 7);

        let count = structured(call_checked(&owner, &schema, "count", json!({"connectionId":"preconfigured","database":DATABASE,"collection":COLLECTION,"query":{}})).await?);
        assert_eq!(count["count"], 2);

        let aggregate = structured(call_checked(&owner, &schema, "aggregate", json!({"connectionId":"preconfigured","database":DATABASE,"collection":COLLECTION,"pipeline":[{"$match":{"kind":"alpha"}},{"$project":{"_id":1,"kind":1}}]})).await?);
        assert_eq!(aggregate["documents"].as_array().unwrap().len(), 1);
        assert_eq!(aggregate["documents"][0]["kind"], "alpha");

        let aggregate_db = structured(call_checked(&owner, &schema, "aggregate_db", json!({"connectionId":"preconfigured","database":DATABASE,"pipeline":[{"$documents":[{"fixture":"native_mcp"}]}]})).await?);
        assert_eq!(aggregate_db["documents"][0]["fixture"], "native_mcp");
        Ok::<(), anyhow::Error>(())
    })
    .await;

    close_provider(&owner).await;
    provider.stop().await;
    mongo.stop().await;
    result.context("MongoDB MCP native coverage exceeded its deadline")?
}
