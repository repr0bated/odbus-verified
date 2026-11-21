//! Vector readiness for the hot coding and knowledge tools.
//!
//! Owner decision 2026-11-19: coding (`code_search`, `code_context`,
//! `code_index`) and knowledge (`search_own_events`) are always loaded, but
//! their Qdrant collections were lost and must be re-vectorized unless the
//! Netcup snapshot is restored. Until every collection a group depends on
//! exists and holds points, a call to one of its tools is answered by the door
//! with a `pending_revectorization` status and nothing is written to FUSE.
//!
//! The probe only reads (`GET /collections/<name>` on the Qdrant REST API).
//! It never creates a collection, upserts a point or starts re-vectorization.

use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Qdrant REST default (the gRPC port is 6334).
pub const DEFAULT_QDRANT_REST_URL: &str = "http://127.0.0.1:6333";
/// Status string returned for a gated tool whose vectors are not ready.
pub const PENDING: &str = "pending_revectorization";

/// Hot coding tools (`<plugin>.<method>`).
pub const CODING_TOOLS: &[&str] = &[
    "cognitive.code_search",
    "cognitive.code_context",
    "cognitive.code_index",
];
/// Hot knowledge tool.
pub const KNOWLEDGE_TOOLS: &[&str] = &["snowball.search_own_events"];

/// The fused voyage-4 retrieval set `code_search`/`code_context` read by
/// default (`op-cognitive/src/rag_pipeline.rs` `DEFAULT_VOYAGE4_COLLECTIONS`).
pub const CODING_COLLECTIONS: &[&str] = &[
    "repos_lsp_c_cpp_voyage_4_lite",
    "repos_lsp_go_voyage_4_large",
    "repos_lsp_java_voyage_4_lite",
    "repos_lsp_python_voyage_4_large",
    "repos_lsp_rust_voyage_4_large",
    "repos_lsp_typescript_voyage_4_lite",
    "repos_specs_docs_voyage_4_lite",
];
/// The chain-footprint collection behind `search_own_events`
/// (`op-cognitive/src/chain_vectors.rs` `DEFAULT_COLLECTION`).
pub const KNOWLEDGE_COLLECTIONS: &[&str] = &["snowball_footprints"];

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const IO_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_RESPONSE: u64 = 1024 * 1024;

/// One vector-backed group of hot tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Gate {
    pub group: String,
    pub tools: Vec<String>,
    pub collections: Vec<String>,
}

/// The default coding and knowledge gates, with optional collection overrides.
pub fn default_gates(coding: Option<Vec<String>>, knowledge: Option<Vec<String>>) -> Vec<Gate> {
    let owned = |names: &[&str]| names.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    vec![
        Gate {
            group: "coding".into(),
            tools: owned(CODING_TOOLS),
            collections: coding.unwrap_or_else(|| owned(CODING_COLLECTIONS)),
        },
        Gate {
            group: "knowledge".into(),
            tools: owned(KNOWLEDGE_TOOLS),
            collections: knowledge.unwrap_or_else(|| owned(KNOWLEDGE_COLLECTIONS)),
        },
    ]
}

