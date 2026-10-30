use super::*;
use crate::cron_methods::CronMethods;
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{Method, StatusCode, header::AUTHORIZATION},
    response::IntoResponse,
    routing::any,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Mutex;

#[derive(Clone)]
struct NativeFixture {
    jobs: Arc<Mutex<Vec<Value>>>,
    calls: Arc<AtomicUsize>,
}

fn job(id: &str, enabled: bool) -> Value {
    json!({
        "id": id,
        "expression": "*/5 * * * *",
        "schedule": {"kind":"cron", "expr":"*/5 * * * *", "tz":"UTC"},
        "command": "",
        "prompt": "summarize",
        "name": "digest",
        "job_type": "agent",
        "session_target": "isolated",
        "model": null,
        "agent_alias": "operator",
        "enabled": enabled,
        "delivery": {"mode":"none", "channel":null, "to":null, "thread_id":null, "best_effort":true},
        "delete_after_run": false,
        "allowed_tools": ["file_read"],
        "uses_memory": false,
        "source": "imperative",
        "shell_output_format": "wrapped",
        "created_at": "2026-09-13T00:00:00Z",
        "next_run": "2026-09-13T00:05:00Z",
        "last_run": null,
        "last_status": null,
        "last_output": null
    })
}

async fn native_gateway(State(state): State<NativeFixture>, request: Request) -> impl IntoResponse {
    if request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        != Some("Bearer fixture-token")
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"unauthorized"})),
        );
    }
    state.calls.fetch_add(1, Ordering::SeqCst);
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let body = to_bytes(request.into_body(), 1024 * 1024)
        .await
        .expect("bounded fixture body");
    let parsed: Value = if body.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(&body).expect("JSON body")
    };
    let mut jobs = state.jobs.lock().await;
    let response = match (method, path.as_str()) {
        (Method::GET, "/api/cron") => json!({"jobs": jobs.clone()}),
        (Method::GET, "/api/cron/one/runs") => json!({"runs":[{
            "id":1, "job_id":"one", "started_at":"2026-09-13T00:00:00Z",
            "finished_at":"2026-09-13T00:00:01Z", "status":"ok",
            "output":"private output", "duration_ms":1
        }]}),
        (Method::POST, "/api/cron") => {
            let mut created = job("created", true);
            created["agent_alias"] = parsed["agent"].clone();
            created["name"] = parsed.get("name").cloned().unwrap_or(Value::Null);
            created["expression"] = parsed["schedule"].clone();
            created["schedule"] = json!({
                "kind":"cron",
                "expr":parsed["schedule"],
                "tz":parsed.get("tz").cloned().unwrap_or(Value::Null)
            });
            created["prompt"] = parsed.get("prompt").cloned().unwrap_or(Value::Null);
            created["command"] = parsed.get("command").cloned().unwrap_or(json!(""));
            // `CreateJobInput` serializes an absent Option as JSON null. The
            // native `CronAddBody` deserializes that as None and applies the
            // documented true default; the fixture must preserve that wire
            // behavior instead of storing null in the required bool field.
            created["uses_memory"] = parsed
                .get("uses_memory")
                .filter(|value| !value.is_null())
                .cloned()
                .unwrap_or(json!(true));
            jobs.push(created.clone());
            json!({"status":"ok", "job":created})
        }
        (Method::PATCH, "/api/cron/one") => {
            let current = jobs.iter_mut().find(|value| value["id"] == "one").unwrap();
            if let Some(enabled) = parsed.get("enabled") {
                current["enabled"] = enabled.clone();
            }
            if let Some(name) = parsed.get("name") {
                current["name"] = name.clone();
            }
            json!({"status":"ok", "job":current.clone()})
        }
        (Method::POST, "/api/cron/one/run") => json!({
            "status":"ok", "job_id":"one", "success":true,
            "output":"private output", "duration_ms":1,
            "started_at":"2026-09-13T00:00:00Z", "finished_at":"2026-09-13T00:00:01Z"
        }),
        (Method::DELETE, "/api/cron/created") => {
            jobs.retain(|value| value["id"] != "created");
            json!({"status":"ok"})
        }
        _ => return (StatusCode::NOT_FOUND, Json(json!({"error":"not found"}))),
    };
    (StatusCode::OK, Json(response))
}

#[test]
fn dynamic_job_id_is_one_encoded_path_segment() {
    assert_eq!(encoded_segment("job/with ?#%"), "job%2Fwith%20%3F%23%25");
}

#[tokio::test]
async fn real_bounded_transport_maps_all_nine_canonical_methods() {
    let fixture = NativeFixture {
        jobs: Arc::new(Mutex::new(vec![job("one", true)])),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server_fixture = fixture.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .fallback(any(native_gateway))
                .with_state(server_fixture),
        )
        .await
        .unwrap();
    });
    let gateway = ChatRouterCronGateway {
        client: ChatRouterRuntimeClient::new(
            format!("http://{address}"),
            "operator".into(),
            Some("fixture-token".into()),
        ),
    };
    let methods = CronMethods::new(Arc::new(gateway));

    methods.dispatch("list_jobs", &json!({})).await.unwrap();
    methods
        .dispatch("get_job", &json!({"job_id":"one"}))
        .await
        .unwrap();
    methods
        .dispatch("get_history", &json!({"job_id":"one","limit":1}))
        .await
        .unwrap();
    methods.dispatch("create_job", &json!({
        "job_type":"agent", "agent":"operator", "schedule":"*/5 * * * *", "prompt":"summarize"
    })).await.unwrap();
    methods
        .dispatch("update_job", &json!({"job_id":"one","name":"renamed"}))
        .await
        .unwrap();
    methods
        .dispatch("disable_job", &json!({"job_id":"one"}))
        .await
        .unwrap();
    methods
        .dispatch("enable_job", &json!({"job_id":"one"}))
        .await
        .unwrap();
    methods
        .dispatch("run_job", &json!({"job_id":"one"}))
        .await
        .unwrap();
    methods
        .dispatch("delete_job", &json!({"job_id":"created"}))
        .await
        .unwrap();

    assert_eq!(fixture.calls.load(Ordering::SeqCst), 11);
    server.abort();
}
