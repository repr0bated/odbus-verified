//! Pure folding of authoritative object/property changes into one plugin state tree.

use anyhow::{bail, ensure, Context, Result};
use serde_json::{Map, Value as JsonValue};
use simd_json::OwnedValue;

use crate::mutation_engine::ChangeType;

#[derive(Debug, Clone, PartialEq)]
pub enum StateTreeFold {
    /// The event carries no state change, or the requested deletion found no node.
    Unchanged,
    /// Complete plugin state after applying the change.
    Present(OwnedValue),
    /// The canonical plugin root was deleted.
    Removed,
}

/// Fold a backend-confirmed set of fields at one object as a single state
/// transaction. Callers commit the final tree once, so paired fields never
/// become independently visible and concurrent sibling branches are retained.
pub(crate) fn merge_plugin_object_fields(
    previous: Option<&OwnedValue>,
    plugin_id: &str,
    object_path: &str,
    fields: &OwnedValue,
) -> Result<StateTreeFold> {
    let fields = serde_json::to_value(fields)?;
    let fields = fields
        .as_object()
        .context("field update must be an object")?;
    logical_path(plugin_id, object_path)?;
    let mut current = previous.cloned();
    for (member, value) in fields {
        let value = simd_json::serde::to_owned_value(value)?;
        match fold_plugin_state(
            current.as_ref(),
            plugin_id,
            object_path,
            ChangeType::PropertySet,
            Some(member),
            &value,
        )? {
            StateTreeFold::Present(state) => current = Some(state),
            _ => unreachable!("a property set produces a complete object"),
        }
    }
    if fields.is_empty() {
        Ok(StateTreeFold::Unchanged)
    } else {
        Ok(StateTreeFold::Present(
            current.context("field update produced no state")?,
        ))
    }
}

/// Fold one already-authoritative state change into the prior complete plugin state.
/// Method arguments and signal payloads are deliberately never folded.
pub fn fold_plugin_state(
    previous: Option<&OwnedValue>,
    plugin_id: &str,
    object_path: &str,
    change_type: ChangeType,
    member_name: Option<&str>,
    new_value: &OwnedValue,
) -> Result<StateTreeFold> {
    if matches!(change_type, ChangeType::MethodCall | ChangeType::Signal) {
        return Ok(StateTreeFold::Unchanged);
    }
    ensure!(
        change_type != ChangeType::SchemaMigration,
        "schema migration has no authoritative state-fold contract"
    );

    let logical = logical_path(plugin_id, object_path)?;
    let mut state = previous
        .map(serde_json::to_value)
        .transpose()
        .context("convert previous plugin state")?;
    let incoming = serde_json::to_value(new_value).context("convert authoritative value")?;

    match change_type {
        ChangeType::PropertySet => {
            if member_name.is_none() {
                ensure!(
                    incoming.is_object(),
                    "whole object state must be a JSON object"
                );
                if logical.segments.is_empty() {
                    return present(incoming);
                }
                let root = object_root_mut(&mut state)?;
                insert_subtree(root, &logical.segments, incoming)?;
                return present(JsonValue::Object(root.clone()));
            }
            let member = member_name.context("property set requires a member at a child path")?;
            validate_segment(member, "property member")?;
            let root = object_root_mut(&mut state)?;
            let object = descend_object_mut(root, &logical.segments, true)?
                .context("property target is absent")?;
            object.insert(member.to_string(), incoming);
            present(JsonValue::Object(root.clone()))
        }
        ChangeType::PropertyDelete => {
            let member = member_name.context("property delete requires a member")?;
            validate_segment(member, "property member")?;
            let Some(root) = existing_root_mut(&mut state)? else {
                return Ok(StateTreeFold::Unchanged);
            };
            let Some(object) = descend_object_mut(root, &logical.segments, false)? else {
                return Ok(StateTreeFold::Unchanged);
            };
            if object.remove(member).is_none() {
                return Ok(StateTreeFold::Unchanged);
            }
            present(JsonValue::Object(root.clone()))
        }
        ChangeType::ObjectAdded => {
            ensure!(
                member_name.is_none(),
                "object add must not include a member"
            );
            ensure!(
                incoming.is_object(),
                "added object state must be a JSON object"
            );
            if logical.segments.is_empty() {
                return present(incoming);
            }
            let root = object_root_mut(&mut state)?;
            insert_subtree(root, &logical.segments, incoming)?;
            present(JsonValue::Object(root.clone()))
        }
        ChangeType::ObjectRemoved => {
            ensure!(
                member_name.is_none(),
                "object removal must not include a member"
            );
            if logical.segments.is_empty() {
                return Ok(StateTreeFold::Removed);
            }
            let Some(root) = existing_root_mut(&mut state)? else {
                return Ok(StateTreeFold::Unchanged);
            };
            if !remove_subtree(root, &logical.segments)? {
                return Ok(StateTreeFold::Unchanged);
            }
            present(JsonValue::Object(root.clone()))
        }
        ChangeType::MethodCall | ChangeType::Signal | ChangeType::SchemaMigration => {
            unreachable!("handled before fold")
        }
    }
}

