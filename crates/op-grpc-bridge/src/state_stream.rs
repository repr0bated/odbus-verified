//! StateSync reads of committed public SHM projections.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;

use crate::sealed_schema_reader::{
    ManifestPinnedCatalog, read_manifest_pinned_catalog, valid_plugin_id,
};

const MAX_VIEW_DEPTH: usize = 64;
const MAX_SELECTED_VIEWS: usize = 4096;
const MAX_SELECTED_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PluginSnapshot {
    pub schema_hash: String,
    pub schema: Value,
    pub state: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StateSnapshot {
    pub generation: u64,
    pub catalog_hash: String,
    pub plugins: BTreeMap<String, PluginSnapshot>,
}

pub(crate) async fn read_public_state_snapshot(
    requested_plugin_ids: &[String],
) -> Result<StateSnapshot> {
    read_public_state_snapshot_with_admission(requested_plugin_ids, false).await
}

pub(crate) async fn read_public_state_snapshot_after_admission(
    requested_plugin_ids: &[String],
) -> Result<StateSnapshot> {
    read_public_state_snapshot_with_admission(requested_plugin_ids, true).await
}

async fn read_public_state_snapshot_with_admission(
    requested_plugin_ids: &[String],
    allow_missing: bool,
) -> Result<StateSnapshot> {
    read_public_state_snapshot_in(
        crate::sealed_schema_reader::blob_catalog_dir_from_env(),
        op_core::projection_shm::shm_state_dir(),
        requested_plugin_ids.to_vec(),
        allow_missing,
    )
    .await
}

pub(crate) async fn read_public_state_snapshot_in(
    catalog_dir: PathBuf,
    state_dir: String,
    requested_plugin_ids: Vec<String>,
    allow_missing: bool,
) -> Result<StateSnapshot> {
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::task::spawn_blocking(move || {
            read_public_state_snapshot_sync(
                &catalog_dir,
                &state_dir,
                &requested_plugin_ids,
                allow_missing,
            )
        }),
    )
    .await
    .context("public SHM snapshot read timed out")?
    .context("public SHM snapshot reader task failed")?
}

fn read_public_state_snapshot_sync(
    catalog_dir: &Path,
    state_dir: &str,
    requested_plugin_ids: &[String],
    allow_missing: bool,
) -> Result<StateSnapshot> {
    for _ in 0..3 {
        let before = read_manifest_pinned_catalog(catalog_dir)
            .context("sealed plugin catalog is unavailable")?;
        let selected = select_plugin_ids(&before, requested_plugin_ids, allow_missing)?;
        let projection = op_core::projection_shm::read_projection_snapshot_in(state_dir, &selected)
            .context("committed public SHM projection is unavailable")?;
        let after = read_manifest_pinned_catalog(catalog_dir)
            .context("sealed plugin catalog is unavailable after SHM read")?;
        if catalog_signature(&before) != catalog_signature(&after) {
            continue;
        }

        let mut plugins = BTreeMap::new();
        for plugin_id in selected {
            let document = after
                .plugins
                .get(&plugin_id)
                .with_context(|| format!("sealed plugin disappeared: {plugin_id}"))?;
            let schema = serde_json::from_str(&document.blob.schema_json)
                .with_context(|| format!("sealed schema JSON is invalid for {plugin_id}"))?;
            let state = projection
                .states
                .get(&plugin_id)
                .map(|bytes| {
                    let mut bytes = bytes.clone();
                    let value = simd_json::to_owned_value(&mut bytes)
                        .with_context(|| format!("public SHM state is invalid for {plugin_id}"))?;
                    let public = crate::mutation_engine::public_plugin_value(&plugin_id, &value);
                    document
                        .schema
                        .project_public_state(&public)?
                        .map(serde_json::to_value)
                        .transpose()
                        .with_context(|| format!("public SHM state is invalid for {plugin_id}"))
                })
                .transpose()?
                .flatten();
            plugins.insert(
                plugin_id,
                PluginSnapshot {
                    schema_hash: document.schema_hash.clone(),
                    schema,
                    state,
                },
            );
        }
        return Ok(StateSnapshot {
            generation: projection.generation,
            catalog_hash: after.catalog_hash,
            plugins,
        });
    }
    bail!("sealed catalog changed repeatedly around the public SHM snapshot")
}

