use super::*;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UnixStream};

const FIXTURE_KEY: &str = "fixture-key";
const FIXTURE_SECRET: &str = "fixture-secret";

#[derive(Clone, Copy)]
enum ApiMode {
    Healthy = 0,
    Unauthorized = 1,
    PublicListener = 2,
    UnauthenticatedHook = 3,
    DefaultListeners = 4,
    ActivePortZero = 5,
    NodeOnlyActivePortZero = 6,
}

#[derive(Debug, Clone)]
struct HttpRequest {
    method: String,
    target: String,
    authorized: bool,
    body: Option<Value>,
}

struct HttpFixture {
    api_base: String,
    requests: Arc<Mutex<Vec<HttpRequest>>>,
    mode: Arc<AtomicU8>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for HttpFixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl HttpFixture {
    async fn spawn(mode: ApiMode) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let mode = Arc::new(AtomicU8::new(mode as u8));
        let task_requests = requests.clone();
        let task_mode = mode.clone();
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                serve_http_request(stream, task_requests.clone(), task_mode.clone()).await;
            }
        });
        Self {
            api_base: format!("http://{address}/api/v5"),
            requests,
            mode,
            task,
        }
    }

    fn set_mode(&self, mode: ApiMode) {
        self.mode.store(mode as u8, Ordering::SeqCst);
    }

    fn requests(&self) -> Vec<HttpRequest> {
        self.requests.lock().unwrap().clone()
    }
}

async fn serve_http_request(
    mut stream: TcpStream,
    requests: Arc<Mutex<Vec<HttpRequest>>>,
    mode: Arc<AtomicU8>,
) {
    let Some((head, body)) = read_http_request(&mut stream).await else {
        return;
    };
    let mut lines = head.lines();
    let Some(request_line) = lines.next() else {
        return;
    };
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default().to_string();
    let target = request_parts.next().unwrap_or_default().to_string();
    let expected_auth = "Basic Zml4dHVyZS1rZXk6Zml4dHVyZS1zZWNyZXQ=";
    let authorized = lines.any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("authorization") && value.trim() == expected_auth
        })
    });
    let body = if body.is_empty() {
        None
    } else {
        serde_json::from_slice::<Value>(&body).ok()
    };
    requests.lock().unwrap().push(HttpRequest {
        method: method.clone(),
        target: target.clone(),
        authorized,
        body,
    });

    let mode = mode.load(Ordering::SeqCst);
    if !authorized || mode == ApiMode::Unauthorized as u8 {
        write_http_response(&mut stream, 401, json!({"code":"UNAUTHORIZED"})).await;
        return;
    }

    let listener_bind = if mode == ApiMode::PublicListener as u8 {
        "0.0.0.0:1883"
    } else if mode == ApiMode::DefaultListeners as u8
        || mode == ApiMode::ActivePortZero as u8
        || mode == ApiMode::NodeOnlyActivePortZero as u8
    {
        ":0"
    } else {
        "127.0.0.1:1883"
    };
    let hook_verify = if mode == ApiMode::UnauthenticatedHook as u8 {
        "verify_none"
    } else {
        "verify_peer"
    };
    let hooks = REGISTERED_HOOKS
        .iter()
        .map(|name| json!({"name": name}))
        .collect::<Vec<_>>();
    let response = match (method.as_str(), target.as_str()) {
        ("GET", "/api/v5/status?format=json") => {
            json!({"node_name":"emqx@127.0.0.1","app_status":"running"})
        }
        ("GET", "/api/v5/nodes") => json!({"data":[{
            "node":"emqx@127.0.0.1",
            "node_status":"running",
            "version":EMQX_RELEASE_VERSION
        }]}),
        ("GET", "/api/v5/listeners") if mode == ApiMode::DefaultListeners as u8 => json!({"data":[
            {"id":"tcp:default","type":"tcp","bind":":0","enable":false,
             "status":{"running":false,"max_connections":"infinity","current_connections":0},
             "node_status":[{"node":"emqx@127.0.0.1","status":{"running":false,"max_connections":"infinity","current_connections":0}}]},
            {"id":"tcp:active","type":"tcp","bind":"127.0.0.1:1883","enable":true,
             "status":{"running":true,"max_connections":1024,"current_connections":3},
             "node_status":[{"node":"emqx@127.0.0.1","status":{"running":true,"max_connections":1024,"current_connections":3}}]}
        ]}),
        ("GET", "/api/v5/listeners") if mode == ApiMode::ActivePortZero as u8 => json!({"data":[
            {"id":"tcp:default","type":"tcp","bind":":0","enable":true,
             "status":{"running":true,"max_connections":1024,"current_connections":0},
             "node_status":[{"node":"emqx@127.0.0.1","status":{"running":true,"max_connections":1024,"current_connections":0}}]}
        ]}),
        ("GET", "/api/v5/listeners") if mode == ApiMode::NodeOnlyActivePortZero as u8 => {
            json!({"data":[
                {"id":"tcp:default","type":"tcp","bind":":0","enable":false,
                 "node_status":[{"node":"emqx@127.0.0.1","status":{"running":true,"max_connections":1024,"current_connections":0}}]}
            ]})
        }
        ("GET", "/api/v5/listeners") => json!({"data":[{
            "id":"tcp:default",
            "type":"tcp",
            "bind":listener_bind,
            "running":true,
            "current_connections":3,
            "max_connections":1024
        }]}),
        ("GET", "/api/v5/exhooks") => json!({"data":[{
            "name":EXHOOK_SERVER_NAME,
            "url":EXHOOK_TARGET,
            "enable":true,
            "ssl":{
                "enable":true,
                "verify":hook_verify,
                "cacertfile":"/etc/emqx/certs/opdbus-ca.crt",
                "certfile":"/etc/emqx/certs/emqx-exhook-client.crt",
                "keyfile":"/etc/emqx/certs/emqx-exhook-client.key"
            },
            "node_status":[{"node":"emqx@127.0.0.1","status":"connected"}],
            "hooks":hooks
        }]}),
        ("PUT", "/api/v5/exhooks/opdbus") => json!({}),
        _ => {
            write_http_response(&mut stream, 404, json!({"code":"NOT_FOUND"})).await;
            return;
        }
    };
    write_http_response(&mut stream, 200, response).await;
}

