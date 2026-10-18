//! Vectorize on mutation (OD-64).
//!
//! Every committed chain event is already a Snowball timing block. This
//! projector turns each block into a vector as soon as it lands — chain first
//! (`vectors/` subvolume, sidecar with host load and latencies), Qdrant on
//! the replica after `btrfs receive`, or on the origin only under the
//! explicit `OPDBUS_CHAIN_VECTORS_UPSERT_ORIGIN=1` exception the existing
//! design reserves for manual use.
//!
//! The noise filter is the **sealed plugin schema that created the data**
//! ([`SchemaProjectionPolicy`]): the embedding text is the method's (or
//! signal's) schema-described arguments with the descriptions the schema
//! author wrote, `x-secret` and `x-opdbus-private` fields dropped, undeclared
//! keys ignored. Hashes,
//! event ids, raw timestamps and identity metadata stay out of the text and
//! go to the Qdrant payload as allow-listed filters.
//!
//! Failure posture: a mutation is durable before its vector exists, so an
//! embedder or Qdrant outage delays the index and never the control plane;
//! the projector retries loudly and `project_pending` drains the backlog.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use op_cognitive::chain_vectors::{ChainVectorIndex, ProjectionPolicy};
use op_snowball::ChainBlockRef;
use op_state_store::schema_projection::described_fields;
use tokio::sync::{Mutex, Notify};

/// Blocks embedded per pass.
const PASS_LIMIT: usize = 256;
/// Sweep even without a wake, so a missed notify cannot strand a block.
const SWEEP_EVERY: Duration = Duration::from_secs(30);
/// Back-off after the index could not be opened or a pass failed.
const RETRY_AFTER: Duration = Duration::from_secs(60);
/// Sealed schema cache lifetime inside the policy.
const SCHEMA_TTL: Duration = Duration::from_secs(30);

/// A cached sealed schema (or a cached miss) with the time it was read.
type CachedSchema = (Instant, Option<Arc<op_state_store::PluginSchema>>);

/// The bridge's noise filter: sealed schema of `(plugin, method|signal)`.
pub struct SchemaProjectionPolicy {
    catalog: std::path::PathBuf,
    cache: std::sync::Mutex<HashMap<String, CachedSchema>>,
}

impl SchemaProjectionPolicy {
    pub fn new(catalog: std::path::PathBuf) -> Self {
        Self {
            catalog,
            cache: std::sync::Mutex::new(HashMap::new()),
        }
    }

    fn schema(&self, plugin: &str) -> Option<Arc<op_state_store::PluginSchema>> {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, schema)) = cache.get(plugin)
            && at.elapsed() < SCHEMA_TTL
        {
            return schema.clone();
        }
        let schema = crate::sealed_schema_reader::read_manifest_pinned_schema_document(
            &self.catalog,
            plugin,
        )
        .ok()
        .map(|pinned| Arc::new(pinned.schema));
        cache.insert(plugin.to_string(), (Instant::now(), schema.clone()));
        schema
    }

    /// The schema-described lines for a block, or `None` when the plugin is
    /// not sealed (retired/unknown): then only the header is embedded.
    fn lines_for(&self, block: &ChainBlockRef) -> (Vec<(String, String)>, Option<String>) {
        let mut lines = Vec::new();
        let Some(schema) = self.schema(&block.category) else {
            return (lines, None);
        };
        let data: serde_json::Value =
            simd_json::serde::from_owned_value(block.data.clone()).unwrap_or_default();
        let payload = &data["payload"];
        let input = payload
            .get("input_payload")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        lines.push((
            "plugin".into(),
            format!(
                "{} — {}",
                schema.display_name.as_deref().unwrap_or(&schema.name),
                schema.description
            ),
        ));
        let (args_schema, subid, kind) = if let Some(method) = schema.methods.get(&block.action) {
            let args: serde_json::Value =
                simd_json::serde::from_owned_value(method.args.clone()).unwrap_or_default();
            (Some(args), Some(method.subid.clone()), "method")
        } else if let Some(signal) = schema.signals.iter().find(|s| s.name == block.action) {
            let args = signal.payload.clone().map(|p| {
                simd_json::serde::from_owned_value::<serde_json::Value>(p).unwrap_or_default()
            });
            (args, Some(signal.subid.clone()), "signal")
        } else {
            (None, None, "operation")
        };
        lines.push((kind.into(), block.action.clone()));
        if let Some(subid) = &subid {
            lines.push(("subid".into(), subid.clone()));
        }
        if let Some(args_schema) = args_schema {
            let value = if kind == "signal" {
                // Signals carry their payload as the event's input payload too.
                input.clone()
            } else {
                input
            };
            for field in described_fields(&args_schema, &value) {
                let label = if field.description.is_empty() {
                    field.path
                } else {
                    format!("{} ({})", field.path, field.description)
                };
                let mut value = field.value;
                if value.len() > 512 {
                    let mut cut = 512;
                    while !value.is_char_boundary(cut) {
                        cut -= 1;
                    }
                    value.truncate(cut);
                    value.push('…');
                }
                lines.push((label, value));
            }
        }
        (lines, subid)
    }
}

