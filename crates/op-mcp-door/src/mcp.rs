//! MCP Streamable HTTP endpoints and meta-tools (Requirements 2, 6, 7.4, 7.5, 8).
//!
//! `/mcp/full[/<a>+<b>]` lists the authorized core (tier 1 + tier 2), any
//! selected or loaded toolsets, and `list_toolsets` / `load_toolset`.
//! `/mcp/loader` lists exactly five meta-tools. Every path goes through
//! [`Policy::authorize`]. Responses are `application/json`; no SSE stream is
//! offered (GET answers 405).

use crate::audit::{AuditLog, AuditRecord, now_rfc3339};
use crate::catalog::{Catalog, Tool};
use crate::classify::Class;
use crate::fusecall::{CallError, CallOpener, Mount, MountSpec, invoke, validate_arguments};
use crate::grants::now_unix;
use crate::identity::{
    ASSERTION_HEADER, Identity, MountMap, NonceBindings, Verifier, extract_assertion,
};
use crate::policy::{Caller, Decision, Policy, Purpose};
use crate::toolsets::{CAP, FULL_META, LOADER_META, Toolsets, ensure_within_cap, parse_selection};
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path as AxPath, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub const SESSION_HEADER: &str = "mcp-session-id";
pub const TOOLSETS_HEADER: &str = "x-mcp-toolsets";
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
pub const FORBIDDEN_OR_UNKNOWN: &str = "forbidden_or_unknown";
const SESSION_IDLE_SECS: i64 = 3600;
const MAX_SESSIONS: usize = 4096;

/// Peer address, inserted per connection by the TLS server.
#[derive(Debug, Clone, Copy)]
pub struct ClientAddr(pub IpAddr);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    Full,
    Loader,
}

impl Endpoint {
    fn as_str(self) -> &'static str {
        match self {
            Endpoint::Full => "full",
            Endpoint::Loader => "loader",
        }
    }
}

#[derive(Debug, Clone)]
struct Session {
    endpoint: Endpoint,
    principal: Option<String>,
    /// Statically selected plus runtime-loaded toolset ids.
    sets: Vec<String>,
    last_seen: i64,
}

/// Per-address fixed-window limiter for public calls (Requirement 7.5).
pub struct RateLimiter {
    per_minute: u32,
    windows: Mutex<HashMap<IpAddr, (i64, u32)>>,
}

impl RateLimiter {
    pub fn new(per_minute: u32) -> Self {
        Self {
            per_minute,
            windows: Mutex::new(HashMap::new()),
        }
    }

    pub fn allow(&self, ip: IpAddr, now: i64) -> bool {
        let mut windows = self.windows.lock().unwrap_or_else(|e| e.into_inner());
        let minute = now / 60;
        windows.retain(|_, (window, _)| *window >= minute - 1);
        let entry = windows.entry(ip).or_insert((minute, 0));
        if entry.0 != minute {
            *entry = (minute, 0);
        }
        entry.1 += 1;
        entry.1 <= self.per_minute
    }
}

/// Everything a request needs.
pub struct Door {
    pub catalog: Catalog,
    pub toolsets: Toolsets,
    pub policy: Policy,
    pub verifier: Verifier,
    pub mounts: MountMap,
    pub opener: Arc<dyn CallOpener>,
    pub audit: AuditLog,
    pub rate: RateLimiter,
    pub allowed_origins: Vec<String>,
    pub max_body: usize,
    nonces: NonceBindings,
    sessions: Mutex<HashMap<String, Session>>,
}

