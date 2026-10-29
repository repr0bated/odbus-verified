//! Projection: one schema in, purpose-selected view out.
//!
//! The sealed schema is the single source of truth: it classifies every
//! field, and each purpose selects the classes it may see. Purposes are a
//! closed per-schema list — an unlisted purpose refuses, exactly like an
//! unmatched tag. The sealed schema stays authoritative; the tables here
//! are a routing view of its field classes.
//!
//! First table mirrors `ctl_plane_chatbot.ReasoningEpisode` as consumed by
//! `plan_vectorization`: the three PII-class fields leave the vectorize
//! view iff the record's PII gate stands (`pii_flagged`).

use serde_json::Value;

/// Closed purpose set. New purposes are added here first, like new tags —
/// never invented at call time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Purpose {
    /// Embedding input: identifiers + content; PII only if ungated.
    Vectorize,
    /// Operator screen: the full record, classes annotated.
    Display,
    /// Overlays and restyle: identifiers only, never content or PII.
    Overlay,
}

impl Purpose {
    /// Closed parse: unknown names yield `None` (refuse downstream).
    pub fn parse(name: &str) -> Option<Purpose> {
        match name {
            "vectorize" => Some(Purpose::Vectorize),
            "display" => Some(Purpose::Display),
            "overlay" => Some(Purpose::Overlay),
            _ => None,
        }
    }
}

/// Sealed-schema field classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldClass {
    Identifier,
    Content,
    Pii,
    System,
}

/// One classified field path (JSON pointer, e.g. `/goal_text`).
pub struct FieldRule {
    pub path: &'static str,
    pub class: FieldClass,
}

/// Per-schema closed projection table.
pub struct SchemaView {
    /// Sealed schema name, e.g. `ctl_plane_chatbot.ReasoningEpisode`.
    pub schema: &'static str,
    pub purposes: &'static [Purpose],
    pub fields: &'static [FieldRule],
    /// Record flag that must read `false` for PII-class fields to pass.
    pub pii_gate: &'static str,
}

/// The episode table: classes mirror what `plan_vectorization` treats as
/// PII (goal, reasoning, decision) versus always-embedded metadata.
const EPISODE: SchemaView = SchemaView {
    schema: "ctl_plane_chatbot.ReasoningEpisode",
    purposes: &[Purpose::Vectorize, Purpose::Display, Purpose::Overlay],
    fields: &[
        FieldRule {
            path: "/episode_id",
            class: FieldClass::Identifier,
        },
        FieldRule {
            path: "/principal_id",
            class: FieldClass::Identifier,
        },
        FieldRule {
            path: "/session_id",
            class: FieldClass::Identifier,
        },
        FieldRule {
            path: "/trigger",
            class: FieldClass::Content,
        },
        FieldRule {
            path: "/exit_reason",
            class: FieldClass::Content,
        },
        FieldRule {
            path: "/outcome_class",
            class: FieldClass::Content,
        },
        FieldRule {
            path: "/tools_consulted",
            class: FieldClass::Content,
        },
        FieldRule {
            path: "/goal_text",
            class: FieldClass::Pii,
        },
        FieldRule {
            path: "/reasoning_summary",
            class: FieldClass::Pii,
        },
        FieldRule {
            path: "/decision_output",
            class: FieldClass::Pii,
        },
        FieldRule {
            path: "/content_hash",
            class: FieldClass::System,
        },
        FieldRule {
            path: "/embedding",
            class: FieldClass::System,
        },
    ],
    pii_gate: "/pii_flagged",
};

const TABLES: &[SchemaView] = &[EPISODE];

/// Why a projection was refused. Gated PII omission is NOT a refusal —
/// flagged fields are dropped from the view, like `plan_vectorization`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionRefusal {
    UnknownSchema(String),
    UnknownPurpose(String),
}

fn admits(purpose: Purpose, class: FieldClass, gate_open: bool) -> bool {
    match (purpose, class) {
        (_, FieldClass::Identifier) => true,
        (Purpose::Display, _) => true,
        (Purpose::Vectorize, FieldClass::Content) => true,
        (Purpose::Vectorize, FieldClass::Pii) => gate_open,
        (Purpose::Overlay, _) => false,
        (_, _) => false,
    }
}

/// Project `value` through `schema` for `purpose`. Unknown schema or
/// purpose refuses; everything else yields the admitted view.
pub fn project(schema: &str, value: &Value, purpose: Purpose) -> Result<Value, ProjectionRefusal> {
    let table = TABLES
        .iter()
        .find(|t| t.schema == schema)
        .ok_or_else(|| ProjectionRefusal::UnknownSchema(schema.to_string()))?;
    if !table.purposes.contains(&purpose) {
        return Err(ProjectionRefusal::UnknownPurpose(format!(
            "{schema}:{}",
            match purpose {
                Purpose::Vectorize => "vectorize",
                Purpose::Display => "display",
                Purpose::Overlay => "overlay",
            }
        )));
    }
    let gate_open = value.pointer(table.pii_gate) != Some(&Value::Bool(true));
    let mut out = serde_json::Map::new();
    for rule in table.fields {
        if !admits(purpose, rule.class, gate_open) {
            continue;
        }
        if let Some(v) = value.pointer(rule.path) {
            if !v.is_null() {
                out.insert(rule.path[1..].to_string(), v.clone());
            }
        }
    }
    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn episode(flagged: bool) -> Value {
        json!({
            "episode_id": "ep-1",
            "principal_id": "p-1",
            "session_id": "s-1",
            "trigger": "chat",
            "exit_reason": "done",
            "outcome_class": "GoalAchieved",
            "tools_consulted": ["get_own_identity"],
            "goal_text": "private goal",
            "reasoning_summary": "private reasoning",
            "decision_output": "private decision",
            "content_hash": "abc",
            "pii_flagged": flagged,
        })
    }

    const SCHEMA: &str = "ctl_plane_chatbot.ReasoningEpisode";

    #[test]
    fn vectorize_redacts_flagged_pii_keeps_metadata() {
        let view = project(SCHEMA, &episode(true), Purpose::Vectorize).unwrap();
        for f in ["goal_text", "reasoning_summary", "decision_output"] {
            assert!(view.get(f).is_none(), "{f} leaked");
        }
        assert_eq!(view["episode_id"], "ep-1");
        assert_eq!(view["outcome_class"], "GoalAchieved");
    }

    #[test]
    fn vectorize_passes_ungated_pii() {
        let view = project(SCHEMA, &episode(false), Purpose::Vectorize).unwrap();
        assert_eq!(view["goal_text"], "private goal");
        assert_eq!(view["reasoning_summary"], "private reasoning");
        assert_eq!(view["decision_output"], "private decision");
    }

    #[test]
    fn overlay_never_carries_content_or_pii() {
        let view = project(SCHEMA, &episode(false), Purpose::Overlay).unwrap();
        assert_eq!(view["episode_id"], "ep-1");
        for f in ["trigger", "goal_text", "reasoning_summary", "content_hash"] {
            assert!(view.get(f).is_none(), "{f} leaked");
        }
    }

    #[test]
    fn display_carries_everything() {
        let view = project(SCHEMA, &episode(true), Purpose::Display).unwrap();
        assert_eq!(view["goal_text"], "private goal");
        assert_eq!(view["content_hash"], "abc");
    }

    #[test]
    fn unknown_schema_and_purpose_refuse() {
        assert_eq!(
            project("nope.Nope", &episode(false), Purpose::Vectorize),
            Err(ProjectionRefusal::UnknownSchema("nope.Nope".into()))
        );
        assert_eq!(Purpose::parse("exfiltrate"), None);
    }
}