impl ProjectionPolicy for SchemaProjectionPolicy {
    fn embedding_lines(&self, block: &ChainBlockRef) -> Option<Vec<(String, String)>> {
        Some(self.lines_for(block).0)
    }

    fn payload_fields(&self, block: &ChainBlockRef) -> serde_json::Map<String, serde_json::Value> {
        let mut out = serde_json::Map::new();
        if let Some(subid) = self.lines_for(block).1 {
            out.insert("subid".into(), subid.into());
        }
        if let Some(schema) = self.schema(&block.category) {
            out.insert("schema_version".into(), schema.version.clone().into());
        }
        out
    }
}

/// Wakes the projection loop; the loop owns the index.
#[derive(Clone)]
pub(crate) struct ChainVectorProjector {
    wake: Arc<Notify>,
    stats: Arc<Mutex<ProjectorStats>>,
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub(crate) struct ProjectorStats {
    pub passes: u64,
    pub embedded: u64,
    pub failures: u64,
    pub last_error: Option<String>,
}

impl ChainVectorProjector {
    /// Start the loop. The index opens lazily and retries; nothing here can
    /// fail a mutation.
    pub(crate) fn spawn(policy: Arc<dyn ProjectionPolicy>) -> Self {
        let wake = Arc::new(Notify::new());
        let stats = Arc::new(Mutex::new(ProjectorStats::default()));
        let upsert_origin = std::env::var("OPDBUS_CHAIN_VECTORS_UPSERT_ORIGIN")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let loop_wake = Arc::clone(&wake);
        let loop_stats = Arc::clone(&stats);
        tokio::spawn(async move {
            let mut index: Option<ChainVectorIndex> = None;
            loop {
                if index.is_none() {
                    match ChainVectorIndex::open().await {
                        Ok(opened) => index = Some(opened.with_policy(Arc::clone(&policy))),
                        Err(error) => {
                            tracing::error!(%error, "chain vector projector cannot open its index; mutations are recorded, vectors are delayed");
                            let mut s = loop_stats.lock().await;
                            s.failures += 1;
                            s.last_error = Some(error.to_string());
                            drop(s);
                            tokio::time::sleep(RETRY_AFTER).await;
                            continue;
                        }
                    }
                }
                let Some(current) = index.as_ref() else {
                    continue;
                };
                match current
                    .project_pending(Some(PASS_LIMIT), upsert_origin)
                    .await
                {
                    Ok(summary) => {
                        let mut s = loop_stats.lock().await;
                        s.passes += 1;
                        s.embedded += summary.embedded as u64;
                        s.last_error = None;
                        let more = summary.embedded >= PASS_LIMIT;
                        drop(s);
                        if more {
                            continue; // backlog: keep draining without waiting
                        }
                    }
                    Err(error) => {
                        tracing::error!(%error, "chain vector projection pass failed");
                        let mut s = loop_stats.lock().await;
                        s.failures += 1;
                        s.last_error = Some(error.to_string());
                        drop(s);
                        index = None; // reopen: Qdrant/embedder may have moved
                        tokio::time::sleep(RETRY_AFTER).await;
                        continue;
                    }
                }
                tokio::select! {
                    _ = loop_wake.notified() => {}
                    _ = tokio::time::sleep(SWEEP_EVERY) => {}
                }
            }
        });
        Self { wake, stats }
    }

    /// A chain block was just written.
    pub(crate) fn notify(&self) {
        self.wake.notify_one();
    }