fn select_plugin_ids(
    catalog: &ManifestPinnedCatalog,
    requested: &[String],
    allow_missing: bool,
) -> Result<Vec<String>> {
    if requested.is_empty() {
        return Ok(catalog.plugins.keys().cloned().collect());
    }
    let mut selected = BTreeSet::new();
    for plugin_id in requested {
        ensure!(valid_plugin_id(plugin_id), "invalid plugin id: {plugin_id}");
        if catalog.plugins.contains_key(plugin_id) {
            selected.insert(plugin_id.clone());
        } else if !allow_missing {
            bail!("plugin is absent from the sealed catalog: {plugin_id}");
        }
    }
    Ok(selected.into_iter().collect())
}

fn catalog_signature(catalog: &ManifestPinnedCatalog) -> (&str, BTreeMap<&str, &str>) {
    (
        catalog.catalog_hash.as_str(),
        catalog
            .plugins
            .iter()
            .map(|(id, document)| (id.as_str(), document.schema_hash.as_str()))
            .collect(),
    )
}

pub(crate) fn plugin_object_path(plugin_id: &str) -> String {
    op_plugins::canonical::plugin_path(plugin_id)
}

pub(crate) fn validate_public_path_patterns(patterns: &[String]) -> Result<()> {
    for pattern in patterns {
        ensure!(
            pattern.starts_with("/org/opdbus/v1/plugins/"),
            "state path pattern is outside the public plugin tree"
        );
        ensure!(
            !pattern.contains("/credentials")
                && !pattern.contains("/.credentials")
                && !pattern.contains("..")
                && !pattern.chars().any(char::is_control),
            "private or invalid state path pattern"
        );
        ensure!(
            pattern
                .split('/')
                .skip(1)
                .all(|segment| !segment.is_empty() && segment != "." && segment != ".."),
            "state path pattern contains an invalid segment"
        );
    }
    Ok(())
}

pub(crate) fn validate_path_patterns_against_snapshot(
    snapshot: &StateSnapshot,
    patterns: &[String],
) -> Result<()> {
    const PREFIX: &str = "/org/opdbus/v1/plugins/";
    for pattern in patterns {
        let selector = pattern
            .strip_prefix(PREFIX)
            .and_then(|suffix| suffix.split('/').next())
            .context("state path pattern has no plugin selector")?;
        ensure!(
            !selector.is_empty(),
            "state path pattern has no plugin selector"
        );
        let resolves = snapshot.plugins.keys().any(|plugin_id| {
            if selector.contains('*') || selector.contains('?') {
                glob_matches(selector, plugin_id)
            } else {
                selector == plugin_id
            }
        });
        ensure!(
            resolves,
            "state path pattern does not resolve to a sealed public plugin root: {pattern}"
        );
    }
    Ok(())
}

pub(crate) fn state_at_object_path<'a>(
    plugin_id: &str,
    state: &'a Value,
    object_path: &str,
) -> Result<&'a Value> {
    let root = plugin_object_path(plugin_id);
    if object_path == root {
        ensure!(
            state.is_object(),
            "public plugin root state is not an object"
        );
        return Ok(state);
    }
    let suffix = object_path
        .strip_prefix(&(root.clone() + "/"))
        .with_context(|| format!("object path is outside canonical plugin root {root}"))?;
    ensure!(!suffix.is_empty(), "child object path is empty");
    let mut current = state;
    for segment in suffix.split('/') {
        ensure!(
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && !segment.contains('\\')
                && !segment.chars().any(char::is_control),
            "invalid child object path segment"
        );
        current = current
            .as_object()
            .and_then(|object| object.get(segment))
            .with_context(|| format!("public SHM object path is absent: {object_path}"))?;
    }
    ensure!(
        current.is_object(),
        "requested D-Bus child is not an object"
    );
    Ok(current)
}

