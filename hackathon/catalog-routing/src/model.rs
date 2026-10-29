//! Request, grants, and decision types: `routing/DATA-MODEL.md` "Types".

use serde::{Deserialize, Serialize};

/// Renderer target. Web is the reference renderer; team chat is the first
/// drop-in target (`docs/DROP-IN-SPEC.md` §2.1). Teams and email are planned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Surface {
    Web,
    TeamChat,
    Teams,
    Email,
}

/// What the request does. Qualifier tags use the same words
/// (`action:read`, `action:write`, `action:publish`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActionKind {
    Read,
    Write,
    Publish,
}

/// Adapter output. `tags` carries ≥1 control tag (`oscal/TAG-TAXONOMY.md`)
/// plus qualifier tags (`action:*`, `approval:root`, `surface:*`, `model:*`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub actor: String,
    pub surface: Surface,
    pub action: ActionKind,
    pub tags: Vec<String>,
    pub payload_ref: String,
}

/// Compiled profile: which controls apply. Grants (authority) arrive
/// separately — tags never grant (`oscal/TAG-TAXONOMY.md` rules).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub controls: Vec<String>,
}
/// The identity's compiled grants: exact sealed capability strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grants {
    pub capabilities: Vec<String>,
}

impl Grants {
    /// Sealed capabilities are opaque strings: exact match only.
    pub fn covers(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|c| c == capability)
    }
}

/// A candidate plugin operation: `operations/<plugin>/<method>/call`.
///
/// `capability` is the exact sealed capability (`opblob inspect`), or
/// `None` when no blob is sealed for the method yet — a method exists if
/// and only if its blob is in the sealed catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    pub plugin: String,
    pub method: String,
    pub capability: Option<String>,
}

/// The verdict. Refusals name the missing capability and the rule;
/// root actions need two different humans (mirrors `approvals/`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    Allow,
    Refuse {
        missing_capability: Option<String>,
        rule: String,
    },
    NeedsApproval {
        have: u32,
        need: u32,
        rule: String,
    },
}

/// The decision: candidate operations plus the verdict on them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub operations: Vec<Operation>,
    pub verdict: Verdict,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grants_cover_exact_capability_only() {
        let grants = Grants {
            capabilities: vec!["cron.invoke".into()],
        };
        assert!(grants.covers("cron.invoke"));
        assert!(!grants.covers("cron.read"));
        assert!(!grants.covers("cron"));
    }

    #[test]
    fn operation_carries_sealed_capability() {
        let op = Operation {
            plugin: "cron".into(),
            method: "create_job".into(),
            capability: Some("cron.invoke".into()),
        };
        assert_eq!(op.capability.as_deref(), Some("cron.invoke"));
    }
}