impl Door {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        catalog: Catalog,
        toolsets: Toolsets,
        policy: Policy,
        verifier: Verifier,
        mounts: MountMap,
        opener: Arc<dyn CallOpener>,
        audit: AuditLog,
        rate: RateLimiter,
        allowed_origins: Vec<String>,
        max_body: usize,
    ) -> Self {
        Self {
            catalog,
            toolsets,
            policy,
            verifier,
            mounts,
            opener,
            audit,
            rate,
            allowed_origins,
            max_body,
            nonces: NonceBindings::default(),
            sessions: Mutex::new(HashMap::new()),
        }
    }

    fn caller(&self, identity: Option<&Identity>) -> Caller {
        match identity {
            Some(id) => Caller {
                principal: Some(id.principal_id.clone()),
                has_mount: self.mounts.for_principal(&id.principal_id).is_some(),
            },
            None => Caller::anonymous(),
        }
    }

    fn visible(&self, caller: &Caller, tool: &Tool) -> bool {
        self.policy
            .authorize(caller, tool, Purpose::Discover, now_unix())
            .allowed()
    }

    /// Authorized tool keys of a toolset.
    fn allowed_members(&self, caller: &Caller, set_id: &str) -> Option<Vec<&Tool>> {
        let set = self.toolsets.get(set_id)?;
        Some(
            set.tools
                .iter()
                .filter_map(|k| self.catalog.by_key(k))
                .filter(|t| self.visible(caller, t))
                .collect(),
        )
    }

    /// The authorized generated tools of a `/mcp/full` session, in priority
    /// order (tier 1, tier 2, then toolset order), deduplicated.
    fn full_tools(&self, caller: &Caller, sets: &[String]) -> Vec<&Tool> {
        let mut seen = BTreeSet::new();
        let mut keys: Vec<String> = self.toolsets.core_keys();
        for id in sets {
            if let Some(set) = self.toolsets.get(id) {
                keys.extend(set.tools.iter().cloned());
            }
        }
        keys.into_iter()
            .filter(|k| seen.insert(k.clone()))
            .filter_map(|k| self.catalog.by_key(&k))
            .filter(|t| self.visible(caller, t))
            .collect()
    }

    fn check_sets(&self, caller: &Caller, sets: &[String]) -> Result<usize, String> {
        for id in sets {
            if self
                .allowed_members(caller, id)
                .is_none_or(|m| m.is_empty())
            {
                return Err(format!("{FORBIDDEN_OR_UNKNOWN}: toolset {id}"));
            }
        }
        let count = self.full_tools(caller, sets).len() + FULL_META.len();
        ensure_within_cap(count)?;
        Ok(count)
    }

    fn resolve(&self, name: &str) -> Option<&Tool> {
        self.catalog
            .by_name(name)
            .or_else(|| self.catalog.by_key(name))
    }
}

// ------------------------------------------------------------- JSON-RPC

fn rpc_result(id: &Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

fn rpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

fn tool_text(value: &Value, is_error: bool) -> Value {
    let text = match value {
        Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    };
    json!({"content":[{"type":"text","text":text}],"isError":is_error})
}

fn json_response(status: StatusCode, body: Value, session: Option<&str>) -> Response {
    let mut response = (status, axum::Json(body)).into_response();
    if let Some(id) = session
        && let Ok(value) = HeaderValue::from_str(id)
    {
        response.headers_mut().insert(SESSION_HEADER, value);
    }
    response
}

fn status(code: StatusCode) -> Response {
    code.into_response()
}

fn meta_descriptor(name: &str) -> Value {
    let (description, schema) = match name {
        "list_toolsets" => (
            "List the toolsets you may load, with the number of tools you are allowed in each.",
            json!({"type":"object","properties":{}}),
        ),
        "search_tools" => (
            "Search every tool you are allowed to call by name and description.",
            json!({"type":"object","properties":{
                "query":{"type":"string"},
                "toolset":{"type":"string"},
                "limit":{"type":"integer","minimum":1,"maximum":50,"default":20}},
                "required":["query"]}),
        ),
        "describe_tool" => (
            "Describe one tool: description, input and output schema, side effect, class and capability.",
            json!({"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}),
        ),
        "call_tool" => (
            "Call any tool you are allowed to call. Mutations need confirm_side_effects=true; shell, exec, admin and secret tools need elevation_ticket.",
            json!({"type":"object","properties":{
                "name":{"type":"string"},
                "arguments":{"type":"object"},
                "confirm_side_effects":{"type":"boolean","default":false},
                "elevation_ticket":{"type":"string"}},
                "required":["name"]}),
        ),
        _ => (
            "Load a toolset into this session (re-list tools afterwards). The tool cap is 50.",
            json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}),
        ),
    };
    json!({"name":name,"description":description,"inputSchema":schema,
           "annotations":{"readOnlyHint": name != "call_tool","openWorldHint":false}})
}

struct Ctx<'a> {
    door: &'a Door,
    endpoint: Endpoint,
    session_id: String,
    caller: Caller,
    client: IpAddr,
}

