//! Acceptance against `routing/DATA-MODEL.md` (tag → control table) and
//! `docs/DROP-IN-SPEC.md` §2.2 (every action takes one path).

#[cfg(test)]
mod tests {
    use crate::catalogs::models::{select, ModelEntry};
    use crate::catalogs::ui::{check, Support};
    use crate::decide;
    use crate::model::*;

    fn profile() -> Profile {
        Profile {
            id: "small-business-portal".into(),
            controls: vec![
                "etched-pt-1".into(),
                "etched-ac-1".into(),
                "etched-au-1".into(),
                "etched-si-1".into(),
                "etched-in-1".into(),
            ],
        }
    }

    fn grants() -> Grants {
        Grants {
            capabilities: vec![
                "cap.governance.schema-tag-registry.read@v1".into(),
                "cap.identity.sled.self.read@v1".into(),
                "cap.governance.routing-decision.read@v1".into(),
                "identity_sled.read".into(),
            ],
        }
    }

    fn request(action: ActionKind, surface: Surface, tags: &[&str]) -> Request {
        Request {
            actor: "principal-1".into(),
            surface,
            action,
            tags: tags.iter().map(|s| s.to_string()).collect(),
            payload_ref: "payload-1".into(),
        }
    }

    #[test]
    fn publish_article_routes_redaction_then_index() {
        let d = decide(
            &request(
                ActionKind::Publish,
                Surface::Web,
                &["pii", "action:publish"],
            ),
            &profile(),
            &grants(),
        );
        assert_eq!(d.verdict, Verdict::Allow);
        assert!(d.operations.iter().any(|o| o.method == "get_policy"));
    }

    #[test]
    fn show_receipts_is_allowed_read() {
        let d = decide(
            &request(
                ActionKind::Read,
                Surface::TeamChat,
                &["audit", "action:read"],
            ),
            &profile(),
            &grants(),
        );
        assert_eq!(d.verdict, Verdict::Allow);
        assert_eq!(check(Surface::TeamChat, "receipt", None), Support::Full);
    }

    #[test]
    fn anchor_release_needs_two_humans() {
        let d = decide(
            &request(
                ActionKind::Write,
                Surface::Web,
                &["integrity", "approval:root"],
            ),
            &profile(),
            &grants(),
        );
        assert_eq!(
            d.verdict,
            Verdict::NeedsApproval {
                have: 0,
                need: 2,
                rule: "two-person-root".into(),
            }
        );
    }

    #[test]
    fn out_of_bounds_refusal_names_capability() {
        let d = decide(
            &request(
                ActionKind::Write,
                Surface::Web,
                &["retention", "action:write"],
            ),
            &profile(),
            &grants(),
        );
        assert_eq!(
            d.verdict,
            Verdict::Refuse {
                missing_capability: Some("cron.invoke".into()),
                rule: "etched-si-1".into(),
            }
        );
    }

    #[test]
    fn vision_request_selects_vision_model() {
        let registry = vec![ModelEntry {
            id: "fixture-vision".into(),
            provider: "fixture".into(),
            capabilities: vec!["chat".into(), "vision".into()],
        }];
        let got = select(&["model:vision".into()], &registry).unwrap();
        assert_eq!(got.id, "fixture-vision");
        assert!(select(&["model:video".into()], &registry).is_none());
    }
}
