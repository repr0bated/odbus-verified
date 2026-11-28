//! End-to-end MCP flows over the axum router, against a tempdir call-file
//! tree with a fake FUSE engine (no socket of any kind behind the door).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use http_body_util::BodyExt;
use op_identity::oracle_assertion::{OracleIdentityAssertion, SignedAssertion};
use op_mcp_door::audit::AuditLog;
use op_mcp_door::config::Config;
use op_mcp_door::fusecall::testing::{FakeOpener, declare_mount, receipt_for};
use op_mcp_door::grants::{EphemeralGrant, GrantStore, Ticket, TicketStore, now_unix};
use op_mcp_door::mcp::{Door, router};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::sync::Arc;
use tower::ServiceExt;

const PUBKEY: &str = "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=";

fn write_op(root: &Path, plugin: &str, method: &str, mutation: bool, props: Value, cap: &str) {
    let dir = root
        .join("plugins")
        .join(plugin)
        .join("operations")
        .join(method);
    std::fs::create_dir_all(&dir).unwrap();
    let args =
        json!({"type":"object","description":format!("{plugin} {method}"),"properties":props});
    let meta = json!({"name":method,"args":args,"returns":{"type":"object"},
        "side_effect": if mutation {"mutation"} else {"read"}, "idempotent": !mutation,
        "required_capability":cap,"subid":format!("x.{plugin}.{method}@v1")});
    std::fs::write(dir.join("metadata.json"), meta.to_string()).unwrap();
    std::fs::write(dir.join("input.schema.json"), args.to_string()).unwrap();
    std::fs::write(dir.join("call"), "").unwrap();
}

struct World {
    _dir: tempfile::TempDir,
    door: Arc<Door>,
    opener: Arc<FakeOpener>,
    state: std::path::PathBuf,
}

fn world() -> World {
    world_with(&[], false)
}