impl Ctx<'_> {
    fn record(&self, via: &str, tool: &str) -> AuditRecord {
        AuditRecord {
            ts: now_rfc3339(),
            door_session: Some(self.session_id.clone()),
            principal_id: self.caller.principal.clone(),
            identity_mode: None,
            client: Some(self.client.to_string()),
            endpoint: self.endpoint.as_str().into(),
            via: via.into(),
            tool: tool.into(),
            ..Default::default()
        }
    }

    fn session_sets(&self) -> Vec<String> {
        let sessions = self.door.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions
            .get(&self.session_id)
            .map(|s| s.sets.clone())
            .unwrap_or_default()
    }

    fn tools_list(&self) -> Value {
        let tools: Vec<Value> = match self.endpoint {
            Endpoint::Loader => LOADER_META.iter().map(|n| meta_descriptor(n)).collect(),
            Endpoint::Full => {
                let mut list: Vec<Value> = self
                    .door
                    .full_tools(&self.caller, &self.session_sets())
                    .into_iter()
                    .take(CAP - FULL_META.len())
                    .map(Tool::descriptor)
                    .collect();
                list.extend(FULL_META.iter().map(|n| meta_descriptor(n)));
                list
            }
        };
        json!({"tools": tools})
    }

    /// `Ok(result)` is a CallToolResult; `Err` is a JSON-RPC error message.
    async fn tools_call(&self, params: &Value) -> Result<Value, String> {
        let name = params["name"].as_str().ok_or("tools/call needs a name")?;
        let mut args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let is_meta = match self.endpoint {
            Endpoint::Loader => LOADER_META.contains(&name),
            Endpoint::Full => FULL_META.contains(&name),
        };
        if is_meta {
            return self.meta(name, &args).await;
        }
        if self.endpoint == Endpoint::Loader {
            self.door.audit.record(&AuditRecord {
                decision: "deny".into(),
                reason: Some("not_listed".into()),
                ..self.record("direct", name)
            });
            return Err(FORBIDDEN_OR_UNKNOWN.into());
        }
        // Direct calls on /mcp/full reach only tools this session lists.
        let listed = self
            .door
            .full_tools(&self.caller, &self.session_sets())
            .iter()
            .any(|t| t.name == name);
        if !listed {
            self.door.audit.record(&AuditRecord {
                decision: "deny".into(),
                reason: Some("not_listed".into()),
                ..self.record("direct", name)
            });
            return Err(FORBIDDEN_OR_UNKNOWN.into());
        }
        let confirm =
            take_confirm(&mut args) || params["_meta"]["opdbus/confirm_side_effects"] == true;
        let ticket = params["_meta"]["opdbus/elevation_ticket"]
            .as_str()
            .map(str::to_owned);
        self.call(name, args, confirm, ticket.as_deref(), "direct")
            .await
    }

    async fn meta(&self, name: &str, args: &Value) -> Result<Value, String> {
        let mut record = self.record("meta", name);
        record.class = Some(Class::Public);
        record.basis = Some("public".into());
        record.decision = "allow".into();
        let result = match name {
            "list_toolsets" => Ok(self.list_toolsets()),
            "search_tools" => Ok(self.search_tools(args)),
            "describe_tool" => self.describe_tool(args),
            "load_toolset" => self.load_toolset(args),
            "call_tool" => {
                let target = args["name"].as_str().ok_or("call_tool needs a name")?;
                let inner = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
                let confirm = args["confirm_side_effects"] == true;
                let ticket = args["elevation_ticket"].as_str();
                return self.call(target, inner, confirm, ticket, "call_tool").await;
            }
            _ => Err(FORBIDDEN_OR_UNKNOWN.into()),
        };
        record.outcome = Some(if result.is_ok() { "ok" } else { "error" }.into());
        self.door.audit.record(&record);
        result
    }

    fn list_toolsets(&self) -> Value {
        let loaded = self.session_sets();
        let sets: Vec<Value> = self
            .door
            .toolsets
            .file
            .toolsets
            .iter()
            .filter_map(|set| {
                let allowed = self.door.allowed_members(&self.caller, &set.id)?.len();
                (allowed > 0).then(|| {
                    json!({"id":set.id,"title":set.title,"tool_count":allowed,
                           "temperature":set.temperature,"loaded":loaded.contains(&set.id)})
                })
            })
            .collect();
        let core = self.door.full_tools(&self.caller, &[]).len() + FULL_META.len();
        tool_text(
            &json!({"cap":CAP,"core_tool_count":core,"missing_hot_tools":self.door.toolsets.file.missing,"toolsets":sets}),
            false,
        )
    }

    fn search_tools(&self, args: &Value) -> Value {
        let query = args["query"].as_str().unwrap_or("").to_lowercase();
        let terms: Vec<&str> = query.split_whitespace().collect();
        let limit = args["limit"].as_u64().unwrap_or(20).clamp(1, 50) as usize;
        let only: Option<BTreeSet<String>> = args["toolset"].as_str().map(|id| {
            self.door
                .toolsets
                .get(id)
                .map(|s| s.tools.iter().cloned().collect())
                .unwrap_or_default()
        });
        let mut hits: Vec<(usize, &Tool)> = self
            .door
            .catalog
            .tools
            .iter()
            .filter(|t| only.as_ref().is_none_or(|keys| keys.contains(&t.key())))
            .filter_map(|t| {
                let hay = format!("{} {} {}", t.name, t.key(), t.description).to_lowercase();
                let score = terms.iter().filter(|term| hay.contains(*term)).count();
                (terms.is_empty() || score > 0).then_some((score, t))
            })
            .filter(|(_, t)| self.door.visible(&self.caller, t))
            .collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        let tools: Vec<Value> = hits
            .into_iter()
            .take(limit)
            .map(|(_, t)| {
                json!({"name":t.name,"key":t.key(),"summary":t.description.chars().take(200).collect::<String>(),
                       "side_effect":if t.is_mutation() {"mutation"} else {"read"},"class":t.class})
            })
            .collect();
        tool_text(&json!({"tools":tools}), false)
    }

    fn describe_tool(&self, args: &Value) -> Result<Value, String> {
        let name = args["name"].as_str().unwrap_or("");
        let tool = self
            .door
            .resolve(name)
            .filter(|t| self.door.visible(&self.caller, t))
            .ok_or(FORBIDDEN_OR_UNKNOWN)?;
        Ok(tool_text(
            &json!({"name":tool.name,"key":tool.key(),"description":tool.description,
                    "input_schema":tool.mcp_input_schema(),"output_schema":tool.output_schema,
                    "side_effect":if tool.is_mutation() {"mutation"} else {"read"},
                    "class":tool.class,"capability":tool.required_capability,
                    "needs_confirmation":tool.is_mutation(),"needs_elevation_ticket":tool.class.needs_ticket()}),
            false,
        ))
    }

    fn load_toolset(&self, args: &Value) -> Result<Value, String> {
        let id = args["id"].as_str().unwrap_or("");
        let members = self
            .door
            .allowed_members(&self.caller, id)
            .filter(|m| !m.is_empty())
            .ok_or(FORBIDDEN_OR_UNKNOWN)?;
        let names: Vec<String> = members.iter().map(|t| t.name.clone()).collect();
        if self.endpoint == Endpoint::Loader {
            return Ok(tool_text(
                &json!({"loaded":names,"note":"use call_tool with these names"}),
                false,
            ));
        }
        let mut sets = self.session_sets();
        if !sets.iter().any(|s| s == id) {
            sets.push(id.to_owned());
        }
        match self.door.check_sets(&self.caller, &sets) {
            Ok(total) => {
                let mut sessions = self.door.sessions.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(session) = sessions.get_mut(&self.session_id) {
                    session.sets = sets;
                }
                Ok(tool_text(
                    &json!({"loaded":names,"tool_count":total,"relist":true}),
                    false,
                ))
            }
            // Refused: the session is unchanged (Requirement 6.4).
            Err(message) => Ok(tool_text(&json!(message), true)),
        }
    }

    /// The one call path for direct calls and `call_tool` (Requirement 6.6).
    async fn call(
        &self,
        name: &str,
        args: Value,
        confirm: bool,
        ticket: Option<&str>,
        via: &str,
    ) -> Result<Value, String> {
        let started = Instant::now();
        let mut record = self.record(via, name);
        record.confirm = confirm;
        let deny = |mut record: AuditRecord, reason: &str| {
            record.decision = "deny".into();
            record.reason = Some(reason.into());
            record.outcome = Some("not_sent".into());
            self.door.audit.record(&record);
        };
        let Some(tool) = self.door.resolve(name) else {
            deny(record, "unknown");
            return Err(FORBIDDEN_OR_UNKNOWN.into());
        };
        record.tool = tool.key();
        record.class = Some(tool.class);
        record.required_capability = tool.required_capability.clone();
        let mut args = args;
        if via == "call_tool" {
            // `confirm_side_effects` inside arguments is honoured on both paths.
            let inner = take_confirm(&mut args);
            record.confirm = confirm || inner;
        }
        let confirm = record.confirm;
        record = record.with_args(tool.class, &args);
        if let Decision::Deny(reason) =
            self.door
                .policy
                .authorize(&self.caller, tool, Purpose::Discover, now_unix())
        {
            deny(record, reason.as_str());
            return Err(FORBIDDEN_OR_UNKNOWN.into());
        }
        if tool.class == Class::Public && !self.door.rate.allow(self.client, now_unix()) {
            deny(record, "rate_limited");
            return Ok(tool_text(&json!("rate_limited"), true));
        }
        if let Err(error) = validate_arguments(tool, &args) {
            deny(record, "invalid_arguments");
            return Ok(tool_text(&json!(error.to_string()), true));
        }
        let spec: Option<MountSpec> = match &self.caller.principal {
            Some(principal) => self.door.mounts.for_principal(principal).cloned(),
            None => None,
        }
        .or_else(|| {
            (tool.class == Class::Public)
                .then(|| self.door.mounts.default.clone())
                .flatten()
        });
        let Some(spec) = spec else {
            deny(record, "no_mount");
            return Err(FORBIDDEN_OR_UNKNOWN.into());
        };
        record.identity_mode = Some(spec.mode.clone());
        let mount = match Mount::open(&spec) {
            Ok(mount) => mount,
            Err(error) => {
                deny(record, "mount_unavailable");
                return Ok(tool_text(&json!(error.to_string()), true));
            }
        };
        let decision = self.door.policy.authorize(
            &self.caller,
            tool,
            Purpose::Call {
                args: &args,
                confirm,
                ticket,
            },
            now_unix(),
        );
        let allowed = match decision {
            Decision::Allow(allowed) => allowed,
            Decision::Deny(reason) => {
                deny(record, reason.as_str());
                return Ok(tool_text(&json!(reason.as_str()), true));
            }
        };
        record.decision = "allow".into();
        record.basis = Some(allowed.basis.label());
        record.elevation_ticket_ref = allowed.ticket.clone();
        let opener = self.door.opener.clone();
        let owned = tool.clone();
        let call_args = args.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            invoke(&mount, opener.as_ref(), &owned, &call_args)
        })
        .await
        .unwrap_or_else(|_| Err(CallError::UnknownOutcome("call worker failed".into())));
        record.latency_ms = Some(started.elapsed().as_millis() as u64);
        let result = match outcome {
            Ok(receipt) => {
                record.outcome = Some("ok".into());
                record.event_id = receipt["result"].get("event_id").cloned();
                tool_text(&receipt["result"], false)
            }
            Err(error) => {
                record.outcome = Some(error.outcome().into());
                if tool.class == Class::Secret {
                    tool_text(&json!(error.outcome()), true)
                } else {
                    tool_text(&json!(error.to_string()), true)
                }
            }
        };
        self.door.audit.record(&record);
        Ok(result)
    }
}