async fn read_http_request(stream: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    const MAX_REQUEST: usize = 64 * 1024;
    let mut bytes = Vec::new();
    let header_end = loop {
        if let Some(offset) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break offset + 4;
        }
        if bytes.len() >= MAX_REQUEST {
            return None;
        }
        let mut buffer = [0_u8; 4096];
        let read = stream.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        bytes.extend_from_slice(&buffer[..read]);
    };
    let head = String::from_utf8(bytes[..header_end].to_vec()).ok()?;
    let content_length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);
    if header_end.saturating_add(content_length) > MAX_REQUEST {
        return None;
    }
    while bytes.len() < header_end + content_length {
        let mut buffer = [0_u8; 4096];
        let read = stream.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    Some((
        head,
        bytes[header_end..header_end + content_length].to_vec(),
    ))
}

async fn write_http_response(stream: &mut TcpStream, status: u16, body: Value) {
    let body = serde_json::to_vec(&body).unwrap();
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        _ => "Not Found",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(&body).await.unwrap();
    stream.shutdown().await.unwrap();
}

struct RunitFixtureState {
    running: bool,
    reject_lifecycle: bool,
    calls: Vec<String>,
}

#[derive(Clone)]
struct RunitFixture {
    state: Arc<Mutex<RunitFixtureState>>,
}

impl RunitFixture {
    fn lifecycle(&self, action: &str, service: &str, running: bool) -> (bool, String) {
        let mut state = self.state.lock().unwrap();
        state.calls.push(format!("{action}:{service}"));
        if state.reject_lifecycle {
            return (false, "fixture rejected lifecycle request".to_string());
        }
        state.running = running;
        (true, format!("fixture accepted {action}"))
    }
}

