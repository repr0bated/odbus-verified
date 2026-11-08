//! Pure backend-state folding and identity-scoped projection selection.
//!
//! A backend update is a complete object, an explicit merge, or a deletion.
//! Neither method arguments nor method results are state observations.
use anyhow::{Context, Result, ensure};
use serde_json::Value;

use super::{BackendStateUpdate, StateScope, deep_merge};

pub(super) fn validate_scope(scope: &StateScope) -> Result<()> {
    if let StateScope::Private { principal_id, session_id } = scope {
        ensure!(
            !principal_id.is_empty() && !session_id.is_empty()
                && !principal_id.chars().any(char::is_control)
                && !session_id.chars().any(char::is_control),
            "private state requires a verified principal and session"
        );
    }
    Ok(())
}

pub(super) fn scope_key(scope: &StateScope) -> Result<String> {
    validate_scope(scope)?;
    Ok(blake3::hash(&serde_json::to_vec(scope)?).to_hex().to_string())
}

pub(super) fn path_segments<'a>(plugin: &str, path: &'a str) -> Result<Vec<&'a str>> {
    let root = op_plugins::canonical::plugin_path(plugin);
    if path == root {
        return Ok(Vec::new());
    }
    let suffix = path.strip_prefix(&format!("{root}/"))
        .context("state object path is outside its canonical plugin root")?;
    let segments: Vec<_> = suffix.split('/').collect();
    for segment in &segments {
        ensure!(
            !segment.is_empty() && *segment != "." && *segment != ".."
                && segment.len() <= 255 && !segment.contains('\\')
                && !segment.chars().any(char::is_control),
            "invalid state child path"
        );
    }
    Ok(segments)
}

pub(super) fn select_object<'a>(
    state: &'a Value,
    plugin: &str,
    path: &str,
) -> Result<Option<&'a Value>> {
    let mut value = state;
    for segment in path_segments(plugin, path)? {
        let object = value.as_object().context("state path traverses a scalar")?;
        let Some(child) = object.get(segment) else { return Ok(None) };
        value = child;
    }
    ensure!(value.is_object(), "requested state is not an object");
    Ok(Some(value))
}

pub(super) fn fold(
    previous: Option<Value>,
    plugin: &str,
    path: &str,
    update: BackendStateUpdate,
) -> Result<Option<Value>> {
    let segments = path_segments(plugin, path)?;
    if matches!(update, BackendStateUpdate::Unchanged) {
        return Ok(previous);
    }
    if let BackendStateUpdate::Replace(value) | BackendStateUpdate::Merge(value) | BackendStateUpdate::Fields(value) = &update {
        ensure!(value.is_object(), "backend object state must be a JSON object");
    }
    if segments.is_empty() {
        return match update {
            BackendStateUpdate::Replace(value) => Ok(Some(value)),
            BackendStateUpdate::Merge(patch) => {
                let mut state = previous.unwrap_or_else(|| Value::Object(Default::default()));
                deep_merge(state.as_object_mut().context("plugin state is not an object")?, patch.as_object().expect("checked object"));
                Ok(Some(state))
            }
            BackendStateUpdate::Fields(fields) => {
                let Value::Object(fields) = fields else { unreachable!("checked object") };
                let mut state = previous.unwrap_or_else(|| Value::Object(Default::default()));
                state.as_object_mut().context("plugin state is not an object")?.extend(fields);
                Ok(Some(state))
            }
            BackendStateUpdate::Delete => Ok(None),
            BackendStateUpdate::Unchanged => unreachable!("handled above"),
            BackendStateUpdate::IdentityPublished(_) => anyhow::bail!("paired identity receipts cannot be folded"),
        };
    }
    if previous.is_none() && matches!(update, BackendStateUpdate::Delete) {
        return Ok(None);
    }
    let mut state = previous.unwrap_or_else(|| Value::Object(Default::default()));
    let mut parent = state.as_object_mut().context("plugin state is not an object")?;
    let (leaf, ancestors) = segments.split_last().expect("nonempty child path");
    for segment in ancestors {
        if !parent.contains_key(*segment) {
            if matches!(update, BackendStateUpdate::Delete) {
                return Ok(Some(state));
            }
            parent.insert((*segment).to_owned(), Value::Object(Default::default()));
        }
        parent = parent.get_mut(*segment).and_then(Value::as_object_mut)
            .context("state path traverses a scalar child")?;
    }
    match update {
        BackendStateUpdate::Replace(value) => { parent.insert((*leaf).to_owned(), value); }
        BackendStateUpdate::Merge(patch) => {
            let object = parent.entry((*leaf).to_owned())
                .or_insert_with(|| Value::Object(Default::default()))
                .as_object_mut().context("state merge target is not an object")?;
            deep_merge(object, patch.as_object().expect("checked object"));
        }
        BackendStateUpdate::Fields(fields) => {
            let Value::Object(fields) = fields else { unreachable!("checked object") };
            parent.entry((*leaf).to_owned()).or_insert_with(|| Value::Object(Default::default()))
                .as_object_mut().context("state fields target is not an object")?.extend(fields);
        }
        BackendStateUpdate::Delete => { parent.remove(*leaf); }
        BackendStateUpdate::Unchanged => unreachable!("handled above"),
        BackendStateUpdate::IdentityPublished(_) => anyhow::bail!("paired identity receipts cannot be folded"),
    }
    Ok(Some(state))
}

pub(super) fn state_hash(state: Option<&Value>) -> Result<String> {
    Ok(blake3::hash(&serde_json::to_vec(state.unwrap_or(&Value::Null))?)
        .to_hex().to_string())
}

pub(super) fn public_state(
    plugin_id: &str,
    schema: &op_state_store::PluginSchema,
    value: Value,
) -> Result<Option<Value>> {
    let raw = simd_json::serde::to_owned_value(value)?;
    Ok(schema.project_public_state(&raw)?
        .map(|value| serde_json::to_value(crate::state_reader::public_plugin_value(plugin_id, &value)))
        .transpose()?)
}
