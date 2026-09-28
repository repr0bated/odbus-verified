//! The shared pool of agent pieces.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::generator::md_parser::{AgentDefinition, parse_agent_markdown};
use crate::unified::agent_trait::{AgentCapability, AgentCategory, UnifiedAgent};

/// What a piece contributes to an assembled persona.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PieceKind {
    /// How the persona behaves (tone, priorities, method). Rendered first.
    Behavior,
    /// A block of domain knowledge.
    Knowledge,
    /// A worked question/answer pair.
    Example,
    /// An executor the persona may direct (`rust_pro check`, …). Rendered as
    /// "tools available" so the model asks for them instead of guessing.
    Capability,
}

/// Where a piece came from — kept on the assembled persona for audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum PieceSource {
    /// Extracted from a static agent in the unified catalog.
    Catalog { agent_id: String },
    /// Read from a markdown agent definition.
    Markdown { name: String },
    /// A memory entry (namespace + key) — runtime-editable, shared.
    Memory { namespace: String, key: String },
    /// Registered programmatically.
    Runtime,
}

/// One reusable fragment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentPiece {
    pub id: String,
    pub kind: PieceKind,
    /// Domain words this piece is about (`rust`, `dbus`, `security`, …),
    /// lowercase.
    pub domains: Vec<String>,
    /// Free tags used for scoring besides domains.
    pub tags: Vec<String>,
    pub title: String,
    pub body: String,
    pub source: PieceSource,
}

impl AgentPiece {
    /// All lowercase terms the assembler may match against.
    pub fn terms(&self) -> BTreeSet<String> {
        let mut terms: BTreeSet<String> = BTreeSet::new();
        for word in self.domains.iter().chain(self.tags.iter()) {
            terms.insert(word.to_ascii_lowercase());
        }
        for word in tokenize(&self.title) {
            terms.insert(word);
        }
        terms
    }
}

/// The shared pool.
#[derive(Debug, Default, Clone)]
pub struct PieceLibrary {
    pieces: Vec<AgentPiece>,
}

impl PieceLibrary {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed from every agent in the unified static catalog.
    pub fn seeded_from_catalog() -> Self {
        let mut library = Self::new();
        let registry = &crate::unified::GLOBAL_REGISTRY;
        for id in registry.list_ids() {
            if let Some(agent) = registry.get(id) {
                library.add_from_agent(agent.as_ref());
            }
        }
        library
    }

    pub fn pieces(&self) -> &[AgentPiece] {
        &self.pieces
    }