#[zbus::interface(name = "org.opdbus.v1.Runit.Systemctl")]
impl RunitFixture {
    fn status(&self, service: &str) -> String {
        let mut state = self.state.lock().unwrap();
        state.calls.push(format!("Status:{service}"));
        if state.running {
            json!({
                "name":service,
                "active_state":"active",
                "sub_state":"running",
                "main_pid":4242,
                "up_time":"17s"
            })
            .to_string()
        } else {
            json!({
                "name":service,
                "active_state":"inactive",
                "sub_state":"down",
                "main_pid":null,
                "up_time":"0s"
            })
            .to_string()
        }
    }

    fn start(&self, service: &str) -> (bool, String) {
        self.lifecycle("Start", service, true)
    }

    fn stop(&self, service: &str) -> (bool, String) {
        self.lifecycle("Stop", service, false)
    }

    fn restart(&self, service: &str) -> (bool, String) {
        self.lifecycle("Restart", service, true)
    }
}

struct NativeFixture {
    transport: EmqxNativeTransport,
    http: HttpFixture,
    runit_state: Arc<Mutex<RunitFixtureState>>,
    _dbus_server: Connection,
}

impl NativeFixture {
    async fn spawn(running: bool, reject_lifecycle: bool, api_mode: ApiMode) -> Self {
        let http = HttpFixture::spawn(api_mode).await;
        let runit_state = Arc::new(Mutex::new(RunitFixtureState {
            running,
            reject_lifecycle,
            calls: Vec::new(),
        }));
        let (server_stream, client_stream) = UnixStream::pair().unwrap();
        let server = zbus::connection::Builder::unix_stream(server_stream)
            .server(zbus::Guid::generate())
            .unwrap()
            .p2p()
            .serve_at(
                RUNIT_OBJECT_PATH,
                RunitFixture {
                    state: runit_state.clone(),
                },
            )
            .unwrap()
            .build();
        let client = zbus::connection::Builder::unix_stream(client_stream)
            .p2p()
            .build();
        let (_dbus_server, client) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::try_join!(server, client)
        })
        .await
        .expect("D-Bus fixture handshake timed out")
        .unwrap();
        let transport = EmqxNativeTransport {
            runit_connection: Some(client),
            api_base: http.api_base.clone(),
            api_credentials: Some((FIXTURE_KEY.to_string(), FIXTURE_SECRET.to_string())),
        };
        Self {
            transport,
            http,
            runit_state,
            _dbus_server,
        }
    }

    async fn call(&self, method: &str, args: Value) -> Result<Value> {
        let schema = emqx_schema();
        let declaration = &schema.methods[method];
        let input_schema = serde_json::to_value(&declaration.args)?;
        jsonschema::validator_for(&input_schema)?
            .validate(&args)
            .map_err(|error| anyhow::anyhow!("EMQX {method} input contract: {error}"))?;
        let output = dispatch_emqx_method_on(&self.transport, method, &args).await?;
        let output_schema = serde_json::to_value(declaration.returns.as_ref().unwrap())?;
        jsonschema::validator_for(&output_schema)?
            .validate(&output)
            .map_err(|error| anyhow::anyhow!("EMQX {method} output contract: {error}"))?;
        Ok(output)
    }

    fn runit_calls(&self) -> Vec<String> {
        self.runit_state.lock().unwrap().calls.clone()
    }
}

