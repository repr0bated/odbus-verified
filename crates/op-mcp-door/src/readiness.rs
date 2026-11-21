//! Vector readiness for the hot coding and knowledge tools.
//!
//! Coding and Snowball knowledge use readiness counts published after durable Cozo writes.

use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Status string returned for a gated tool whose vectors are not ready.
pub const PENDING: &str = "pending_revectorization";

/// Hot coding tools (`<plugin>.<method>`).
pub const CODING_TOOLS: &[&str] = &["cognitive.code_search", "cognitive.code_context"];
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

/// Collection names use `[A-Za-z0-9_.-]` passes.
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

/// Keep database files private to the footprint indexer. The door consumes
/// only a count that the indexer atomically publishes after the index commits.
pub struct CozoReadiness {
    footprint_path: PathBuf,
    semantic_path: PathBuf,
}

impl CozoReadiness {
    pub fn new(footprint_path: PathBuf, semantic_path: PathBuf) -> Self {
        Self {
            footprint_path,
            semantic_path,
        }
    }
}

impl VectorProbe for CozoReadiness {
    fn points_count(&self, collection: &str) -> Result<Option<u64>, String> {
        use sha2::{Digest, Sha256};
        let path = if KNOWLEDGE_COLLECTIONS.contains(&collection) {
            self.footprint_path.join(".indexed-count")
        } else {
            let fingerprint = Sha256::digest(collection.as_bytes());
            self.semantic_path
                .join(format!(".indexed-count-{fingerprint:x}"))
        };
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("read Cozo vector readiness: {error}")),
        };
        text.trim()
            .parse()
            .map(Some)
            .map_err(|_| "invalid Cozo vector count".into())
    }
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

/// Gates with a short cache to avoid repeated readiness file reads.
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
    fn cozo_footprint_readiness_uses_local_count() {
        let directory = tempfile::tempdir().unwrap();
        let probe = CozoReadiness::new(directory.path().into(), directory.path().into());
        assert_eq!(probe.points_count("snowball_footprints"), Ok(None));
        std::fs::write(directory.path().join(".indexed-count"), "12\n").unwrap();
        assert_eq!(probe.points_count("snowball_footprints"), Ok(Some(12)));
        use sha2::{Digest, Sha256};
        let collection = CODING_COLLECTIONS[0];
        let fingerprint = Sha256::digest(collection.as_bytes());
        std::fs::write(
            directory
                .path()
                .join(format!(".indexed-count-{fingerprint:x}")),
            "4\n",
        )
        .unwrap();
        assert_eq!(probe.points_count(collection), Ok(Some(4)));
    }

    #[test]
    fn default_gates_cover_the_hot_coding_and_knowledge_tools() {
        let gates = default_gates(None, None);
        assert_eq!(gates[0].group, "coding");
        assert_eq!(gates[0].tools.len(), 2);
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
}
