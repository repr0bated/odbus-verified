//! Subid → UI presentation remap (render path only).
//!
//! Source of truth for the *rules*: `docs/subid-taxonomy.md` (dual id + seven
//! categories). This module does **not** invent taxonomy from a live scrape.
//!
//! Identity stays `uuid` + `subid`. Remap is strictly for rendering:
//! display / arrangement / priority / audience / element-type **role**.
//! Concrete json-render names (`Card`, `Button`, …) are out of scope here —
//! those resolve later in the projector against the gallery/catalog.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The seven subid categories from `docs/subid-taxonomy.md`.
pub const SUBID_CATEGORIES: &[&str] = &["src", "prj", "sch", "mut", "obs", "evt", "exp"];

/// Presentation roles — data-semantic only (no json-render component names).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UiRole {
    /// `exp.*` — consumer-facing surface
    Surface,
    /// `obs` scalar — read-only value
    DisplayValue,
    /// `obs` boolean — on/off state
    StateFlag,
    /// `obs` list of records
    CollectionView,
    /// `obs` record
    RecordView,
    /// `obs` list of scalars
    ValueList,
    /// `mut` boolean
    BinaryControl,
    /// `mut` string
    TextControl,
    /// `mut` integer / number
    NumericControl,
    /// `mut` list of scalars
    MultiChoice,
    /// `mut` list of records
    EditableCollection,
    /// `mut` record
    RecordEditor,
    /// `mut` otherwise / unstructured
    StructuredControl,
    /// `sch.*` — validation attaches to controls
    ValidationCarrier,
    /// `src.*` — hydration / ingress
    HydrationSource,
    /// `evt.*` — trigger / audit binding
    TriggerBinding,
    /// `prj.*` — projection / repeat binding
    RepeatBinding,
}

impl UiRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Surface => "surface",
            Self::DisplayValue => "display-value",
            Self::StateFlag => "state-flag",
            Self::CollectionView => "collection-view",
            Self::RecordView => "record-view",
            Self::ValueList => "value-list",
            Self::BinaryControl => "binary-control",
            Self::TextControl => "text-control",
            Self::NumericControl => "numeric-control",
            Self::MultiChoice => "multi-choice",
            Self::EditableCollection => "editable-collection",
            Self::RecordEditor => "record-editor",
            Self::StructuredControl => "structured-control",
            Self::ValidationCarrier => "validation-carrier",
            Self::HydrationSource => "hydration-source",
            Self::TriggerBinding => "trigger-binding",
            Self::RepeatBinding => "repeat-binding",
        }
    }
}

/// Normalized field shape used only to refine `obs`/`mut` roles.
/// Derived from schema field types when available; optional for method-only rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiFieldShape {
    Boolean,
    String,
    Integer,
    Float,
    ListScalar,
    ListRecord,
    Record,
    Any,
}

/// Extract the category (first segment) from a subid. Doc pattern:
/// `<category>.<component-type>.<subject>.<verb>[.<facet>][@vN]`
pub fn subid_category(subid: &str) -> Option<&str> {
    let base = subid.split('@').next().unwrap_or(subid);
    let cat = base.split('.').next()?;
    if SUBID_CATEGORIES.contains(&cat) {
        Some(cat)
    } else {
        None
    }
}

/// Doc-derived remap: category (+ optional shape) → presentation role.
///
/// Only the **category** drives the role. Remaining subid segments stay the
/// unique element join key; they are not reinterpreted here.
pub fn ui_role_from_subid(subid: &str, shape: Option<UiFieldShape>) -> Option<UiRole> {
    let cat = subid_category(subid)?;
    Some(match cat {
        "exp" => UiRole::Surface,
        "sch" => UiRole::ValidationCarrier,
        "src" => UiRole::HydrationSource,
        "evt" => UiRole::TriggerBinding,
        "prj" => UiRole::RepeatBinding,
        "obs" => match shape.unwrap_or(UiFieldShape::Any) {
            UiFieldShape::Boolean => UiRole::StateFlag,
            UiFieldShape::ListRecord => UiRole::CollectionView,
            UiFieldShape::Record => UiRole::RecordView,
            UiFieldShape::ListScalar => UiRole::ValueList,
            _ => UiRole::DisplayValue,
        },
        "mut" => match shape.unwrap_or(UiFieldShape::Any) {
            UiFieldShape::Boolean => UiRole::BinaryControl,
            UiFieldShape::String => UiRole::TextControl,
            UiFieldShape::Integer | UiFieldShape::Float => UiRole::NumericControl,
            UiFieldShape::ListScalar => UiRole::MultiChoice,
            UiFieldShape::ListRecord => UiRole::EditableCollection,
            UiFieldShape::Record => UiRole::RecordEditor,
            UiFieldShape::Any => UiRole::StructuredControl,
        },
        _ => return None,
    })
}