#[tokio::test]
async fn retained_read_methods_use_authenticated_native_readback() {
    let fixture = NativeFixture::spawn(true, false, ApiMode::Healthy).await;

    let mcp = fixture.call("get_mcp_status", json!({})).await.unwrap();
    assert_eq!(mcp["gateway_installed"], false);
    assert_eq!(mcp["internal_transport_only"], true);
    assert_eq!(mcp["external_mcp_endpoint"], "https://10.0.0.3:8090/mcp");

    let status = fixture.call("get_status", json!({})).await.unwrap();
    assert_eq!(status["running"], true);
    assert_eq!(status["pid"], 4242);
    assert_eq!(status["uptime_seconds"], 17);
    assert_eq!(status["version"], EMQX_RELEASE_VERSION);
    assert_eq!(status["listeners_active"], 1);
    assert_eq!(status["connections_current"], 3);

    let listeners = fixture.call("list_listeners", json!({})).await.unwrap();
    assert_eq!(listeners["total_connections"], 3);
    assert_eq!(listeners["all_loopback_or_uds"], true);
    assert_eq!(listeners["listeners"][0]["bind"], EMQX_MQTT_TCP_BIND);

    let hooks = fixture.call("list_hooks", json!({})).await.unwrap();
    assert_eq!(hooks["total_hooks"], REGISTERED_HOOKS.len());
    assert_eq!(hooks["exhook_servers"][0]["transport_authenticated"], true);

    let hook = fixture
        .call("get_hook_status", json!({"server_name":EXHOOK_SERVER_NAME}))
        .await
        .unwrap();
    assert_eq!(hook["found"], true);
    assert_eq!(hook["server"]["name"], EXHOOK_SERVER_NAME);

    let requests = fixture.http.requests();
    assert_eq!(
        requests
            .iter()
            .map(|request| request.target.as_str())
            .collect::<Vec<_>>(),
        [
            "/api/v5/status?format=json",
            "/api/v5/nodes",
            "/api/v5/listeners",
            "/api/v5/listeners",
            "/api/v5/exhooks",
            "/api/v5/exhooks",
        ]
    );
    assert!(requests.iter().all(|request| request.authorized));
    assert!(requests.iter().all(|request| request.method == "GET"));
    assert!(
        fixture
            .runit_calls()
            .iter()
            .all(|call| call == "Status:emqx")
    );
}

#[tokio::test]
async fn configure_hooks_sends_fixed_mtls_shape_and_verifies_readback() {
    let fixture = NativeFixture::spawn(true, false, ApiMode::Healthy).await;
    let hooks = REGISTERED_HOOKS
        .iter()
        .map(|hook| (*hook).to_string())
        .collect::<Vec<_>>();
    let output = fixture
        .call(
            "configure_hooks",
            json!({
                "server_name":EXHOOK_SERVER_NAME,
                "target":"loopback_mtls",
                "enable":true,
                "hooks":hooks
            }),
        )
        .await
        .unwrap();
    assert_eq!(output["configured"], true);
    assert_eq!(output["server_name"], EXHOOK_SERVER_NAME);
    assert_eq!(output["is_local"], true);

    let requests = fixture.http.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "PUT");
    assert_eq!(requests[0].target, "/api/v5/exhooks/opdbus");
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].target, "/api/v5/exhooks");
    assert!(requests.iter().all(|request| request.authorized));
    let body = requests[0].body.as_ref().unwrap();
    assert_eq!(body["name"], EXHOOK_SERVER_NAME);
    assert_eq!(body["url"], EXHOOK_TARGET);
    assert_eq!(body["enable"], true);
    assert_eq!(body["ssl"]["verify"], "verify_peer");
    assert_eq!(
        body["ssl"]["certfile"],
        "/etc/emqx/certs/emqx-exhook-client.crt"
    );
    assert!(!body.to_string().contains(FIXTURE_SECRET));

    let request_count = requests.len();
    let error = fixture
        .call(
            "configure_hooks",
            json!({
                "server_name":EXHOOK_SERVER_NAME,
                "target":"loopback_mtls",
                "enable":true,
                "hooks":["message.publish"]
            }),
        )
        .await
        .expect_err("partial hook set must be rejected before the API call");
    assert!(error.to_string().contains("exact supported"));
    assert_eq!(fixture.http.requests().len(), request_count);
}

#[tokio::test]
async fn listeners_accept_disabled_port_zero_and_read_nested_node_status() {
    let fixture = NativeFixture::spawn(true, false, ApiMode::DefaultListeners).await;
    let listeners = fixture.call("list_listeners", json!({})).await.unwrap();
    assert_eq!(listeners["total_connections"], 3);
    assert_eq!(listeners["all_loopback_or_uds"], true);
    assert_eq!(listeners["listeners"][0]["bind"], ":0");
    assert_eq!(listeners["listeners"][0]["running"], false);
    assert_eq!(listeners["listeners"][1]["running"], true);
    assert_eq!(listeners["listeners"][1]["current_connections"], 3);
}