#[derive(Debug)]
struct LogicalPath {
    segments: Vec<String>,
}

fn logical_path(plugin_id: &str, object_path: &str) -> Result<LogicalPath> {
    ensure!(!plugin_id.is_empty(), "plugin id must not be empty");
    ensure!(
        !plugin_id.contains('/')
            && !plugin_id.contains('\\')
            && !plugin_id.chars().any(char::is_control),
        "plugin id contains invalid path characters"
    );
    let root = op_plugins::canonical::plugin_path(plugin_id);
    if object_path == root {
        return Ok(LogicalPath { segments: vec![] });
    }
    let suffix = object_path
        .strip_prefix(&(root.clone() + "/"))
        .with_context(|| {
            format!("object path is outside canonical plugin root {root}: {object_path}")
        })?;
    let segments = suffix
        .split('/')
        .map(|segment| {
            validate_segment(segment, "child path segment")?;
            Ok(segment.to_string())
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(!segments.is_empty(), "child object path is empty");
    Ok(LogicalPath { segments })
}

fn validate_segment(segment: &str, label: &str) -> Result<()> {
    ensure!(!segment.is_empty(), "{label} must not be empty");
    ensure!(
        segment != "." && segment != "..",
        "{label} traverses the state tree"
    );
    ensure!(segment.len() <= 255, "{label} is too long");
    ensure!(
        !segment.chars().any(char::is_control) && !segment.contains('/') && !segment.contains('\\'),
        "{label} contains invalid path characters"
    );
    Ok(())
}

fn object_root_mut(state: &mut Option<JsonValue>) -> Result<&mut Map<String, JsonValue>> {
    if state.is_none() {
        *state = Some(JsonValue::Object(Map::new()));
    }
    existing_root_mut(state)?.context("previous plugin state is not an object")
}

fn existing_root_mut(state: &mut Option<JsonValue>) -> Result<Option<&mut Map<String, JsonValue>>> {
    match state {
        None => Ok(None),
        Some(JsonValue::Object(object)) => Ok(Some(object)),
        Some(_) => bail!("previous plugin state is not an object"),
    }
}

fn descend_object_mut<'a>(
    root: &'a mut Map<String, JsonValue>,
    segments: &[String],
    create: bool,
) -> Result<Option<&'a mut Map<String, JsonValue>>> {
    let mut current = root;
    for segment in segments {
        if !current.contains_key(segment) {
            if !create {
                return Ok(None);
            }
            current.insert(segment.clone(), JsonValue::Object(Map::new()));
        }
        current = current
            .get_mut(segment)
            .and_then(JsonValue::as_object_mut)
            .with_context(|| format!("state path traverses scalar child '{segment}'"))?;
    }
    Ok(Some(current))
}

fn insert_subtree(
    root: &mut Map<String, JsonValue>,
    segments: &[String],
    value: JsonValue,
) -> Result<()> {
    let (leaf, parents) = segments.split_last().context("object path has no child")?;
    let parent = descend_object_mut(root, parents, true)?.context("object parent is absent")?;
    parent.insert(leaf.clone(), value);
    Ok(())
}