    pub fn len(&self) -> usize {
        self.pieces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    /// Insert or replace by id.
    pub fn upsert(&mut self, piece: AgentPiece) {
        if let Some(existing) = self.pieces.iter_mut().find(|p| p.id == piece.id) {
            *existing = piece;
        } else {
            self.pieces.push(piece);
        }
    }

    /// Break one static agent into shareable pieces.
    pub fn add_from_agent(&mut self, agent: &dyn UnifiedAgent) {
        let agent_id = agent.id().to_string();
        let mut domains = domain_words(agent);
        domains.sort();
        domains.dedup();
        let source = PieceSource::Catalog {
            agent_id: agent_id.clone(),
        };

        let behavior = agent.system_prompt().trim();
        if !behavior.is_empty() {
            self.upsert(AgentPiece {
                id: format!("catalog:{agent_id}:behavior"),
                kind: PieceKind::Behavior,
                domains: domains.clone(),
                tags: vec![agent.name().to_ascii_lowercase()],
                title: format!("{} behaviour", agent.name()),
                body: behavior.to_string(),
                source: source.clone(),
            });
        }
        if let Some(knowledge) = agent.knowledge_base() {
            for (index, (title, body)) in split_sections(knowledge).into_iter().enumerate() {
                self.upsert(AgentPiece {
                    id: format!("catalog:{agent_id}:knowledge:{index}"),
                    kind: PieceKind::Knowledge,
                    domains: domains.clone(),
                    tags: tokenize(&title).into_iter().collect(),
                    title,
                    body,
                    source: source.clone(),
                });
            }
        }
        for (index, (question, answer)) in agent.examples().into_iter().enumerate() {
            self.upsert(AgentPiece {
                id: format!("catalog:{agent_id}:example:{index}"),
                kind: PieceKind::Example,
                domains: domains.clone(),
                tags: tokenize(question).into_iter().collect(),
                title: question.to_string(),
                body: answer.to_string(),
                source: source.clone(),
            });
        }
        if agent.category() == AgentCategory::Execution {
            for capability in agent.capabilities() {
                if let Some((title, body)) = capability_piece(&agent_id, &capability) {
                    self.upsert(AgentPiece {
                        id: format!("catalog:{agent_id}:capability:{}", slug(&title)),
                        kind: PieceKind::Capability,
                        domains: domains.clone(),
                        tags: tokenize(&title).into_iter().collect(),
                        title,
                        body,
                        source: source.clone(),
                    });
                }
            }
        }
    }

    /// Read a markdown agent definition (frontmatter + `## Purpose`,
    /// `## Capabilities`, `## Behavioral Traits`, `## Knowledge Base`,
    /// `## Examples`) into pieces.
    pub fn add_from_markdown(&mut self, markdown: &str) -> anyhow::Result<usize> {
        let definition = parse_agent_markdown(markdown)?;
        Ok(self.add_from_definition(&definition))
    }

    pub fn add_from_definition(&mut self, definition: &AgentDefinition) -> usize {
        let name = definition.name.trim().to_string();
        let mut domains: Vec<String> = tokenize(&definition.description).into_iter().collect();
        domains.extend(tokenize(&name));
        domains.sort();
        domains.dedup();
        let source = PieceSource::Markdown { name: name.clone() };
        let before = self.pieces.len();

        let mut behavior = String::new();
        if let Some(purpose) = definition.purpose.as_deref() {
            behavior.push_str(purpose.trim());
            behavior.push('\n');
        }
        for trait_line in &definition.behavioral_traits {
            behavior.push_str("- ");
            behavior.push_str(trait_line.trim());
            behavior.push('\n');
        }
        if !behavior.trim().is_empty() {
            self.upsert(AgentPiece {
                id: format!("markdown:{}:behavior", slug(&name)),
                kind: PieceKind::Behavior,
                domains: domains.clone(),
                tags: vec![name.to_ascii_lowercase()],
                title: format!("{name} behaviour"),
                body: behavior.trim().to_string(),
                source: source.clone(),
            });
        }
        for (index, block) in definition.knowledge_base.iter().enumerate() {
            let block = block.trim();
            if block.is_empty() {
                continue;
            }
            let title = block
                .lines()
                .next()
                .unwrap_or("knowledge")
                .trim()
                .to_string();
            self.upsert(AgentPiece {
                id: format!("markdown:{}:knowledge:{index}", slug(&name)),
                kind: PieceKind::Knowledge,
                domains: domains.clone(),
                tags: tokenize(&title).into_iter().collect(),
                title,
                body: block.to_string(),
                source: source.clone(),
            });
        }
        for (index, example) in definition.examples.iter().enumerate() {
            let example = example.trim();
            if example.is_empty() {
                continue;
            }
            self.upsert(AgentPiece {
                id: format!("markdown:{}:example:{index}", slug(&name)),
                kind: PieceKind::Example,
                domains: domains.clone(),
                tags: Vec::new(),
                title: example
                    .lines()
                    .next()
                    .unwrap_or("example")
                    .trim()
                    .to_string(),
                body: example.to_string(),
                source: source.clone(),
            });
        }
        for item in &definition.capabilities.items {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            self.upsert(AgentPiece {
                id: format!("markdown:{}:capability:{}", slug(&name), slug(item)),
                kind: PieceKind::Capability,
                domains: domains.clone(),
                tags: tokenize(item).into_iter().collect(),
                title: item.to_string(),
                body: item.to_string(),
                source: source.clone(),
            });
        }
        self.pieces.len() - before
    }

    /// Merge pieces stored as memory entries: `value` is the JSON of an
    /// [`AgentPiece`] without `id`/`source` (they are derived from the entry),
    /// or a plain string body (stored as a knowledge piece tagged by key words).
    pub fn add_from_memory_entry(
        &mut self,
        namespace: &str,
        key: &str,
        value: &serde_json::Value,
    ) -> Option<String> {
        let id = format!("memory:{namespace}:{key}");
        let source = PieceSource::Memory {
            namespace: namespace.to_string(),
            key: key.to_string(),
        };
        let piece = match value {
            serde_json::Value::String(body) => AgentPiece {
                id: id.clone(),
                kind: PieceKind::Knowledge,
                domains: tokenize(key).into_iter().collect(),
                tags: Vec::new(),
                title: key.replace(['_', '-'], " "),
                body: body.clone(),
                source,
            },
            serde_json::Value::Object(map) => {
                let body = map.get("body").and_then(serde_json::Value::as_str)?;
                let kind = map
                    .get("kind")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|k| serde_json::from_value(serde_json::Value::String(k.into())).ok())
                    .unwrap_or(PieceKind::Knowledge);
                let list = |field: &str| -> Vec<String> {
                    map.get(field)
                        .and_then(serde_json::Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(serde_json::Value::as_str)
                                .map(|s| s.to_ascii_lowercase())
                                .collect()
                        })
                        .unwrap_or_default()
                };
                let mut domains = list("domains");
                if domains.is_empty() {
                    domains = tokenize(key).into_iter().collect();
                }
                AgentPiece {
                    id: id.clone(),
                    kind,
                    domains,
                    tags: list("tags"),
                    title: map
                        .get("title")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(key)
                        .to_string(),
                    body: body.to_string(),
                    source,
                }
            }
            _ => return None,
        };
        self.upsert(piece);
        Some(id)
    }
}