pub(crate) fn object_views(
    plugin_id: &str,
    state: &Value,
    patterns: &[String],
) -> Result<BTreeMap<String, Value>> {
    let root = plugin_object_path(plugin_id);
    if patterns.is_empty() {
        ensure!(
            state.is_object(),
            "public plugin root state is not an object"
        );
        ensure!(
            serde_json::to_vec(state)?.len() <= MAX_SELECTED_BYTES,
            "selected public state exceeds 16 MiB"
        );
        return Ok([(root, state.clone())].into_iter().collect());
    }
    let mut output = BTreeMap::new();
    let mut selected_bytes = 0usize;
    for pattern in patterns
        .iter()
        .filter(|pattern| !pattern.contains('*') && !pattern.contains('?'))
    {
        if let Ok(value) = state_at_object_path(plugin_id, state, pattern) {
            insert_selected_view(&mut output, pattern.clone(), value, &mut selected_bytes)?;
        }
    }
    if patterns
        .iter()
        .any(|pattern| pattern.contains('*') || pattern.contains('?'))
    {
        collect_matching_object_views(root, state, patterns, 0, &mut output, &mut selected_bytes)?;
    }
    Ok(output)
}

fn collect_matching_object_views(
    path: String,
    value: &Value,
    patterns: &[String],
    depth: usize,
    output: &mut BTreeMap<String, Value>,
    selected_bytes: &mut usize,
) -> Result<()> {
    ensure!(
        depth <= MAX_VIEW_DEPTH,
        "public state tree exceeds depth 64"
    );
    let Some(object) = value.as_object() else {
        return Ok(());
    };
    if patterns.iter().any(|pattern| glob_matches(pattern, &path)) {
        insert_selected_view(output, path.clone(), value, selected_bytes)?;
    }
    for (segment, child) in object {
        if valid_child_segment(segment) && child.is_object() {
            collect_matching_object_views(
                format!("{path}/{segment}"),
                child,
                patterns,
                depth + 1,
                output,
                selected_bytes,
            )?;
        }
    }
    Ok(())
}

fn insert_selected_view(
    output: &mut BTreeMap<String, Value>,
    path: String,
    value: &Value,
    selected_bytes: &mut usize,
) -> Result<()> {
    if output.contains_key(&path) {
        return Ok(());
    }
    ensure!(
        output.len() < MAX_SELECTED_VIEWS,
        "selected public state exceeds 4096 object frames"
    );
    *selected_bytes = selected_bytes
        .checked_add(serde_json::to_vec(value)?.len())
        .context("selected public state byte count overflow")?;
    ensure!(
        *selected_bytes <= MAX_SELECTED_BYTES,
        "selected public state exceeds 16 MiB"
    );
    output.insert(path, value.clone());
    Ok(())
}

