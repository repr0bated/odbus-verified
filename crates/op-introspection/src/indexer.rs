//! Persistent D-Bus introspection index backed by CozoDB.
//!
//! Search uses Cozo's native full-text index over service, path, interface,
//! member, description, and signature. Query syntax follows Cozo FTS (including
//! terms, phrases, Boolean operators, and prefix `*`); invalid syntax is an error.

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use op_core::types::{BusType, ObjectInfo, PropertyAccess};
use op_cozo_store::CozoGraphShuttle;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::scanner::ServiceScanner;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IndexStatistics {
    pub total_services: usize,
    pub total_objects: usize,
    pub total_interfaces: usize,
    pub total_methods: usize,
    pub total_properties: usize,
    pub total_signals: usize,
    pub scan_duration_seconds: f64,
    pub indexed_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub service: String,
    pub object_path: String,
    pub interface: String,
    pub item_type: String,
    pub item_name: String,
    pub description: String,
    /// Cozo full-text TF-IDF score; higher values rank first.
    pub relevance_score: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct IndexRecord {
    service: String,
    object_path: String,
    interface: String,
    item_type: String,
    item_name: String,
    description: String,
    signature: String,
}

#[derive(Deserialize)]
struct FtsHit {
    #[serde(flatten)]
    record: IndexRecord,
    score: f64,
}

/// A derived index. A supplied legacy SQLite file remains untouched; CozoDB
/// lives in the sibling `<db_path>.cozo` directory and is rebuilt on demand.
pub struct DbusIndexer {
    store: CozoGraphShuttle,
    scanner: Arc<ServiceScanner>,
}

const ITEMS: &str = "?[service, object_path, interface, item_type, item_name, description, signature, search_text] := *dbus_index_items{service, object_path, interface, item_type, item_name, description, signature, search_text} :limit 1";
const KEY_SELECT: &str = "?[service, object_path, interface, item_type, item_name] := *dbus_index_items{service, object_path, interface, item_type, item_name}";

// Cozo 0.7.6 applies the index's Lowercase filter to ordinary terms but leaves
// prefix literals unchanged. Fold only unquoted prefix terms here, so operators
// and quoted phrase syntax retain their original meaning.
fn normalize_prefix_terms(query: &str) -> String {
    let mut output = String::with_capacity(query.len());
    let mut term = String::new();
    let mut quote = None;
    let mut escaped = false;
    for ch in query.chars() {
        if let Some(delimiter) = quote {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == delimiter {
                quote = None;
            }
        } else if ch == '\'' || ch == '"' {
            output.push_str(&term);
            term.clear();
            output.push(ch);
            quote = Some(ch);
        } else if ch.is_alphanumeric() || ch == '_' {
            term.push(ch);
        } else if ch == '*' {
            output.push_str(&term.to_lowercase());
            term.clear();
            output.push(ch);
        } else {
            output.push_str(&term);
            term.clear();
            output.push(ch);
        }
    }
    output.push_str(&term);
    output
}

impl DbusIndexer {
    pub async fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let path = db_path.as_ref();
        let store = if path == Path::new(":memory:") {
            CozoGraphShuttle::new_in_memory()?
        } else {
            let mut sibling = path.as_os_str().to_os_string();
            sibling.push(".cozo");
            CozoGraphShuttle::new_persistent(PathBuf::from(sibling))
                .context("open introspection Cozo index")?
        };
        for schema in [
            ":create dbus_index_items { service: String, object_path: String, interface: String, item_type: String, item_name: String => description: String, signature: String, search_text: String }",
            ":create dbus_index_stats { id: String => document: String }",
            "::fts create dbus_index_items:search { extractor: search_text, tokenizer: Simple, filters: [Lowercase] }",
        ] {
            if let Err(error) = store.run_query(schema, None) {
                let message = error.to_string();
                if !message.contains("already exists")
                    && !message.contains("AlreadyExists")
                    && !message.contains("conflicts with an existing")
                {
                    return Err(error.into());
                }
            }
        }
        // Existing relations must have the expected shape. A corrupt or unrelated
        // database is an error, not an apparently empty index.
        store
            .run_query(ITEMS, None)
            .context("validate introspection item relation")?;
        store
            .run_query("?[id, document] := *dbus_index_stats{id, document}", None)
            .context("validate introspection statistics relation")?;
        Ok(Self {
            store,
            scanner: Arc::new(ServiceScanner::new()),
        })
    }

    pub async fn build_index(&self, bus_type: BusType) -> Result<IndexStatistics> {
        info!("Starting D-Bus index build for {:?} bus", bus_type);
        let start = std::time::Instant::now();
        let services = self.scanner.list_services(bus_type).await?;
        let timestamp = Utc::now().timestamp();
        let mut stats = IndexStatistics {
            total_services: services.len(),
            total_objects: 0,
            total_interfaces: 0,
            total_methods: 0,
            total_properties: 0,
            total_signals: 0,
            scan_duration_seconds: 0.0,
            indexed_at: timestamp,
        };
        self.clear_index()?;
        for service in &services {
            if let Err(error) = self
                .index_service(bus_type, &service.name, timestamp, &mut stats)
                .await
            {
                warn!(service = %service.name, %error, "failed to index D-Bus service");
            }
        }
        stats.scan_duration_seconds = start.elapsed().as_secs_f64();
        self.store_statistics(&stats)?;
        info!(
            services = stats.total_services,
            objects = stats.total_objects,
            "D-Bus index build complete"
        );
        Ok(stats)
    }

    async fn index_service(
        &self,
        bus_type: BusType,
        service: &str,
        timestamp: i64,
        stats: &mut IndexStatistics,
    ) -> Result<()> {
        let service_path = format!("/{}", service.replace('.', "/"));
        for path in ["/", service_path.as_str()] {
            match self.scanner.introspect(bus_type, service, path).await {
                Ok(object) => {
                    self.store_object(service, &object, timestamp)?;
                    stats.total_objects += 1;
                    stats.total_interfaces += object.interfaces.len();
                    for interface in &object.interfaces {
                        stats.total_methods += interface.methods.len();
                        stats.total_properties += interface.properties.len();
                        stats.total_signals += interface.signals.len();
                    }
                }
                Err(error) => debug!(service, path, %error, "D-Bus introspection skipped"),
            }
        }
        Ok(())
    }

    fn store_object(&self, service: &str, object: &ObjectInfo, _timestamp: i64) -> Result<()> {
        // Re-indexing an object removes members that disappeared from its schema.
        self.store.run_query(
            "?[service, object_path, interface, item_type, item_name] := *dbus_index_items{service, object_path, interface, item_type, item_name}, service = $service, object_path = $path :rm dbus_index_items {service, object_path, interface, item_type, item_name}",
            Some(json!({"service": service, "path": object.path})),
        )?;
        for interface in &object.interfaces {
            self.put(IndexRecord {
                service: service.into(),
                object_path: object.path.clone(),
                interface: interface.name.clone(),
                item_type: "interface".into(),
                item_name: interface.name.clone(),
                description: interface.name.clone(),
                signature: String::new(),
            })?;
            for method in &interface.methods {
                let signature = method
                    .in_args
                    .iter()
                    .chain(&method.out_args)
                    .map(|arg| {
                        format!("{}:{}", arg.name.as_deref().unwrap_or("arg"), arg.signature)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                self.put(IndexRecord {
                    service: service.into(),
                    object_path: object.path.clone(),
                    interface: interface.name.clone(),
                    item_type: "method".into(),
                    item_name: method.name.clone(),
                    description: format!("{}.{}", interface.name, method.name),
                    signature,
                })?;
            }
            for property in &interface.properties {
                let access = match property.access {
                    PropertyAccess::Read => "read",
                    PropertyAccess::Write => "write",
                    PropertyAccess::ReadWrite => "readwrite",
                };
                self.put(IndexRecord {
                    service: service.into(),
                    object_path: object.path.clone(),
                    interface: interface.name.clone(),
                    item_type: "property".into(),
                    item_name: property.name.clone(),
                    description: format!("{}.{}", interface.name, property.name),
                    signature: format!("{} {}", property.signature, access),
                })?;
            }
            for signal in &interface.signals {
                let signature = signal
                    .args
                    .iter()
                    .map(|arg| {
                        format!("{}:{}", arg.name.as_deref().unwrap_or("arg"), arg.signature)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                self.put(IndexRecord {
                    service: service.into(),
                    object_path: object.path.clone(),
                    interface: interface.name.clone(),
                    item_type: "signal".into(),
                    item_name: signal.name.clone(),
                    description: format!("{}.{}", interface.name, signal.name),
                    signature,
                })?;
            }
        }
        Ok(())
    }

    fn put(&self, record: IndexRecord) -> Result<()> {
        let search_text = [
            record.service.as_str(),
            record.object_path.as_str(),
            record.interface.as_str(),
            record.item_name.as_str(),
            record.description.as_str(),
            record.signature.as_str(),
        ]
        .join(" ");
        self.store.run_query(
            "?[service, object_path, interface, item_type, item_name, description, signature, search_text] <- [[$service, $path, $interface, $type, $name, $description, $signature, $search_text]] :put dbus_index_items {service, object_path, interface, item_type, item_name => description, signature, search_text}",
            Some(json!({
                "service": record.service, "path": record.object_path,
                "interface": record.interface, "type": record.item_type,
                "name": record.item_name, "description": record.description,
                "signature": record.signature, "search_text": search_text,
            })),
        )?;
        Ok(())
    }

    fn store_statistics(&self, stats: &IndexStatistics) -> Result<()> {
        self.store.run_query(
            "?[id, document] <- [['current', $document]] :put dbus_index_stats {id => document}",
            Some(json!({"document": serde_json::to_string(stats)?})),
        )?;
        Ok(())
    }

    pub fn get_statistics(&self) -> Result<Option<IndexStatistics>> {
        let rows = self.store.run_query(
            "?[document] := *dbus_index_stats{id: 'current', document}",
            None,
        )?;
        let rows = rows
            .as_array()
            .ok_or_else(|| anyhow!("invalid Cozo statistics result"))?;
        match rows.first() {
            Some(row) => {
                let document = row
                    .get("document")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("malformed persisted index statistics"))?;
                Ok(Some(
                    serde_json::from_str(document).context("decode index statistics")?,
                ))
            }
            None => Ok(None),
        }
    }

    fn search(
        &self,
        query: &str,
        limit: usize,
        item_type: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let k = i64::try_from(limit).unwrap_or(i64::MAX);
        let query = normalize_prefix_terms(query);
        let (script, params) = match item_type {
            Some(kind) => (
                "?[service, object_path, interface, item_type, item_name, description, signature, score] := ~dbus_index_items:search{service, object_path, interface, item_type, item_name, description, signature | query: $query, k: $k, bind_score: score, filter: item_type == $kind}",
                json!({"query": query, "k": k, "kind": kind}),
            ),
            None => (
                "?[service, object_path, interface, item_type, item_name, description, signature, score] := ~dbus_index_items:search{service, object_path, interface, item_type, item_name, description, signature | query: $query, k: $k, bind_score: score}",
                json!({"query": query, "k": k}),
            ),
        };
        let rows = self.store.run_query(script, Some(params))?;
        let hits: Vec<FtsHit> =
            serde_json::from_value(rows).context("decode Cozo full-text index results")?;
        let mut results = Vec::with_capacity(hits.len());
        for FtsHit { record, score } in hits {
            results.push(SearchResult {
                service: record.service,
                object_path: record.object_path,
                interface: record.interface,
                item_type: record.item_type,
                item_name: record.item_name,
                description: record.description,
                relevance_score: score,
            });
        }
        results.sort_by(|a, b| {
            b.relevance_score
                .total_cmp(&a.relevance_score)
                .then_with(|| a.service.cmp(&b.service))
                .then_with(|| a.object_path.cmp(&b.object_path))
                .then_with(|| a.interface.cmp(&b.interface))
                .then_with(|| a.item_type.cmp(&b.item_type))
                .then_with(|| a.item_name.cmp(&b.item_name))
        });
        results.truncate(limit);
        Ok(results)
    }

    pub fn search_methods(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        self.search(query, limit, Some("method"))
    }

    pub fn search_properties(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        self.search(query, limit, Some("property"))
    }

    pub fn search_all(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        self.search(query, limit, None)
    }

    pub fn clear_index(&self) -> Result<()> {
        self.store.run_query(
            &format!("{KEY_SELECT} :rm dbus_index_items {{service, object_path, interface, item_type, item_name}}"),
            None,
        )?;
        self.store.run_query(
            "?[id] := *dbus_index_stats{id} :rm dbus_index_stats {id}",
            None,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_core::types::{InterfaceInfo, MethodInfo, PropertyInfo, SignalInfo};

    #[test]
    fn prefix_normalization_preserves_boolean_and_phrase_syntax() {
        assert_eq!(
            normalize_prefix_terms("Conn* AND \"Changed Signal\" OR Net*"),
            "conn* AND \"Changed Signal\" OR net*"
        );
    }

    fn fixture() -> ObjectInfo {
        ObjectInfo {
            path: "/org/example/Network".into(),
            children: vec![],
            interfaces: vec![InterfaceInfo {
                name: "org.example.Network".into(),
                methods: vec![MethodInfo {
                    name: "Connect".into(),
                    in_args: vec![],
                    out_args: vec![],
                    annotations: Default::default(),
                }],
                properties: vec![PropertyInfo {
                    name: "Connected".into(),
                    signature: "b".into(),
                    access: PropertyAccess::Read,
                }],
                signals: vec![SignalInfo {
                    name: "Changed".into(),
                    args: vec![],
                }],
            }],
        }
    }

    #[tokio::test]
    async fn cozo_index_reopens_with_all_member_kinds_and_statistics() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.db");
        let legacy = b"SQLite format 3\0legacy data";
        std::fs::write(&path, legacy).unwrap();
        {
            let index = DbusIndexer::new(&path).await.unwrap();
            assert!(index.get_statistics().unwrap().is_none());
            index
                .store_object("org.example.Network", &fixture(), 1)
                .unwrap();
            index
                .store_statistics(&IndexStatistics {
                    total_services: 1,
                    total_objects: 1,
                    total_interfaces: 1,
                    total_methods: 1,
                    total_properties: 1,
                    total_signals: 1,
                    scan_duration_seconds: 0.5,
                    indexed_at: 1,
                })
                .unwrap();
            assert_eq!(index.search_methods("connect", 10).unwrap().len(), 1);
            assert_eq!(index.search_methods("Conn*", 10).unwrap().len(), 1);
        }
        let index = DbusIndexer::new(&path).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), legacy);
        assert_eq!(index.get_statistics().unwrap().unwrap().total_signals, 1);
        let results = index.search_all("network", 10).unwrap();
        assert_eq!(results.len(), 4);
        assert!(results.iter().any(|row| row.item_type == "interface"));
        assert!(results.iter().any(|row| row.item_type == "signal"));
        assert_eq!(index.search_properties("CONNECTED", 10).unwrap().len(), 1);
        assert!(index.search_methods("missing", 10).unwrap().is_empty());
        assert!(index.search_all("\"unterminated", 10).is_err());
    }

    #[tokio::test]
    async fn cozo_reindex_removes_stale_members_and_clear_removes_stats() {
        let index = DbusIndexer::new(":memory:").await.unwrap();
        let mut object = fixture();
        index
            .store_object("org.example.Network", &object, 1)
            .unwrap();
        object.interfaces[0].methods.clear();
        index
            .store_object("org.example.Network", &object, 2)
            .unwrap();
        assert!(index.search_methods("Connect", 10).unwrap().is_empty());
        assert_eq!(index.search_all("network", 10).unwrap().len(), 3);
        index.clear_index().unwrap();
        assert!(index.search_all("network", 10).unwrap().is_empty());
        assert!(index.get_statistics().unwrap().is_none());
    }

    #[tokio::test]
    async fn malformed_cozo_path_fails_without_touching_legacy_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.db");
        std::fs::write(&path, b"legacy").unwrap();
        std::fs::write(dir.path().join("index.db.cozo"), b"not a directory").unwrap();
        assert!(DbusIndexer::new(&path).await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"legacy");
    }

    #[tokio::test]
    async fn malformed_persisted_statistics_return_error() {
        let index = DbusIndexer::new(":memory:").await.unwrap();
        index
            .store
            .run_query(
                "?[id, document] <- [['current', '{broken']] :put dbus_index_stats {id => document}",
                None,
            )
            .unwrap();
        assert!(index.get_statistics().is_err());
    }
}