fn remove_subtree(root: &mut Map<String, JsonValue>, segments: &[String]) -> Result<bool> {
    let (leaf, parents) = segments.split_last().context("object path has no child")?;
    let Some(parent) = descend_object_mut(root, parents, false)? else {
        return Ok(false);
    };
    Ok(parent.remove(leaf).is_some())
}

fn present(value: JsonValue) -> Result<StateTreeFold> {
    Ok(StateTreeFold::Present(
        simd_json::serde::to_owned_value(value).context("convert folded plugin state")?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use simd_json::json;
    use simd_json::prelude::ValueObjectAccess;

    #[test]
    fn related_field_updates_preserve_siblings_at_root_and_child() {
        let previous = json!({"status":"observed","model_assignments":{"ovs":"old","cozo":"keep"}});
        let StateTreeFold::Present(state) = merge_plugin_object_fields(
            Some(&previous),
            "router",
            "/org/opdbus/v1/plugins/router",
            &json!({"selected_provider":"provider", "selected_model":"model"}),
        )
        .unwrap() else {
            panic!("expected complete tree")
        };
        assert_eq!(state["status"], json!("observed"));
        assert_eq!(state["selected_provider"], json!("provider"));
        assert_eq!(state["selected_model"], json!("model"));
        let StateTreeFold::Present(state) = merge_plugin_object_fields(
            Some(&state),
            "router",
            "/org/opdbus/v1/plugins/router/model_assignments",
            &json!({"ovs":"new"}),
        )
        .unwrap() else {
            panic!("expected complete tree")
        };
        assert_eq!(
            state["model_assignments"],
            json!({"ovs":"new","cozo":"keep"})
        );
        assert_eq!(state["selected_model"], json!("model"));
        assert!(merge_plugin_object_fields(
            Some(&state),
            "router",
            "/org/opdbus/v1/plugins/other",
            &json!({"x":1})
        )
        .is_err());
        assert!(merge_plugin_object_fields(
            Some(&state),
            "router",
            "/org/opdbus/v1/plugins/router/status",
            &json!({"x":1})
        )
        .is_err());
    }

    fn folded(
        previous: Option<&OwnedValue>,
        path: &str,
        change: ChangeType,
        member: Option<&str>,
        value: &OwnedValue,
    ) -> Result<StateTreeFold> {
        fold_plugin_state(previous, "demo", path, change, member, value)
    }

    fn present_value(result: StateTreeFold) -> OwnedValue {
        match result {
            StateTreeFold::Present(value) => value,
            other => panic!("expected present state, got {other:?}"),
        }
    }

    #[test]
    fn root_whole_state_and_property_updates_preserve_siblings() {
        let root = "/org/opdbus/v1/plugins/demo";
        let first = present_value(
            folded(
                None,
                root,
                ChangeType::PropertySet,
                None,
                &json!({"a":1,"b":2}),
            )
            .unwrap(),
        );
        let second = present_value(
            folded(
                Some(&first),
                root,
                ChangeType::PropertySet,
                Some("a"),
                &json!(3),
            )
            .unwrap(),
        );
        assert_eq!(second, json!({"a":3,"b":2}));
    }

    #[test]
    fn descendant_insert_replace_and_delete_preserve_other_branches() {
        let root = json!({"instances":{"one":{"status":"up"}},"sibling":7});
        let child = "/org/opdbus/v1/plugins/demo/instances/two";
        let inserted = present_value(
            folded(
                Some(&root),
                child,
                ChangeType::PropertySet,
                Some("status"),
                &json!("down"),
            )
            .unwrap(),
        );
        assert_eq!(inserted["instances"]["one"]["status"], "up");
        assert_eq!(inserted["instances"]["two"]["status"], "down");
        assert_eq!(inserted["sibling"], 7);

        let replaced_by_property_change = present_value(
            folded(
                Some(&inserted),
                child,
                ChangeType::PropertySet,
                None,
                &json!({"status":"starting"}),
            )
            .unwrap(),
        );
        assert_eq!(
            replaced_by_property_change["instances"]["two"],
            json!({"status":"starting"})
        );
        assert_eq!(
            replaced_by_property_change["instances"]["one"]["status"],
            "up"
        );

        let replaced = present_value(
            folded(
                Some(&replaced_by_property_change),
                child,
                ChangeType::ObjectAdded,
                None,
                &json!({"status":"ready","pid":4}),
            )
            .unwrap(),
        );
        assert_eq!(replaced["instances"]["two"]["pid"], 4);
        let removed = present_value(
            folded(
                Some(&replaced),
                child,
                ChangeType::ObjectRemoved,
                None,
                &json!("ignored"),
            )
            .unwrap(),
        );
        assert!(removed["instances"].get("two").is_none());
        assert_eq!(removed["instances"]["one"]["status"], "up");
    }

    #[test]
    fn property_delete_and_root_delete_are_distinct() {
        let root_path = "/org/opdbus/v1/plugins/demo";
        let prior = json!({"keep":1,"drop":2});
        let updated = present_value(
            folded(
                Some(&prior),
                root_path,
                ChangeType::PropertyDelete,
                Some("drop"),
                &json!(null),
            )
            .unwrap(),
        );
        assert_eq!(updated, json!({"keep":1}));
        assert_eq!(
            folded(
                Some(&updated),
                root_path,
                ChangeType::ObjectRemoved,
                None,
                &json!(null),
            )
            .unwrap(),
            StateTreeFold::Removed
        );
    }

    #[test]
    fn missing_deletion_does_not_fabricate_empty_state() {
        assert_eq!(
            folded(
                None,
                "/org/opdbus/v1/plugins/demo/missing",
                ChangeType::ObjectRemoved,
                None,
                &json!(null),
            )
            .unwrap(),
            StateTreeFold::Unchanged
        );
    }

    #[test]
    fn rejects_wrong_root_traversal_and_scalar_descent() {
        assert!(folded(
            None,
            "/org/opdbus/v1/plugins/other",
            ChangeType::PropertySet,
            None,
            &json!({})
        )
        .is_err());
        assert!(folded(
            None,
            "/org/opdbus/v1/plugins/demo/../other",
            ChangeType::ObjectAdded,
            None,
            &json!({})
        )
        .is_err());
        let scalar = json!({"child":5,"keep":true});
        assert!(folded(
            Some(&scalar),
            "/org/opdbus/v1/plugins/demo/child/grandchild",
            ChangeType::PropertySet,
            Some("value"),
            &json!(1)
        )
        .is_err());
        assert_eq!(scalar, json!({"child":5,"keep":true}));
    }

    #[test]
    fn signals_and_method_arguments_never_become_state() {
        let prior = json!({"actual":"state"});
        for change in [ChangeType::Signal, ChangeType::MethodCall] {
            assert_eq!(
                folded(
                    Some(&prior),
                    "/org/opdbus/v1/plugins/demo",
                    change,
                    Some("method_or_signal"),
                    &json!({"secret_argument":true}),
                )
                .unwrap(),
                StateTreeFold::Unchanged
            );
        }
    }

    #[test]
    fn ovsdb_delta_path_is_rejected_until_it_has_a_row_fold_contract() {
        let prior = json!({"Open_vSwitch":{"old":1}});
        assert!(fold_plugin_state(
            Some(&prior),
            "net",
            "/org/opdbus/v1/ovsdb/Bridge",
            ChangeType::PropertySet,
            Some("Bridge"),
            &json!({"uuid":{"new":{"name":"br0"}}}),
        )
        .is_err());
        assert_eq!(prior, json!({"Open_vSwitch":{"old":1}}));
    }

    #[test]
    fn schema_migration_requires_an_explicit_owner_contract() {
        assert!(folded(
            None,
            "/org/opdbus/v1/plugins/demo",
            ChangeType::SchemaMigration,
            None,
            &json!({"schema":"not state"})
        )
        .is_err());
    }
}
