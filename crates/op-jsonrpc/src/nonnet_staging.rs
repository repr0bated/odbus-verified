use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use simd_json::{json, OwnedValue as Value};
use tokio::fs;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use crate::state::StateManager;

// Minimal JSON-RPC handler for a read-only, OVSDB-like interface over a unix socket.
// Methods: list_dbs() -> ["OpNonNet"], get_schema(db) -> { tables: {...} }, transact([db, ops]) with select ops only.

pub async fn run_unix_jsonrpc(state: Arc<StateManager>, socket_path: &str) -> Result<()> {
    let p = Path::new(socket_path);
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).await.ok();
    }
    if p.exists() {
        let _ = fs::remove_file(p).await;
    }

    let listener = UnixListener::bind(p).context("bind nonnet DB socket")?;
    loop {
        let (stream, _) = listener.accept().await?;
        let st = Arc::clone(&state);
        tokio::spawn(async move {
            let _ = handle_connection(st, stream).await;
        });
    }
}

async fn handle_connection(state: Arc<StateManager>, stream: UnixStream) -> Result<()> {
    let (r, mut w) = stream.into_split();
    let mut reader = BufReader::new(r);
    let mut line = String::new();
    while reader.read_line(&mut line).await? > 0 {
        let response = match simd_json::from_str::<Value>(&line) {
            Ok(req) => handle_request(&state, req)
                .await
                .unwrap_or_else(|e| json!({"error": e.to_string()})),
            Err(e) => json!({"error": format!("invalid json: {}", e)}),
        };
        let s = simd_json::to_string(&response)?;
        w.write_all(s.as_bytes()).await?;
        w.write_all(b"\n").await?;
        line.clear();
    }
    Ok(())
}

async fn handle_request(state: &Arc<StateManager>, req: Value) -> Result<Value> {
    let id = req.get("id").cloned().unwrap_or(json!(null));
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(json!([]));

    let result = match method {
        "list_dbs" => json!(["OpNonNet"]),
        "get_schema" => {
            // params: [db]
            let _db = params.get(0).and_then(|v| v.as_str()).unwrap_or("OpNonNet");
            let current = state.query_current_state().await?;
            json!({"tables": build_tables_schema(&current.plugins) })
        }
        "transact" => {
            // params: [db, ops]
            let db = params.get(0).and_then(|v| v.as_str()).unwrap_or("OpNonNet");
            let ops = params.get(1).cloned().unwrap_or(json!([]));
            if db != "OpNonNet" {
                json!([{ "error": "unknown db" }])
            } else {
                handle_transact_select(state, ops).await?
            }
        }
        _ => json!({"error": format!("unknown method: {}", method)}),
    };

    Ok(json!({"result": result, "id": id}))
}

fn build_tables_schema(plugins: &HashMap<String, Value>) -> Value {
    let mut tables = simd_json::value::owned::Object::new();
    for (name, val) in plugins {
        if name == "net" {
            continue;
        }
        let columns = infer_columns(val);
        tables.insert(name.clone(), json!({"columns": columns}));
    }
    Value::Object(tables)
}

fn infer_columns(val: &Value) -> Value {
    match val {
        Value::Object(map) => {
            let mut cols = simd_json::value::owned::Object::new();
            for (k, v) in map {
                cols.insert(k.clone(), json!(infer_type(v)));
            }
            Value::Object(cols)
        }
        _ => json!({"value": infer_type(val)}),
    }
}

fn infer_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

async fn handle_transact_select(state: &Arc<StateManager>, ops: Value) -> Result<Value> {
    let mut out = Vec::new();
    let current = state.query_current_state().await?;
    let plugins = &current.plugins;
    if let Some(arr) = ops.as_array() {
        for op in arr {
            let table = op.get("table").and_then(|v| v.as_str()).unwrap_or("");
            if table == "net" {
                out.push(json!({"rows": []}));
                continue;
            }
            let val = plugins.get(table).cloned().unwrap_or(json!(null));
            let rows = rows_from_plugin_value(&val);
            out.push(json!({"rows": rows}));
        }
    }
    Ok(json!(out))
}

fn rows_from_plugin_value(val: &Value) -> Value {
    // Heuristics: if object with a single array field -> rows from that array; if array -> rows = items; else single row.
    match val {
        Value::Object(map) => {
            // find first array member
            if let Some((_, Value::Array(arr))) =
                map.iter().find(|(_, v)| matches!(v, Value::Array(_)))
            {
                let rows: Vec<Value> = arr.clone();
                Value::Array(rows)
            } else {
                Value::Array(vec![val.clone()])
            }
        }
        Value::Array(arr) => Value::Array(arr.clone()),
        _ => Value::Array(vec![val.clone()]),
    }
}
