//! Model catalog: provider/model selection over a caller-supplied registry.
//!
//! The live `chat_router` catalog is authoritative. This module only matches:
//! `model:*` request tags must all be covered by the entry's capabilities,
//! and the first covering entry wins (registries are preference-ordered).
//! No provider or model names live here — fixtures in tests are marked.

/// One registry entry, supplied by the caller (runtime: `chat_router`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelEntry {
    pub id: String,
    pub provider: String,
    pub capabilities: Vec<String>,
}

/// First registry entry covering every `model:*` tag's requirement.
/// No requirements → first entry. No covering entry → `None` (refuse
/// upstream, never fall back to an unqualified model silently).
pub fn select<'a>(tags: &[String], registry: &'a [ModelEntry]) -> Option<&'a ModelEntry> {
    let required: Vec<&str> = tags
        .iter()
        .filter_map(|t| t.strip_prefix("model:"))
        .collect();
    registry.iter().find(|entry| {
        required
            .iter()
            .all(|r| entry.capabilities.iter().any(|c| c == r))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test fixtures only — not real providers.
    fn registry() -> Vec<ModelEntry> {
        vec![
            ModelEntry {
                id: "fixture-chat".into(),
                provider: "fixture".into(),
                capabilities: vec!["chat".into()],
            },
            ModelEntry {
                id: "fixture-vision".into(),
                provider: "fixture".into(),
                capabilities: vec!["chat".into(), "vision".into()],
            },
        ]
    }

    #[test]
    fn no_requirements_selects_first() {
        let reg = registry();
        assert_eq!(select(&["audit".into()], &reg).unwrap().id, "fixture-chat");
    }

    #[test]
    fn requirement_selects_covering_entry() {
        let reg = registry();
        let got = select(&["model:vision".into()], &reg).unwrap();
        assert_eq!(got.id, "fixture-vision");
    }

    #[test]
    fn uncovered_requirement_selects_none() {
        let reg = registry();
        assert!(select(&["model:video".into()], &reg).is_none());
        assert!(select(&["model:vision".into()], &[]).is_none());
    }
}
