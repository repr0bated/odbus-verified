//! One preview holds the CMS spec and the Cozo relations together.
//!
//! Neither artifact is committed until [`StagedPair::publish`]. Deny drops
//! both. The chatbot never receives the spec; this module only applies the
//! already-built console generator to the same rows it turns into relations.

use serde_json::Value;

use super::specgen::{console_spec, GrantRow};

/// Uncommitted pair. Dropping it, or calling [`StagedPair::deny`], writes nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct StagedPair {
    spec: Value,
    relations: String,
}

/// What a reviewer sees. This is not a commit.
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    pub spec: Value,
    pub relations: String,
}

/// Both artifacts, released together. There is no half commit.
#[derive(Debug, Clone, PartialEq)]
pub struct Published {
    pub spec: Value,
    pub relations: String,
}

/// Build the CMS spec and the Cozo `:create` script from the same rows.
/// Nothing is stored.
pub fn stage(title: &str, subtitle: &str, rows: &[GrantRow]) -> StagedPair {
    StagedPair {
        spec: console_spec(title, subtitle, rows),
        relations: relations_script(rows),
    }
}

impl StagedPair {
    /// View of the output. Does not commit.
    pub fn view(&self) -> Preview {
        Preview {
            spec: self.spec.clone(),
            relations: self.relations.clone(),
        }
    }

    /// Commit both. The caller must persist both or persist neither.
    pub fn publish(self) -> Published {
        Published {
            spec: self.spec,
            relations: self.relations,
        }
    }

    /// Drop both. Nothing is returned to persist.
    pub fn deny(self) {}
}

fn relations_script(rows: &[GrantRow]) -> String {
    let mut script = String::from(
        ":create cms_grant {id: String => principal: String, capability: String, effect: String, decision: String}\n",
    );
    script.push_str("?[id, principal, capability, effect, decision] <- [");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            script.push_str(", ");
        }
        script.push_str(&format!(
            "[{id:?}, {principal:?}, {capability:?}, {effect:?}, {decision:?}]",
            id = format!("grant-{index}"),
            principal = row.principal,
            capability = row.capability,
            effect = row.effect,
            decision = row.decision,
        ));
    }
    script.push_str("]\n:put cms_grant {id => principal, capability, effect, decision}\n");
    script
}