fn valid_child_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && !segment.contains('/')
        && !segment.contains('\\')
        && !segment.chars().any(char::is_control)
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    let (mut pattern_index, mut value_index, mut star, mut retry) = (0, 0, None, 0);
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    while value_index < value.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == b'?' || pattern[pattern_index] == value[value_index])
        {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star = Some(pattern_index);
            pattern_index += 1;
            retry = value_index;
        } else if let Some(star_index) = star {
            pattern_index = star_index + 1;
            retry += 1;
            value_index = retry;
        } else {
            return false;
        }
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sealed_policy_controls_real_committed_child_snapshots() {
        let root = tempfile::tempdir().unwrap();
        let catalog = root.path().join("blobs");
        let state_dir = root.path().join("state").to_str().unwrap().to_string();
        let child = op_state_store::PluginSchema::builder("child")
            .string_field("status", false, "Public child status")
            .build();
        let schema = op_state_store::PluginSchema::builder("demo")
            .object_field("child", child.fields, false, "Observed child")
            .string_field("secret", false, "Internal value")
            .public_state_path("/child")
            .build();
        let private_schema = op_state_store::PluginSchema::builder("private")
            .string_field("secret", false, "Internal value")
            .build();
        let mut store = op_blob::BlobStore::open(&catalog).unwrap();
        for schema in [schema, private_schema] {
            store
                .write(&op_blob::blobify_plugin_schema(
                    &schema.name.clone(),
                    schema,
                ))
                .unwrap();
        }
        let commit = op_core::projection_shm::update_projection_in(&state_dir, "demo", |_| {
            Ok(Some(
                br#"{"child":{"status":"ready","hidden":"secret"},"secret":"secret"}"#.to_vec(),
            ))
        })
        .unwrap();
        op_core::projection_shm::update_projection_in(&state_dir, "private", |_| {
            Ok(Some(br#"{"secret":"secret"}"#.to_vec()))
        })
        .unwrap();
        let snapshot =
            read_public_state_snapshot_in(catalog.clone(), state_dir.clone(), vec![], false)
                .await
                .unwrap();
        assert!(snapshot.generation >= commit.generation);
        let public = snapshot.plugins["demo"].state.as_ref().unwrap();
        assert_eq!(public, &serde_json::json!({"child":{"status":"ready"}}));
        assert_eq!(
            state_at_object_path("demo", public, "/org/opdbus/v1/plugins/demo/child").unwrap(),
            &serde_json::json!({"status":"ready"})
        );
        assert!(snapshot.plugins["private"].state.is_none());
        assert!(!snapshot.plugins["private"].schema.is_null());
        op_core::projection_shm::update_projection_in(&state_dir, "demo", |_| {
            Ok(Some(br#"{"secret":"still internal"}"#.to_vec()))
        })
        .unwrap();
        let after =
            read_public_state_snapshot_in(catalog.clone(), state_dir.clone(), vec![], false)
                .await
                .unwrap();
        assert_eq!(after.plugins["demo"].state, Some(serde_json::json!({})));
        assert!(after.generation > snapshot.generation);
        // A partial/in-progress publication is never emitted as a snapshot.
        std::fs::write(
            std::path::Path::new(&state_dir).join(".manifest.json"),
            br#"{"generation":99,"publishing":true}"#,
        )
        .unwrap();
        assert!(
            read_public_state_snapshot_in(catalog, state_dir, vec![], false)
                .await
                .is_err()
        );
    }

    #[test]
    fn child_paths_project_objects_without_exposing_scalar_members() {
        let state = serde_json::json!({"root":1,"children":{"a":{"value":2},"scalar":3}});
        assert_eq!(
            state_at_object_path("demo", &state, "/org/opdbus/v1/plugins/demo/children/a").unwrap(),
            &serde_json::json!({"value":2})
        );
        let views = object_views(
            "demo",
            &state,
            &["/org/opdbus/v1/plugins/demo/children/*".into()],
        )
        .unwrap();
        assert_eq!(views.len(), 1);
        assert!(views.contains_key("/org/opdbus/v1/plugins/demo/children/a"));
    }

    #[test]
    fn path_glob_is_anchored() {
        assert!(glob_matches("/org/*/demo", "/org/opdbus/v1/plugins/demo"));
        assert!(!glob_matches(
            "/plugins/demo",
            "/org/opdbus/v1/plugins/demo"
        ));
    }

    #[test]
    fn path_patterns_must_resolve_to_a_sealed_plugin_root() {
        let snapshot = StateSnapshot {
            generation: 1,
            catalog_hash: "catalog".into(),
            plugins: [(
                "demo".into(),
                PluginSnapshot {
                    schema_hash: "schema".into(),
                    schema: serde_json::json!({}),
                    state: None,
                },
            )]
            .into_iter()
            .collect(),
        };
        validate_path_patterns_against_snapshot(&snapshot, &["/org/opdbus/v1/plugins/de*".into()])
            .unwrap();
        assert!(
            validate_path_patterns_against_snapshot(
                &snapshot,
                &["/org/opdbus/v1/plugins/private_store".into()],
            )
            .is_err()
        );
    }
}