    #[allow(dead_code)]
    pub(crate) async fn stats(&self) -> ProjectorStats {
        self.stats.lock().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_cognitive::chain_vectors::embedding_text_under;

    fn block(plugin: &str, action: &str, input: serde_json::Value) -> ChainBlockRef {
        let data = serde_json::json!({
            "plugin_id": plugin,
            "operation": action,
            "payload": {
                "event_id": 7,
                "event_hash": "97aadd58cafe",
                "prev_hash": "00",
                "input_patch_hash": "beef",
                "input_payload": input,
                "session_genesis": "genesis-secret-hex",
            },
            "metadata": {"actor_id": "principal-1", "session_genesis": "genesis-secret-hex", "decision": "Allow"}
        });
        let mut fields = Vec::new();
        fn flatten(v: &serde_json::Value, prefix: &str, out: &mut Vec<(String, String)>) {
            match v {
                serde_json::Value::Object(m) => {
                    for (k, c) in m {
                        flatten(c, &format!("{prefix}.{k}"), out)
                    }
                }
                serde_json::Value::Array(a) => {
                    for (i, c) in a.iter().enumerate() {
                        flatten(c, &format!("{prefix}[{i}]"), out)
                    }
                }
                serde_json::Value::Null => {}
                s => out.push((
                    prefix.to_string(),
                    s.to_string().trim_matches('"').to_string(),
                )),
            }
        }
        flatten(&data, "data", &mut fields);
        ChainBlockRef {
            block_num: 7,
            hash: "97aadd58cafe".into(),
            category: plugin.into(),
            action: action.into(),
            timestamp: 1,
            data_fields: fields,
            has_vector: false,
            data: simd_json::serde::to_owned_value(data).unwrap(),
        }
    }

    /// Against the real sealed catalog fixture: the chatbot's `set_config`
    /// carries described fields; hashes/genesis are noise and stay out.
    #[test]
    fn schema_policy_embeds_described_fields_and_drops_noise() {
        use crate::human_principal_dispatch::tests::temp_shm_with_schemas;
        use op_state::StatePlugin as _;
        let _shm = temp_shm_with_schemas(&[
            op_plugins::state_plugins::tched_router::TchedRouterPlugin::new()
                .schema()
                .unwrap(),
        ]);
        let catalog = crate::sealed_schema_reader::blob_catalog_dir_from_env();
        let policy = SchemaProjectionPolicy::new(catalog);
        let block = block(
            "tched_router",
            "Chat",
            serde_json::json!({
                "message": "restart the emqx broker",
                "provider": "anthropic",
                "not_declared": "stray-noise-token",
            }),
        );
        let text = embedding_text_under(&policy, &block);
        assert!(text.contains("plugin: "), "{text}");
        assert!(text.contains("method: Chat"), "{text}");
        assert!(text.contains("restart the emqx broker"), "{text}");
        assert!(
            text.contains("message ("),
            "described with its schema description: {text}"
        );
        for noise in [
            "97aadd58cafe",
            "genesis-secret-hex",
            "stray-noise-token",
            "input_patch_hash",
            "event_id",
        ] {
            assert!(!text.contains(noise), "{noise} leaked into {text}");
        }
        let payload = policy.payload_fields(&block);
        assert!(
            payload
                .get("subid")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.contains("chat")),
            "{payload:?}"
        );
        assert!(payload.contains_key("schema_version"));
    }

    /// A committed episode arrives as the `episode_committed` signal; its
    /// declared payload schema is the filter.
    #[test]
    fn signal_payloads_are_projected_through_their_declared_schema() {
        use crate::human_principal_dispatch::tests::temp_shm_with_schemas;
        use op_state::StatePlugin as _;
        let _shm = temp_shm_with_schemas(&[
            op_plugins::state_plugins::ctl_plane_chatbot::CtlPlaneChatbotPlugin::new()
                .schema()
                .unwrap(),
        ]);
        let policy =
            SchemaProjectionPolicy::new(crate::sealed_schema_reader::blob_catalog_dir_from_env());
        let block = block(
            "ctl_plane_chatbot",
            "episode_committed",
            serde_json::json!({
                "goal_text": "why is line1 hot",
                "decision_output": "the fan on line1 stalled",
                "tools_consulted": ["rust_pro"],
                "session_genesis": "genesis-secret-hex",
                "undeclared_junk": "junk-token",
            }),
        );
        let text = embedding_text_under(&policy, &block);
        assert!(text.contains("signal: episode_committed"), "{text}");
        assert!(
            text.contains("why is line1 hot") && text.contains("fan on line1 stalled"),
            "{text}"
        );
        assert!(text.contains("tools_consulted[0]"), "{text}");
        assert!(!text.contains("junk-token"), "{text}");
        // session_genesis IS declared on the episode (identity scope) — it is
        // embedded as the schema says; the identity digest stays in the payload.
        assert!(
            policy.payload_fields(&block)["subid"]
                .as_str()
                .unwrap()
                .starts_with("evt.")
        );
    }

    #[test]
    fn unknown_plugins_embed_only_the_header() {
        let dir = tempfile::tempdir().unwrap();
        let policy = SchemaProjectionPolicy::new(dir.path().to_path_buf());
        let block = block(
            "retired_plugin",
            "do_it",
            serde_json::json!({"secret": "x"}),
        );
        let text = embedding_text_under(&policy, &block);
        assert_eq!(text, "block: 7\nplugin: retired_plugin\naction: do_it");
    }
}