#[tokio::test]
async fn listeners_reject_active_port_zero_endpoints() {
    let fixture = NativeFixture::spawn(true, false, ApiMode::ActivePortZero).await;
    let error = fixture
        .call("list_listeners", json!({}))
        .await
        .expect_err("active port-zero listener must fail closed");
    assert!(error.to_string().contains("outside loopback/UDS"));
}

#[tokio::test]
async fn node_status_running_overrides_disabled_port_zero_config() {
    let fixture = NativeFixture::spawn(true, false, ApiMode::NodeOnlyActivePortZero).await;
    let error = fixture
        .call("list_listeners", json!({}))
        .await
        .expect_err("node status must reject an active port-zero listener");
    assert!(error.to_string().contains("outside loopback/UDS"));
}

#[tokio::test]
async fn lifecycle_methods_use_fixed_dbus_members_and_observe_status() {
    let fixture = NativeFixture::spawn(false, false, ApiMode::Healthy).await;

    let started = fixture.call("start", json!({})).await.unwrap();
    assert_eq!(started["started"], true);
    assert_eq!(started["pid"], 4242);
    assert_eq!(started["node_name"], "emqx@127.0.0.1");

    let stopped = fixture.call("stop", json!({})).await.unwrap();
    assert_eq!(stopped["stopped"], true);
    assert_eq!(stopped["was_running"], true);

    let restarted = fixture.call("restart", json!({})).await.unwrap();
    assert_eq!(restarted["restarted"], true);
    assert_eq!(restarted["previous_uptime"], 0);
    assert_eq!(restarted["node_name"], "emqx@127.0.0.1");

    let calls = fixture.runit_calls();
    assert!(calls.iter().any(|call| call == "Start:emqx"));
    assert!(calls.iter().any(|call| call == "Stop:emqx"));
    assert!(calls.iter().any(|call| call == "Restart:emqx"));
    assert!(calls.iter().all(|call| call.ends_with(EMQX_SERVICE)));
    let requests = fixture.http.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.target == "/api/v5/status?format=json")
            .count(),
        2
    );
    assert!(requests.iter().all(|request| request.authorized));
}

#[tokio::test]
async fn native_auth_and_lifecycle_rejections_fail_without_false_success() {
    let unauthorized = NativeFixture::spawn(true, false, ApiMode::Unauthorized).await;
    let error = unauthorized
        .call("get_status", json!({}))
        .await
        .expect_err("API authentication failure must surface");
    let diagnostic = error.to_string();
    assert!(diagnostic.contains("HTTP 401"));
    assert!(!diagnostic.contains(FIXTURE_KEY));
    assert!(!diagnostic.contains(FIXTURE_SECRET));

    let rejected = NativeFixture::spawn(false, true, ApiMode::Healthy).await;
    let error = rejected
        .call("start", json!({}))
        .await
        .expect_err("D-Bus lifecycle rejection must surface");
    assert!(error.to_string().contains("rejected EMQX Start"));
    assert!(rejected.http.requests().is_empty());
    assert!(
        rejected
            .runit_calls()
            .iter()
            .any(|call| call == "Start:emqx")
    );

    let unreadable = NativeFixture::spawn(false, false, ApiMode::Unauthorized).await;
    let error = unreadable
        .call("start", json!({}))
        .await
        .expect_err("accepted lifecycle without authenticated health readback must fail");
    assert!(
        error
            .to_string()
            .contains("accepted but broker health readback failed")
    );
    assert!(
        unreadable
            .runit_calls()
            .iter()
            .any(|call| call == "Start:emqx")
    );
}

#[tokio::test]
async fn native_inventory_policy_violations_fail_closed() {
    let fixture = NativeFixture::spawn(true, false, ApiMode::PublicListener).await;
    let listener_error = fixture
        .call("list_listeners", json!({}))
        .await
        .expect_err("public listener must violate local transport policy");
    assert!(listener_error.to_string().contains("outside loopback/UDS"));

    fixture.http.set_mode(ApiMode::UnauthenticatedHook);
    let hook_error = fixture
        .call("list_hooks", json!({}))
        .await
        .expect_err("unauthenticated ExHook must violate transport policy");
    assert!(
        hook_error
            .to_string()
            .contains("unauthenticated or non-local")
    );
}
