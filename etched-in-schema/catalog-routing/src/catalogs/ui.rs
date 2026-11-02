//! UI catalog: component support per renderer target.
//!
//! Mapping is `docs/DROP-IN-SPEC.md` §2.1–§2.3 as code; the sealed
//! `json_render` blob is authoritative. A target renders what it supports
//! and degrades the rest predictably — never drops an action or a refusal.

use crate::model::Surface;

/// How a component renders on a target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Support {
    /// Renders natively.
    Full,
    /// Renders with a predictable fallback (attachment, modal, context line).
    Fallback(FallbackKind),
    /// Target cannot render it; the reply must say so, not drop it.
    Unsupported,
}

/// Predictable degradations from the §2.1 mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FallbackKind {
    /// Large tables: attached file plus summary lines.
    AttachmentPlusSummary,
    /// Selects and inputs: modal with the control.
    Modal,
    /// Status, receipts, decisions: context line.
    ContextLine,
}

/// Targets without an implemented mapping yet (`docs/DROP-IN-SPEC.md` §2.1:
/// Teams and email are "later"). Any check against them degrades explicitly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetStatus {
    Implemented,
    Planned,
}

pub fn target_status(target: Surface) -> TargetStatus {
    match target {
        Surface::Web | Surface::TeamChat => TargetStatus::Implemented,
        Surface::Teams | Surface::Email => TargetStatus::Planned,
    }
}

/// Component support on team chat. Web supports every catalog component;
/// unknown components are unsupported everywhere (they fail validation
/// before rendering, so reaching here is a caller bug — still explicit).
pub fn check(target: Surface, component: &str, rows: Option<usize>) -> Support {
    match target {
        Surface::Web => Support::Full,
        Surface::TeamChat => match component {
            "heading" | "text" | "button" | "action" | "key_value" | "list" | "status"
            | "progress" | "receipt" | "refusal" | "approval_request" | "redaction_notice"
            | "policy_decision" => Support::Full,
            "table" => match rows {
                Some(n) if n > 20 => Support::Fallback(FallbackKind::AttachmentPlusSummary),
                _ => Support::Full,
            },
            "select" | "input" => Support::Fallback(FallbackKind::Modal),
            _ => Support::Unsupported,
        },
        Surface::Teams | Surface::Email => match component {
            "receipt" | "refusal" | "approval_request" | "redaction_notice" => Support::Unsupported,
            _ => Support::Unsupported,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_supports_everything() {
        assert_eq!(check(Surface::Web, "anything_new", None), Support::Full);
    }

    #[test]
    fn team_chat_governance_components_are_native() {
        for c in ["receipt", "refusal", "approval_request", "redaction_notice"] {
            assert_eq!(check(Surface::TeamChat, c, None), Support::Full, "{c}");
        }
    }

    #[test]
    fn team_chat_large_table_degrades_to_attachment() {
        assert_eq!(
            check(Surface::TeamChat, "table", Some(500)),
            Support::Fallback(FallbackKind::AttachmentPlusSummary)
        );
        assert_eq!(check(Surface::TeamChat, "table", Some(5)), Support::Full);
    }

    #[test]
    fn team_chat_inputs_degrade_to_modal() {
        assert_eq!(
            check(Surface::TeamChat, "select", None),
            Support::Fallback(FallbackKind::Modal)
        );
    }

    #[test]
    fn planned_targets_are_explicit() {
        assert_eq!(target_status(Surface::Teams), TargetStatus::Planned);
        assert_eq!(check(Surface::Teams, "text", None), Support::Unsupported);
    }
}