/// One populated catalog row for dump / API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiSubidProjection {
    pub plugin_id: String,
    pub kind: String,
    pub id: String,
    pub subid: String,
    pub category: String,
    pub role: String,
    /// Unique join key = subid segments after category (stable element id).
    pub element_key: String,
}

/// Strip category; remainder is the unique element key (plus `@vN` if present).
pub fn element_key_from_subid(subid: &str) -> String {
    let (base, ver) = match subid.split_once('@') {
        Some((b, v)) => (b, Some(v)),
        None => (subid, None),
    };
    let mut parts = base.split('.');
    let _cat = parts.next();
    let rest: Vec<&str> = parts.collect();
    let key = rest.join(".");
    match ver {
        Some(v) if !key.is_empty() => format!("{key}@{v}"),
        Some(v) => format!("@{v}"),
        None => key,
    }
}

/// Map a sealed `PluginSchema`'s fields + methods to presentation rows.
pub fn project_schema_ui(plugin_id: &str, schema: &crate::PluginSchema) -> Vec<UiSubidProjection> {
    let mut out = Vec::new();

    if let Some(sid) = schema.subids.get("__schema__") {
        if let Some(row) = row(plugin_id, "schema", plugin_id, sid, None) {
            out.push(row);
        }
    }

    for (fname, field) in &schema.fields {
        let Some(sid) = schema.subids.get(fname) else {
            continue;
        };
        let shape = Some(shape_from_field_type(&field.field_type));
        if let Some(row) = row(plugin_id, "field", fname, sid, shape) {
            out.push(row);
        }
    }

    for (mname, method) in &schema.methods {
        let sid = method.subid.as_str();
        if sid.is_empty() {
            continue;
        }
        let id = if method.name.is_empty() {
            mname.as_str()
        } else {
            method.name.as_str()
        };
        if let Some(row) = row(plugin_id, "method", id, sid, None) {
            out.push(row);
        }
    }

    out.sort_by(|a, b| (&a.kind, &a.id, &a.subid).cmp(&(&b.kind, &b.id, &b.subid)));
    out
}

/// Role histogram for a dump (what populated).
pub fn role_population(rows: &[UiSubidProjection]) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for r in rows {
        *m.entry(r.role.clone()).or_insert(0) += 1;
    }
    m
}

fn row(
    plugin_id: &str,
    kind: &str,
    id: &str,
    subid: &str,
    shape: Option<UiFieldShape>,
) -> Option<UiSubidProjection> {
    let category = subid_category(subid)?.to_string();
    let role = ui_role_from_subid(subid, shape)?;
    Some(UiSubidProjection {
        plugin_id: plugin_id.to_string(),
        kind: kind.to_string(),
        id: id.to_string(),
        subid: subid.to_string(),
        category,
        role: role.as_str().to_string(),
        element_key: element_key_from_subid(subid),
    })
}

fn shape_from_field_type(ft: &crate::FieldType) -> UiFieldShape {
    use crate::FieldType;
    match ft {
        FieldType::Boolean => UiFieldShape::Boolean,
        FieldType::String | FieldType::Enum(_) => UiFieldShape::String,
        FieldType::Integer => UiFieldShape::Integer,
        FieldType::Float => UiFieldShape::Float,
        FieldType::Object(_) | FieldType::Map(_) => UiFieldShape::Record,
        FieldType::Array(inner) => match inner.as_ref() {
            FieldType::Object(_) => UiFieldShape::ListRecord,
            FieldType::String
            | FieldType::Integer
            | FieldType::Float
            | FieldType::Boolean
            | FieldType::Enum(_) => UiFieldShape::ListScalar,
            _ => UiFieldShape::ListScalar,
        },
        FieldType::OneOf(_) | FieldType::Any | FieldType::JsonSchema(_) => UiFieldShape::Any,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_only_from_doc_seven() {
        assert_eq!(
            subid_category("obs.software.plugin.xray.status@v1"),
            Some("obs")
        );
        assert_eq!(subid_category("mut.service.llm.generate@v1"), Some("mut"));
        assert_eq!(subid_category("nope.software.foo.bar@v1"), None);
    }

    #[test]
    fn roles_follow_category_not_component_names() {
        assert_eq!(
            ui_role_from_subid("exp.software.plugin.json-render.components@v1", None),
            Some(UiRole::Surface)
        );
        assert_eq!(
            ui_role_from_subid(
                "obs.software.plugin.cognitive-mcp.running@v1",
                Some(UiFieldShape::Boolean)
            ),
            Some(UiRole::StateFlag)
        );
        assert_eq!(
            ui_role_from_subid(
                "mut.service.snowball.snapshot-interval@v1",
                Some(UiFieldShape::Integer)
            ),
            Some(UiRole::NumericControl)
        );
        // Never Card/Button — role string is abstract
        assert_eq!(UiRole::BinaryControl.as_str(), "binary-control");
    }

    #[test]
    fn element_key_strips_category_keeps_rest() {
        assert_eq!(
            element_key_from_subid("mut.service.llm.generate@v1"),
            "service.llm.generate@v1"
        );
    }
}