/// `vector_tools` adds the hot coding and knowledge tools; `extra` adds
/// environment (e.g. the Qdrant URL the readiness gate probes).
fn world_with(extra: &[(&'static str, String)], vector_tools: bool) -> World {
    let dir = tempfile::tempdir().unwrap();
    let mount = dir.path().join("mount");
    let mode = json!({"kind":"guest_kernel"});
    declare_mount(&mount, mode.clone());
    write_op(
        &mount,
        "cognitive",
        "memory_recall",
        false,
        json!({"namespace":{"type":"string"}}),
        "cognitive.read",
    );
    write_op(
        &mount,
        "cognitive",
        "memory_store",
        true,
        json!({"key":{"type":"string"}}),
        "cognitive.write",
    );
    write_op(
        &mount,
        "cognitive",
        "schema_read",
        false,
        json!({"plugin":{"type":"string"}}),
        "cognitive.read",
    );
    write_op(
        &mount,
        "cognitive",
        "agent_bash_pro_exec",
        true,
        json!({"command":{"type":"string"}}),
        "cognitive.agent.bash_pro",
    );
    if vector_tools {
        write_op(
            &mount,
            "cognitive",
            "code_search",
            false,
            json!({"query":{"type":"string"}}),
            "cognitive.read",
        );
        write_op(
            &mount,
            "snowball",
            "search_own_events",
            false,
            json!({"query":{"type":"string"}}),
            "cap.obs.snowball.events.self.search@v1",
        );
    }
    for i in 0..60 {
        write_op(
            &mount,
            "lxd",
            &format!("get_thing_{i:02}"),
            false,
            json!({}),
            "lxd.read",
        );
    }
    let config_dir = dir.path().join("etc");
    std::fs::create_dir_all(&config_dir).unwrap();
    let spec = json!({"path": mount, "mode": mode});
    std::fs::write(
        config_dir.join("mounts.json"),
        json!({"default": spec, "principals": {"p-1": spec}}).to_string(),
    )
    .unwrap();
    std::fs::write(
        config_dir.join("principals.json"),
        json!({PUBKEY: "p-1"}).to_string(),
    )
    .unwrap();
    let verifying = base64::engine::general_purpose::STANDARD
        .encode(SigningKey::from_bytes(&[7; 32]).verifying_key().to_bytes());
    std::fs::write(
        config_dir.join("oia1-keys.json"),
        json!({"k1": verifying}).to_string(),
    )
    .unwrap();
    let grants_doc = dir.path().join("grants.json");
    std::fs::write(
        &grants_doc,
        json!({"p-1": {"capabilities": ["cognitive.read", "cognitive.write", "lxd.read",
            "cap.obs.snowball.events.self.search@v1"]}})
        .to_string(),
    )
    .unwrap();
    let state = dir.path().join("state");
    let mut env: HashMap<&str, String> = HashMap::from([
        ("OP_MCP_DOOR_CONFIG_DIR", config_dir.display().to_string()),
        ("OP_MCP_DOOR_STATE_DIR", state.display().to_string()),
        (
            "OP_MCP_DOOR_GRANTS_DOCUMENT",
            grants_doc.display().to_string(),
        ),
        (
            "OP_MCP_TOOLSETS_PATH",
            dir.path()
                .join("absent-toolsets.json")
                .display()
                .to_string(),
        ),
        (
            "OP_MCP_DOOR_ALLOWED_ORIGINS",
            "https://allowed.example".into(),
        ),
        ("OP_MCP_DOOR_PUBLIC_RATE", "3".into()),
    ]);
    env.extend(extra.iter().cloned());
    let config = Config::from_lookup(|k| env.get(k).cloned()).unwrap();
    let opener = Arc::new(FakeOpener::new(|path, _written| {
        let meta: Value = serde_json::from_slice(&std::fs::read(
            path.parent().unwrap().join("metadata.json"),
        )?)
        .unwrap();
        let plugin = path
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        Ok(receipt_for(&plugin, &meta, json!({"ok": true})))
    }));
    let (door, _warnings) =
        op_mcp_door::app::build_door(&config, AuditLog::memory(), opener.clone()).unwrap();
    World {
        _dir: dir,
        door: Arc::new(door),
        opener,
        state,
    }
}

fn assertion(nonce: u8) -> String {
    let now = now_unix();
    let assertion = OracleIdentityAssertion {
        human_pubkey: PUBKEY.into(),
        issued_at: now - 5,
        expires_at: now + 600,
        nonce: [nonce; 16],
        tunnel_inner_ip: IpAddr::V4(Ipv4Addr::new(10, 200, 0, 7)),
        decoy_key_id: "k1".into(),
    };
    let signature = SigningKey::from_bytes(&[7; 32])
        .sign(&assertion.signing_bytes())
        .to_bytes();
    base64::engine::general_purpose::STANDARD.encode(
        SignedAssertion {
            assertion,
            signature,
        }
        .to_wire(),
    )
}

struct Reply {
    status: StatusCode,
    session: Option<String>,
    body: Value,
}

async fn send(w: &World, method: &str, path: &str, headers: &[(&str, &str)], body: Value) -> Reply {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    for (k, v) in headers {
        request = request.header(*k, *v);
    }
    let response = router(w.door.clone())
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let session = response
        .headers()
        .get("mcp-session-id")
        .map(|v| v.to_str().unwrap().to_owned());
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    Reply {
        status,
        session,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}

struct Client<'a> {
    w: &'a World,
    path: String,
    session: String,
    auth: Option<String>,
    next: i64,
}

impl<'a> Client<'a> {
    async fn open(w: &'a World, path: &str, auth: Option<String>) -> Client<'a> {
        let mut headers = Vec::new();
        if let Some(a) = &auth {
            headers.push(("x-oracle-identity-assertion-bin", a.as_str()));
        }
        let r = send(w, "POST", path, &headers, json!({"jsonrpc":"2.0","id":0,"method":"initialize",
            "params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}})).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
        assert_eq!(r.body["result"]["protocolVersion"], "2025-06-18");
        Client {
            w,
            path: path.to_owned(),
            session: r.session.expect("session id"),
            auth,
            next: 1,
        }
    }

    async fn rpc(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let mut headers = vec![("mcp-session-id", self.session.as_str())];
        if let Some(a) = &self.auth {
            headers.push(("x-oracle-identity-assertion-bin", a.as_str()));
        }
        let r = send(
            self.w,
            "POST",
            &self.path,
            &headers,
            json!({"jsonrpc":"2.0","id":self.next,"method":method,"params":params}),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK);
        r.body
    }

    async fn names(&mut self) -> Vec<String> {
        let body = self.rpc("tools/list", json!({})).await;
        body["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect()
    }

    async fn call(&mut self, name: &str, args: Value) -> Value {
        self.rpc("tools/call", json!({"name":name,"arguments":args}))
            .await
    }
}

fn text(result: &Value) -> Value {
    let text = result["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("null");
    serde_json::from_str(text).unwrap_or(Value::String(text.to_owned()))
}

#[tokio::test]
async fn anonymous_callers_see_and_reach_only_public_tools() {
    let w = world();
    let mut c = Client::open(&w, "/mcp/full", None).await;
    let names = c.names().await;
    assert_eq!(names, vec!["schema_read", "list_toolsets", "load_toolset"]);
    let denied = c.call("memory_recall", json!({})).await;
    assert_eq!(denied["error"]["message"], "forbidden_or_unknown");
    let unknown = c.call("no_such_tool", json!({})).await;
    assert_eq!(
        unknown["error"], denied["error"],
        "hidden and unknown are indistinguishable"
    );
    assert_eq!(w.opener.writes(), 0);
    let ok = c.call("schema_read", json!({"plugin":"memory"})).await;
    assert_eq!(ok["result"]["isError"], false, "{ok}");
    assert_eq!(text(&ok)["event_id"], 7);
    assert_eq!(w.opener.writes(), 1);
    // Public calls are rate-limited per client address (3/minute here).
    for _ in 0..2 {
        c.call("schema_read", json!({})).await;
    }
    let limited = c.call("schema_read", json!({})).await;
    assert_eq!(text(&limited), "rate_limited");
    assert_eq!(w.opener.writes(), 3);
}

#[tokio::test]
async fn authenticated_full_endpoint_lists_granted_core_and_enforces_confirmation() {
    let w = world();
    let mut c = Client::open(&w, "/mcp/full", Some(assertion(1))).await;
    let names = c.names().await;
    assert_eq!(
        names,
        vec![
            "memory_store",
            "memory_recall",
            "schema_read",
            "list_toolsets",
            "load_toolset"
        ]
    );
    assert!(
        !names.iter().any(|n| n.contains("bash")),
        "shell never listed by default"
    );
    let unconfirmed = c.call("memory_store", json!({"key":"k"})).await;
    assert_eq!(text(&unconfirmed), "confirmation_required");
    assert_eq!(w.opener.writes(), 0);
    let confirmed = c
        .call(
            "memory_store",
            json!({"key":"k","confirm_side_effects":true}),
        )
        .await;
    assert_eq!(confirmed["result"]["isError"], false, "{confirmed}");
    let written = w.opener.log.lock().unwrap().writes[0].1.clone();
    assert_eq!(
        written.as_slice(),
        br#"{"key":"k"}"#,
        "confirmation flag never reaches the call file"
    );
    let invalid = c.call("memory_recall", json!({"namespace": 5})).await;
    assert!(
        text(&invalid)
            .as_str()
            .unwrap()
            .starts_with("invalid_arguments")
    );
    assert_eq!(w.opener.writes(), 1);
}

#[tokio::test]
async fn loader_has_five_meta_tools_and_call_tool_matches_direct_calls() {
    let w = world();
    let mut anon = Client::open(&w, "/mcp/loader", None).await;
    assert_eq!(
        anon.names().await,
        vec![
            "list_toolsets",
            "search_tools",
            "describe_tool",
            "call_tool",
            "load_toolset"
        ]
    );
    let search = anon.call("search_tools", json!({"query":"memory"})).await;
    assert_eq!(text(&search)["tools"], json!([]));
    let describe = anon
        .call("describe_tool", json!({"name":"memory_recall"}))
        .await;
    assert_eq!(describe["error"]["message"], "forbidden_or_unknown");
    let via = anon
        .call("call_tool", json!({"name":"memory_recall","arguments":{}}))
        .await;
    assert_eq!(via["error"]["message"], "forbidden_or_unknown");
    let direct_hidden = anon.call("memory_recall", json!({})).await;
    assert_eq!(
        direct_hidden["error"]["message"], "forbidden_or_unknown",
        "no direct calls past the meta-tools"
    );

    let mut c = Client::open(&w, "/mcp/loader", Some(assertion(2))).await;
    let search = c.call("search_tools", json!({"query":"memory"})).await;
    let found: Vec<String> = text(&search)["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect();
    assert!(
        found.contains(&"memory_recall".to_owned()) && found.contains(&"memory_store".to_owned())
    );
    let describe = c
        .call("describe_tool", json!({"name":"memory_store"}))
        .await;
    assert_eq!(text(&describe)["needs_confirmation"], true);
    let unconfirmed = c
        .call(
            "call_tool",
            json!({"name":"memory_store","arguments":{"key":"k"}}),
        )
        .await;
    assert_eq!(text(&unconfirmed), "confirmation_required");
    let ok = c
        .call(
            "call_tool",
            json!({"name":"memory_store","arguments":{"key":"k"},"confirm_side_effects":true}),
        )
        .await;
    assert_eq!(ok["result"]["isError"], false);
    let shell = c.call("call_tool", json!({"name":"cognitive__agent_bash_pro_exec","arguments":{"command":"id"},"confirm_side_effects":true})).await;
    assert_eq!(
        shell["error"]["message"], "forbidden_or_unknown",
        "no ephemeral grant: hidden"
    );
    let shell_desc = c
        .call(
            "describe_tool",
            json!({"name":"cognitive.agent_bash_pro_exec"}),
        )
        .await;
    assert_eq!(shell_desc["error"]["message"], "forbidden_or_unknown");
    assert_eq!(w.opener.writes(), 1);
    let loaded = c.call("load_toolset", json!({"id":"plugin:lxd"})).await;
    assert_eq!(text(&loaded)["loaded"].as_array().unwrap().len(), 60);
}

#[tokio::test]
async fn shell_needs_ephemeral_grant_and_single_use_ticket() {
    let w = world();
    let now = now_unix();
    GrantStore::new(w.state.join("grants"))
        .issue(&EphemeralGrant {
            grant_id: "g1".into(),
            principal_id: "p-1".into(),
            tool: "cognitive.agent_bash_pro_exec".into(),
            args_constraint: None,
            not_before: now - 10,
            expires_at: now + 600,
            max_calls: 5,
            issued_by: "test".into(),
            approval_ticket_ref: None,
        })
        .unwrap();
    let args = json!({"command":"uptime"});
    TicketStore::new(w.state.join("tickets"))
        .issue(&Ticket {
            ticket_id: "t1".into(),
            principal_id: "p-1".into(),
            tool: "cognitive.agent_bash_pro_exec".into(),
            args_sha256: op_mcp_door::canonical::args_sha256(&args),
            issued_at: now,
            expires_at: now + 120,
            issued_by: "test".into(),
        })
        .unwrap();
    let mut c = Client::open(&w, "/mcp/loader", Some(assertion(3))).await;
    let call = |ticket: Option<&str>| json!({"name":"cognitive__agent_bash_pro_exec","arguments":args,"confirm_side_effects":true,"elevation_ticket":ticket});
    let no_ticket = c.call("call_tool", call(None)).await;
    assert_eq!(text(&no_ticket), "elevation_required");
    let ok = c.call("call_tool", call(Some("t1"))).await;
    assert_eq!(ok["result"]["isError"], false, "{ok}");
    let replay = c.call("call_tool", call(Some("t1"))).await;
    assert_eq!(text(&replay), "elevation_required");
    assert_eq!(w.opener.writes(), 1);
    let lines = w.door.audit.lines();
    let shell_lines: Vec<&Value> = lines
        .iter()
        .filter(|l| l["tool"] == "cognitive.agent_bash_pro_exec")
        .collect();
    assert_eq!(shell_lines.len(), 3);
    assert_eq!(shell_lines[1]["decision"], "allow");
    assert_eq!(shell_lines[1]["elevation_ticket_ref"], "t1");
    assert_eq!(shell_lines[1]["basis"], "grant:g1");
    assert!(
        shell_lines
            .iter()
            .all(|l| l["args"].is_null() && l["args_sha256"].is_string()),
        "shell args never logged"
    );
}

#[tokio::test]
async fn toolset_selection_respects_the_cap() {
    let w = world();
    let auth = assertion(4);
    let headers = [("x-oracle-identity-assertion-bin", auth.as_str())];
    let init = json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18"}});
    let refused = send(&w, "POST", "/mcp/full/plugin:lxd", &headers, init.clone()).await;
    let message = refused.body["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("65") && message.contains("50"),
        "{message}"
    );
    assert!(refused.session.is_none());
    let unknown = send(
        &w,
        "POST",
        "/mcp/full?toolsets=nope",
        &headers,
        init.clone(),
    )
    .await;
    assert!(
        unknown.body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("forbidden_or_unknown")
    );
    let header_sel = send(
        &w,
        "POST",
        "/mcp/full",
        &[headers[0], ("x-mcp-toolsets", "memory")],
        init,
    )
    .await;
    assert!(header_sel.session.is_some());

    let mut c = Client::open(&w, "/mcp/full", Some(assertion(5))).await;
    let before = c.names().await;
    let load = c.call("load_toolset", json!({"id":"plugin:lxd"})).await;
    assert_eq!(load["result"]["isError"], true);
    assert!(text(&load).as_str().unwrap().contains("cap is 50"));
    assert_eq!(
        c.names().await,
        before,
        "refused load leaves the session unchanged"
    );
    let load = c.call("load_toolset", json!({"id":"coding"})).await;
    assert_eq!(
        load["error"]["message"], "forbidden_or_unknown",
        "empty or unknown set"
    );
}

#[tokio::test]
async fn transport_rules() {
    let w = world();
    let ping = json!({"jsonrpc":"2.0","id":1,"method":"ping"});
    assert_eq!(
        send(&w, "POST", "/mcp/full", &[], ping.clone())
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(
            &w,
            "POST",
            "/mcp/full",
            &[("mcp-session-id", "nope")],
            ping.clone()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(&w, "GET", "/mcp/full", &[], Value::Null).await.status,
        StatusCode::METHOD_NOT_ALLOWED
    );
    assert_eq!(
        send(
            &w,
            "POST",
            "/mcp/full",
            &[("origin", "https://evil.example")],
            ping.clone()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&w, "POST", "/mcp/full", &[], json!([ping]))
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    let c = Client::open(&w, "/mcp/full", None).await;
    let sid = c.session.clone();
    let note = send(
        &w,
        "POST",
        "/mcp/full",
        &[
            ("mcp-session-id", &sid),
            ("origin", "https://allowed.example"),
        ],
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
    .await;
    assert_eq!(note.status, StatusCode::ACCEPTED);
    assert_eq!(
        send(
            &w,
            "POST",
            "/mcp/loader",
            &[("mcp-session-id", &sid)],
            ping.clone()
        )
        .await
        .status,
        StatusCode::NOT_FOUND,
        "sessions are per endpoint"
    );
    assert_eq!(
        send(
            &w,
            "DELETE",
            "/mcp/full",
            &[("mcp-session-id", &sid)],
            Value::Null
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            &w,
            "POST",
            "/mcp/full",
            &[("mcp-session-id", &sid)],
            ping.clone()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // A session opened as p-1 cannot continue anonymously (Requirement 2.8)…
    let auth = assertion(6);
    let authed = Client::open(&w, "/mcp/full", Some(auth.clone())).await;
    assert_eq!(
        send(
            &w,
            "POST",
            "/mcp/full",
            &[("mcp-session-id", &authed.session)],
            ping.clone()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    // …and its assertion nonce cannot open a second session (Requirement 12.3).
    let reuse = send(
        &w,
        "POST",
        "/mcp/full",
        &[("x-oracle-identity-assertion-bin", &auth)],
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}),
    )
    .await;
    assert_eq!(reuse.status, StatusCode::FORBIDDEN);
}

/// A read-only fake Qdrant REST server: every collection holds `points`
/// (`None` = 404). Returns its URL and a log of request lines.
fn fake_qdrant(points: Option<u64>) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = log.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 2048];
            let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            seen.lock()
                .unwrap()
                .push(request.lines().next().unwrap_or("").to_owned());
            let reply = match points {
                Some(p) => {
                    let body = format!(r#"{{"result":{{"points_count":{p}}},"status":"ok"}}"#);
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    )
                }
                None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_owned(),
            };
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    (url, log)
}

#[tokio::test]
async fn hot_vector_tools_answer_pending_until_vectors_exist() {
    // Collections absent (404): coding and knowledge are listed but pending.
    let (url, log) = fake_qdrant(None);
    let w = world_with(&[("OP_MCP_QDRANT_REST_URL", url)], true);
    let mut c = Client::open(&w, "/mcp/full", Some(assertion(11))).await;
    let names = c.names().await;
    assert!(names.contains(&"code_search".to_owned()), "{names:?}");
    assert!(names.contains(&"search_own_events".to_owned()), "{names:?}");
    for tool in ["code_search", "search_own_events"] {
        let pending = c.call(tool, json!({"query": "x"})).await;
        assert_eq!(pending["result"]["isError"], true, "{pending}");
        assert_eq!(text(&pending)["status"], "pending_revectorization");
    }
    assert_eq!(w.opener.writes(), 0, "nothing reaches FUSE while pending");
    // Ungated hot/memory tools are unaffected.
    let ok = c.call("memory_recall", json!({})).await;
    assert_eq!(ok["result"]["isError"], false, "{ok}");
    assert_eq!(w.opener.writes(), 1);
    let listed = c.call("list_toolsets", json!({})).await;
    let readiness = &text(&listed)["vector_readiness"];
    assert_eq!(readiness[0]["group"], "coding");
    assert_eq!(readiness[0]["status"], "pending_revectorization");
    assert_eq!(readiness[1]["group"], "knowledge");
    assert!(
        log.lock()
            .unwrap()
            .iter()
            .all(|l| l.starts_with("GET /collections/")),
        "the probe only reads"
    );
    let lines = w.door.audit.lines();
    assert!(
        lines
            .iter()
            .any(|l| l["reason"] == "pending_revectorization" && l["outcome"] == "not_sent"),
        "{lines:?}"
    );
}

#[tokio::test]
async fn hot_vector_tools_call_through_once_vectors_are_populated() {
    let (url, _log) = fake_qdrant(Some(1200));
    let w = world_with(&[("OP_MCP_QDRANT_REST_URL", url)], true);
    let mut c = Client::open(&w, "/mcp/full", Some(assertion(12))).await;
    let ok = c.call("code_search", json!({"query": "x"})).await;
    assert_eq!(ok["result"]["isError"], false, "{ok}");
    let ok = c.call("search_own_events", json!({"query": "x"})).await;
    assert_eq!(ok["result"]["isError"], false, "{ok}");
    assert_eq!(w.opener.writes(), 2);
}