/// A collection name is used in a URL path, so only `[A-Za-z0-9_.-]` passes.
pub fn valid_collection(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// Points in a collection: `Ok(None)` when the collection does not exist.
pub trait VectorProbe: Send + Sync {
    fn points_count(&self, collection: &str) -> Result<Option<u64>, String>;
}

/// The state of one gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GateStatus {
    pub group: String,
    /// `ready` or `pending_revectorization`.
    pub status: String,
    /// Point count per collection; `null` when absent or unreadable.
    pub collections: BTreeMap<String, Option<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl GateStatus {
    pub fn is_ready(&self) -> bool {
        self.status == "ready"
    }

    /// The tool result body for a call refused while pending.
    pub fn pending_body(&self, tool: &str) -> Value {
        json!({
            "status": PENDING,
            "tool": tool,
            "group": self.group,
            "collections": self.collections,
            "detail": self.detail,
            "message": format!(
                "{tool} is loaded but its {} vectors are not populated yet; \
                 it answers once re-vectorization (or a snapshot restore) fills them",
                self.group
            ),
        })
    }
}

/// Probe one gate (blocking).
pub fn probe_gate(probe: &dyn VectorProbe, gate: &Gate) -> GateStatus {
    let mut collections = BTreeMap::new();
    let mut problems: Vec<String> = Vec::new();
    for name in &gate.collections {
        if !valid_collection(name) {
            problems.push(format!("{name}: invalid collection name"));
            collections.insert(name.clone(), None);
            continue;
        }
        match probe.points_count(name) {
            Ok(Some(n)) => {
                if n == 0 {
                    problems.push(format!("{name}: empty"));
                }
                collections.insert(name.clone(), Some(n));
            }
            Ok(None) => {
                problems.push(format!("{name}: absent"));
                collections.insert(name.clone(), None);
            }
            Err(error) => {
                problems.push(format!("{name}: {error}"));
                collections.insert(name.clone(), None);
            }
        }
    }
    if gate.collections.is_empty() {
        problems.push("no collections configured".into());
    }
    GateStatus {
        group: gate.group.clone(),
        status: if problems.is_empty() {
            "ready"
        } else {
            PENDING
        }
        .into(),
        collections,
        detail: (!problems.is_empty()).then(|| problems.join("; ")),
    }
}

/// Gates with a short cache, so a burst of calls probes Qdrant once.
pub struct Readiness {
    probe: Arc<dyn VectorProbe>,
    gates: Vec<Gate>,
    ttl: Duration,
    cache: Mutex<HashMap<String, (Instant, GateStatus)>>,
}

impl Readiness {
    pub fn new(probe: Arc<dyn VectorProbe>, gates: Vec<Gate>, ttl: Duration) -> Self {
        Self {
            probe,
            gates,
            ttl,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn gates(&self) -> &[Gate] {
        &self.gates
    }

    /// The gate a `<plugin>.<method>` key belongs to.
    pub fn gate_for(&self, key: &str) -> Option<&Gate> {
        self.gates.iter().find(|g| g.tools.iter().any(|t| t == key))
    }

    /// Cached status of a gate (blocking on a cache miss).
    pub fn status(&self, gate: &Gate) -> GateStatus {
        if let Some((at, status)) = self
            .cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&gate.group)
            && at.elapsed() < self.ttl
        {
            return status.clone();
        }
        let status = probe_gate(self.probe.as_ref(), gate);
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(gate.group.clone(), (Instant::now(), status.clone()));
        status
    }

    /// `Some(status)` when `key` is gated and its vectors are not ready.
    pub fn pending(&self, key: &str) -> Option<GateStatus> {
        let gate = self.gate_for(key)?;
        let status = self.status(gate);
        (!status.is_ready()).then_some(status)
    }

    /// Every gate's status (blocking on cache misses).
    pub fn snapshot(&self) -> Vec<GateStatus> {
        self.gates.iter().map(|g| self.status(g)).collect()
    }
}

/// Read-only Qdrant REST probe over plain HTTP/1.1 (loopback deployment).
pub struct QdrantRest {
    host: String,
    port: u16,
    api_key_file: Option<PathBuf>,
}

impl QdrantRest {
    /// Accepts `http://host[:port][/]` only.
    pub fn new(url: &str, api_key_file: Option<PathBuf>) -> Result<Self, String> {
        let rest = url
            .strip_prefix("http://")
            .ok_or_else(|| format!("unsupported Qdrant URL scheme in {url:?} (http:// only)"))?;
        let authority = rest.trim_end_matches('/');
        if authority.is_empty() || authority.contains('/') || authority.contains('@') {
            return Err(format!(
                "Qdrant URL must be http://host[:port], got {url:?}"
            ));
        }
        let bad_port = || format!("bad Qdrant port in {url:?}");
        let (host, port) = if let Some(bracketed) = authority.strip_prefix('[') {
            let (h, tail) = bracketed
                .split_once(']')
                .ok_or_else(|| format!("bad Qdrant host in {url:?}"))?;
            let port = match tail.strip_prefix(':') {
                Some(p) => p.parse::<u16>().map_err(|_| bad_port())?,
                None if tail.is_empty() => 80,
                None => return Err(bad_port()),
            };
            (format!("[{h}]"), port)
        } else {
            match authority.split_once(':') {
                Some((h, p)) => (h.to_owned(), p.parse::<u16>().map_err(|_| bad_port())?),
                None => (authority.to_owned(), 80),
            }
        };
        if host.is_empty() {
            return Err(format!("Qdrant URL has no host: {url:?}"));
        }
        Ok(Self {
            host,
            port,
            api_key_file,
        })
    }

    fn get(&self, path: &str) -> Result<(u16, Vec<u8>), String> {
        let host = self.host.trim_start_matches('[').trim_end_matches(']');
        let addr = (host, self.port)
            .to_socket_addrs()
            .map_err(|e| format!("resolve: {e}"))?
            .next()
            .ok_or("resolve: no address")?;
        let mut stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
            .map_err(|e| format!("connect: {e}"))?;
        stream
            .set_read_timeout(Some(IO_TIMEOUT))
            .and_then(|_| stream.set_write_timeout(Some(IO_TIMEOUT)))
            .map_err(|e| format!("socket: {e}"))?;
        let mut request = format!(
            "GET {path} HTTP/1.1\r\nHost: {}:{}\r\nAccept: application/json\r\nConnection: close\r\n",
            self.host, self.port
        );
        if let Some(file) = &self.api_key_file {
            // The key is sent, never logged or returned.
            let key = std::fs::read_to_string(file).map_err(|_| "api key file unreadable")?;
            let key = key.trim();
            if key.is_empty() || key.contains(['\r', '\n']) {
                return Err("api key file is empty or malformed".into());
            }
            request.push_str("api-key: ");
            request.push_str(key);
            request.push_str("\r\n");
        }
        request.push_str("\r\n");
        stream
            .write_all(request.as_bytes())
            .map_err(|e| format!("write: {e}"))?;
        let mut raw = Vec::new();
        stream
            .take(MAX_RESPONSE)
            .read_to_end(&mut raw)
            .map_err(|e| format!("read: {e}"))?;
        parse_response(&raw)
    }
}

impl VectorProbe for QdrantRest {
    fn points_count(&self, collection: &str) -> Result<Option<u64>, String> {
        if !valid_collection(collection) {
            return Err("invalid collection name".into());
        }
        let (status, body) = self.get(&format!("/collections/{collection}"))?;
        match status {
            404 => Ok(None),
            200 => {
                let value: Value =
                    serde_json::from_slice(&body).map_err(|e| format!("body: {e}"))?;
                let result = &value["result"];
                result["points_count"]
                    .as_u64()
                    .or_else(|| result["vectors_count"].as_u64())
                    .map(Some)
                    .ok_or_else(|| "no points_count in collection info".into())
            }
            other => Err(format!("HTTP {other}")),
        }
    }
}

/// Split an HTTP/1.1 response into status and (de-chunked) body.
pub fn parse_response(raw: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("truncated response")?;
    let head = std::str::from_utf8(&raw[..split]).map_err(|_| "non-UTF-8 headers")?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or("bad status line")?;
    let chunked = lines.any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    let body = &raw[split + 4..];
    if !chunked {
        return Ok((status, body.to_vec()));
    }
    let mut out = Vec::new();
    let mut rest = body;
    loop {
        let line_end = rest
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("truncated chunk")?;
        let size_text = std::str::from_utf8(&rest[..line_end]).map_err(|_| "bad chunk size")?;
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| "bad chunk size")?;
        rest = &rest[line_end + 2..];
        if size == 0 {
            return Ok((status, out));
        }
        if rest.len() < size {
            return Err("truncated chunk".into());
        }
        out.extend_from_slice(&rest[..size]);
        rest = rest.get(size + 2..).ok_or("truncated chunk")?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fake {
        counts: HashMap<String, Result<Option<u64>, String>>,
        calls: AtomicUsize,
    }

    impl Fake {
        fn new(pairs: &[(&str, Result<Option<u64>, String>)]) -> Self {
            Self {
                counts: pairs
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), v.clone()))
                    .collect(),
                calls: AtomicUsize::new(0),
            }
        }
    }

    impl VectorProbe for Fake {
        fn points_count(&self, collection: &str) -> Result<Option<u64>, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.counts
                .get(collection)
                .cloned()
                .unwrap_or(Err("connect: refused".into()))
        }
    }

    fn gates() -> Vec<Gate> {
        default_gates(
            Some(vec!["code_a".into(), "code_b".into()]),
            Some(vec!["know".into()]),
        )
    }

    #[test]
    fn default_gates_cover_the_hot_coding_and_knowledge_tools() {
        let gates = default_gates(None, None);
        assert_eq!(gates[0].group, "coding");
        assert_eq!(gates[0].tools.len(), 3);
        assert_eq!(gates[0].collections.len(), 7);
        assert_eq!(gates[1].group, "knowledge");
        assert_eq!(gates[1].tools, vec!["snowball.search_own_events"]);
        assert_eq!(gates[1].collections, vec!["snowball_footprints"]);
        for key in CODING_TOOLS.iter().chain(KNOWLEDGE_TOOLS) {
            let (plugin, method) = key.split_once('.').unwrap();
            assert!(crate::catalog::TIER_HOT.contains(&method), "{key}");
            assert!(!plugin.is_empty());
        }
    }

    #[test]
    fn empty_absent_or_unreachable_collections_are_pending() {
        let fake = Arc::new(Fake::new(&[
            ("code_a", Ok(Some(10))),
            ("code_b", Ok(Some(0))),
        ]));
        let r = Readiness::new(fake, gates(), Duration::from_secs(60));
        let pending = r.pending("cognitive.code_search").unwrap();
        assert_eq!(pending.status, PENDING);
        assert_eq!(pending.collections["code_a"], Some(10));
        assert!(pending.detail.as_deref().unwrap().contains("code_b: empty"));
        let knowledge = r.pending("snowball.search_own_events").unwrap();
        assert!(knowledge.detail.unwrap().contains("know: connect: refused"));
        let body = pending.pending_body("cognitive.code_search");
        assert_eq!(body["status"], PENDING);
        assert_eq!(body["group"], "coding");
        // Ungated tools are never held back.
        assert!(r.pending("cognitive.sequential_thinking").is_none());
        assert!(r.pending("cognitive.memory_store").is_none());
    }

    #[test]
    fn populated_collections_are_ready_and_cached() {
        let fake = Arc::new(Fake::new(&[
            ("code_a", Ok(Some(5))),
            ("code_b", Ok(Some(7))),
            ("know", Ok(None)),
        ]));
        let r = Readiness::new(fake.clone(), gates(), Duration::from_secs(60));
        assert!(r.pending("cognitive.code_context").is_none());
        assert!(r.pending("cognitive.code_index").is_none());
        assert_eq!(
            fake.calls.load(Ordering::SeqCst),
            2,
            "second call hit the cache"
        );
        let knowledge = r.pending("snowball.search_own_events").unwrap();
        assert!(knowledge.detail.unwrap().contains("know: absent"));
        let snapshot = r.snapshot();
        assert!(snapshot[0].is_ready());
        assert!(!snapshot[1].is_ready());
    }

    #[test]
    fn expired_cache_reprobes() {
        let fake = Arc::new(Fake::new(&[
            ("code_a", Ok(Some(1))),
            ("code_b", Ok(Some(1))),
        ]));
        let r = Readiness::new(fake.clone(), gates(), Duration::ZERO);
        r.pending("cognitive.code_search");
        r.pending("cognitive.code_search");
        assert_eq!(fake.calls.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn collection_names_are_path_safe() {
        assert!(valid_collection("repos_lsp_rust_voyage_4_large"));
        for bad in ["", ".", "..", "a/b", "a b", "a?x=1", "ä"] {
            assert!(!valid_collection(bad), "{bad}");
        }
        let gate = Gate {
            group: "g".into(),
            tools: vec![],
            collections: vec!["../x".into()],
        };
        let fake = Fake::new(&[]);
        let status = probe_gate(&fake, &gate);
        assert_eq!(status.status, PENDING);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn url_parsing_is_http_host_port_only() {
        let q = QdrantRest::new("http://127.0.0.1:6333/", None).unwrap();
        assert_eq!((q.host.as_str(), q.port), ("127.0.0.1", 6333));
        let q = QdrantRest::new("http://qdrant", None).unwrap();
        assert_eq!(q.port, 80);
        let q = QdrantRest::new("http://[::1]:6333", None).unwrap();
        assert_eq!((q.host.as_str(), q.port), ("[::1]", 6333));
        for bad in [
            "https://127.0.0.1:6333",
            "127.0.0.1:6333",
            "http://",
            "http://a:b",
            "http://u@h:1",
            "http://h:1/collections",
            "http://:6333",
        ] {
            assert!(QdrantRest::new(bad, None).is_err(), "{bad}");
        }
    }

    #[test]
    fn responses_parse_with_and_without_chunking() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";
        assert_eq!(parse_response(plain).unwrap(), (200, b"{}".to_vec()));
        let chunked =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n{\"a\r\n4\r\n\":1}\r\n0\r\n\r\n";
        assert_eq!(
            parse_response(chunked).unwrap(),
            (200, br#"{"a":1}"#.to_vec())
        );
        assert!(parse_response(b"HTTP/1.1 200").is_err());
    }

    #[test]
    fn qdrant_rest_reads_points_count_from_a_live_socket() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("key");
        std::fs::write(&key, "k3y\n").unwrap();
        let server = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for _ in 0..2 {
                let (mut s, _) = listener.accept().unwrap();
                let mut buf = [0u8; 2048];
                let n = s.read(&mut buf).unwrap();
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let reply = if req.starts_with("GET /collections/full ") {
                    let body = r#"{"result":{"status":"green","points_count":42},"status":"ok"}"#;
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    )
                } else {
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_owned()
                };
                s.write_all(reply.as_bytes()).unwrap();
                seen.push(req);
            }
            seen
        });
        let q = QdrantRest::new(&format!("http://127.0.0.1:{port}"), Some(key)).unwrap();
        assert_eq!(q.points_count("full").unwrap(), Some(42));
        assert_eq!(q.points_count("gone").unwrap(), None);
        let seen = server.join().unwrap();
        assert!(
            seen.iter().all(|r| r.starts_with("GET ")),
            "read-only probe"
        );
        assert!(seen[0].contains("api-key: k3y\r\n"));
    }
}