fn take_confirm(args: &mut Value) -> bool {
    args.as_object_mut()
        .and_then(|o| o.remove("confirm_side_effects"))
        .is_some_and(|v| v == true)
}

// ---------------------------------------------------------------- HTTP

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

async fn handle(
    door: Arc<Door>,
    endpoint: Endpoint,
    path_sets: Option<String>,
    query: HashMap<String, String>,
    headers: HeaderMap,
    client: IpAddr,
    body: Bytes,
) -> Response {
    if let Some(origin) = header(&headers, "origin")
        && !door.allowed_origins.iter().any(|o| o == origin)
    {
        return status(StatusCode::FORBIDDEN);
    }
    if body.len() > door.max_body {
        return status(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let Ok(message) = serde_json::from_slice::<Value>(&body) else {
        return json_response(
            StatusCode::BAD_REQUEST,
            rpc_error(&Value::Null, -32700, "parse error"),
            None,
        );
    };
    if message.is_array() {
        return json_response(
            StatusCode::BAD_REQUEST,
            rpc_error(&Value::Null, -32600, "batch requests are not supported"),
            None,
        );
    }
    let id = message.get("id").cloned();
    let method = message["method"].as_str().unwrap_or("").to_owned();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let now = now_unix();
    // Any credential problem means anonymous (Requirement 12.2).
    let identity = extract_assertion(
        header(&headers, ASSERTION_HEADER),
        header(&headers, "authorization"),
    )
    .ok()
    .flatten()
    .and_then(|wire| door.verifier.verify(&wire, now).ok());
    let caller = door.caller(identity.as_ref());

    if method == "initialize" {
        let Some(id) = id else {
            return status(StatusCode::BAD_REQUEST);
        };
        let sets = match endpoint {
            Endpoint::Full => parse_selection(
                path_sets.as_deref(),
                query.get("toolsets").map(String::as_str),
                header(&headers, TOOLSETS_HEADER),
            ),
            Endpoint::Loader => Vec::new(),
        };
        if endpoint == Endpoint::Full
            && let Err(message) = door.check_sets(&caller, &sets)
        {
            return json_response(StatusCode::OK, rpc_error(&id, -32602, &message), None);
        }
        let session_id = uuid::Uuid::new_v4().simple().to_string();
        if let Some(identity) = &identity
            && !door
                .nonces
                .bind(identity.nonce, &session_id, identity.expires_at, now)
        {
            return status(StatusCode::FORBIDDEN);
        }
        {
            let mut sessions = door.sessions.lock().unwrap_or_else(|e| e.into_inner());
            sessions.retain(|_, s| now - s.last_seen < SESSION_IDLE_SECS);
            if sessions.len() >= MAX_SESSIONS {
                return status(StatusCode::SERVICE_UNAVAILABLE);
            }
            sessions.insert(
                session_id.clone(),
                Session {
                    endpoint,
                    principal: caller.principal.clone(),
                    sets,
                    last_seen: now,
                },
            );
        }
        let requested = params["protocolVersion"].as_str().unwrap_or("");
        let version = if PROTOCOL_VERSIONS.contains(&requested) {
            requested
        } else {
            PROTOCOL_VERSIONS[0]
        };
        let result = json!({
            "protocolVersion": version,
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {"name": "op-mcp-door", "version": env!("CARGO_PKG_VERSION")},
            "instructions": match endpoint {
                Endpoint::Full => "Curated tools. list_toolsets / load_toolset add more (cap 50); re-list after loading.",
                Endpoint::Loader => "Use search_tools, describe_tool and call_tool to reach every tool you are allowed.",
            },
        });
        return json_response(StatusCode::OK, rpc_result(&id, result), Some(&session_id));
    }

    let Some(session_id) = header(&headers, SESSION_HEADER).map(str::to_owned) else {
        return status(StatusCode::BAD_REQUEST);
    };
    if let Some(version) = header(&headers, "mcp-protocol-version")
        && !PROTOCOL_VERSIONS.contains(&version)
    {
        return status(StatusCode::BAD_REQUEST);
    }
    {
        let mut sessions = door.sessions.lock().unwrap_or_else(|e| e.into_inner());
        let Some(session) = sessions.get_mut(&session_id) else {
            return status(StatusCode::NOT_FOUND);
        };
        if session.endpoint != endpoint {
            return status(StatusCode::NOT_FOUND);
        }
        // The credential must still resolve to the session's principal (Req 2.8).
        if session.principal != caller.principal {
            return status(StatusCode::FORBIDDEN);
        }
        session.last_seen = now;
    }
    if let Some(identity) = &identity
        && !door
            .nonces
            .bind(identity.nonce, &session_id, identity.expires_at, now)
    {
        return status(StatusCode::FORBIDDEN);
    }
    let Some(id) = id else {
        // Notifications and responses: accepted, nothing to say.
        return status(StatusCode::ACCEPTED);
    };
    let ctx = Ctx {
        door: &door,
        endpoint,
        session_id: session_id.clone(),
        caller,
        client,
    };
    let reply = match method.as_str() {
        "ping" => rpc_result(&id, json!({})),
        "tools/list" => rpc_result(&id, ctx.tools_list()),
        "tools/call" => match ctx.tools_call(&params).await {
            Ok(result) => rpc_result(&id, result),
            Err(message) => rpc_error(&id, -32602, &message),
        },
        _ => rpc_error(&id, -32601, "method not found"),
    };
    json_response(StatusCode::OK, reply, Some(&session_id))
}

async fn delete(door: Arc<Door>, endpoint: Endpoint, headers: HeaderMap) -> Response {
    let Some(session_id) = header(&headers, SESSION_HEADER) else {
        return status(StatusCode::BAD_REQUEST);
    };
    let mut sessions = door.sessions.lock().unwrap_or_else(|e| e.into_inner());
    match sessions.get(session_id) {
        Some(session) if session.endpoint == endpoint => {
            sessions.remove(session_id);
            status(StatusCode::NO_CONTENT)
        }
        _ => status(StatusCode::NOT_FOUND),
    }
}

fn client_of(ext: Option<axum::Extension<ClientAddr>>) -> IpAddr {
    ext.map(|e| e.0.0)
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
}

type Q = Query<HashMap<String, String>>;
type Ext = Option<axum::Extension<ClientAddr>>;

/// The axum router for both endpoints.
pub fn router(door: Arc<Door>) -> Router {
    let max_body = door.max_body;
    Router::new()
        .route(
            "/mcp/full",
            post(|State(d): State<Arc<Door>>, Query(q): Q, h: HeaderMap, e: Ext, b: Bytes| async move {
                handle(d, Endpoint::Full, None, q, h, client_of(e), b).await
            })
            .get(|| async { status(StatusCode::METHOD_NOT_ALLOWED) })
            .delete(|State(d): State<Arc<Door>>, h: HeaderMap| async move { delete(d, Endpoint::Full, h).await }),
        )
        .route(
            "/mcp/full/{sets}",
            post(
                |State(d): State<Arc<Door>>, AxPath(sets): AxPath<String>, Query(q): Q, h: HeaderMap, e: Ext, b: Bytes| async move {
                    handle(d, Endpoint::Full, Some(sets), q, h, client_of(e), b).await
                },
            )
            .get(|| async { status(StatusCode::METHOD_NOT_ALLOWED) })
            .delete(|State(d): State<Arc<Door>>, h: HeaderMap| async move { delete(d, Endpoint::Full, h).await }),
        )
        .route(
            "/mcp/loader",
            post(|State(d): State<Arc<Door>>, Query(q): Q, h: HeaderMap, e: Ext, b: Bytes| async move {
                handle(d, Endpoint::Loader, None, q, h, client_of(e), b).await
            })
            .get(|| async { status(StatusCode::METHOD_NOT_ALLOWED) })
            .delete(|State(d): State<Arc<Door>>, h: HeaderMap| async move { delete(d, Endpoint::Loader, h).await }),
        )
        .layer(DefaultBodyLimit::max(max_body))
        .with_state(door)
}