fn domain_words(agent: &dyn UnifiedAgent) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    for part in agent.id().split(['-', '_']) {
        if !matches!(
            part,
            "pro" | "expert" | "executor" | "architect" | "agent" | "specialist" | ""
        ) {
            words.push(part.to_ascii_lowercase());
        }
    }
    words.extend(tokenize(agent.description()));
    for capability in agent.capabilities() {
        match capability {
            AgentCapability::RunCode { language } => words.push(language.to_ascii_lowercase()),
            AgentCapability::RunCommand { commands } => {
                words.extend(commands.iter().map(|c| c.to_ascii_lowercase()))
            }
            AgentCapability::CodeReview => words.push("review".into()),
            AgentCapability::ArchitectureDesign => words.push("architecture".into()),
            AgentCapability::SecurityAudit => words.push("security".into()),
            AgentCapability::Documentation => words.push("docs".into()),
            AgentCapability::Debugging => words.push("debug".into()),
            AgentCapability::Optimization => words.push("performance".into()),
            _ => {}
        }
    }
    words
}

fn capability_piece(agent_id: &str, capability: &AgentCapability) -> Option<(String, String)> {
    match capability {
        AgentCapability::RunCode { language } => Some((
            format!("{language} executor"),
            format!(
                "The `{agent_id}` executor can run {language} code and its toolchain \
                 (build, test, lint, format, run). Ask for it by operation instead of \
                 guessing the outcome."
            ),
        )),
        AgentCapability::RunCommand { commands } if !commands.is_empty() => Some((
            format!("{} commands", agent_id),
            format!(
                "The `{agent_id}` executor can run: {}. Ask for it instead of guessing.",
                commands.join(", ")
            ),
        )),
        _ => None,
    }
}

/// Split a knowledge base on markdown `##` headings into (title, body).
pub(crate) fn split_sections(text: &str) -> Vec<(String, String)> {
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut title = String::from("overview");
    let mut body = String::new();
    for line in text.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            if !body.trim().is_empty() {
                sections.push((title.clone(), body.trim().to_string()));
            }
            title = heading.trim().to_string();
            body.clear();
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    if !body.trim().is_empty() {
        sections.push((title, body.trim().to_string()));
    }
    sections
}

/// Lowercase alphanumeric words of at least three characters, stop words removed.
pub(crate) fn tokenize(text: &str) -> Vec<String> {
    const STOP: [&str; 24] = [
        "the", "and", "for", "with", "that", "this", "from", "into", "are", "you", "your", "use",
        "using", "how", "what", "why", "does", "not", "can", "all", "any", "when", "our", "its",
    ];
    let mut out: Vec<String> = Vec::new();
    for raw in text.split(|c: char| !c.is_alphanumeric() && c != '_') {
        let word = raw.trim_matches('_').to_ascii_lowercase();
        if word.len() < 3 || STOP.contains(&word.as_str()) {
            continue;
        }
        if !out.contains(&word) {
            out.push(word);
        }
    }
    out
}

pub(crate) fn slug(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}
