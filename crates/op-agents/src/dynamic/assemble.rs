//! Compose a persona for one task from the shared piece pool.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::pieces::{AgentPiece, PieceKind, PieceLibrary, PieceSource, tokenize};

/// Everything the assembler knows about the task at hand. The caller (the
/// bridge) fills it from the verified session: the question, the path being
/// worked on, recent activity, and any context it already retrieved.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskContext {
    /// The question, instruction or failing output.
    pub question: String,
    /// Path the task is about, if any (drives language detection).
    pub path: Option<String>,
    /// Explicit domain hints from the caller (`rust`, `dbus`, …).
    pub domain_hints: Vec<String>,
    /// Short lines describing recent session activity (tool calls, errors).
    pub activity: Vec<String>,
    /// Context already retrieved by the caller (memory hits, RAG snippets,
    /// knowledge pushes). Rendered verbatim into the persona prompt.
    pub snippets: Vec<String>,
    /// Executor tool names the caller is willing to let this persona direct
    /// (e.g. `rust_pro`). Rendered as available tools.
    pub executors: Vec<String>,
}

impl TaskContext {
    /// All lowercase terms describing the task.
    pub fn terms(&self) -> BTreeSet<String> {
        let mut terms: BTreeSet<String> = self
            .domain_hints
            .iter()
            .map(|h| h.to_ascii_lowercase())
            .collect();
        terms.extend(tokenize(&self.question));
        if let Some(path) = self.path.as_deref() {
            terms.extend(language_terms(path));
            terms.extend(tokenize(path));
        }
        for line in &self.activity {
            terms.extend(tokenize(line));
        }
        terms
    }
}

/// Map a path's extension / well-known file names to domain terms.
pub fn language_terms(path: &str) -> Vec<String> {
    let lower = path.to_ascii_lowercase();
    let mut out: Vec<String> = Vec::new();
    let ext = lower.rsplit('.').next().unwrap_or("");
    let mut push = |s: &str| {
        if !out.iter().any(|o| o == s) {
            out.push(s.to_string());
        }
    };
    match ext {
        "rs" => {
            push("rust");
            push("cargo");
        }
        "py" => push("python"),
        "ts" | "tsx" => {
            push("typescript");
            push("javascript");
        }
        "js" | "jsx" | "mjs" => push("javascript"),
        "go" => push("go"),
        "sh" | "bash" => {
            push("shell");
            push("bash");
        }
        "sql" => push("sql"),
        "proto" => {
            push("grpc");
            push("protobuf");
        }
        "toml" if lower.ends_with("cargo.toml") => {
            push("rust");
            push("cargo");
        }
        "json" | "yaml" | "yml" | "toml" => push("config"),
        "md" => push("docs"),
        _ => {}
    }
    if lower.contains("dbus") || lower.contains("zbus") {
        push("dbus");
    }
    if lower.contains("openflow") || lower.contains("ovs") || lower.contains("rtnetlink") {
        push("network");
    }
    out
}

/// A persona composed for one task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssembledPersona {
    /// Human-readable name derived from the dominant domains.
    pub name: String,
    /// Dominant domain terms, most relevant first.
    pub domains: Vec<String>,
    /// Ids of the pieces used, in render order (audit trail).
    pub piece_ids: Vec<String>,
    /// Where those pieces came from.
    pub sources: Vec<PieceSource>,
    /// The system prompt to hand to the model.
    pub system_prompt: String,
    /// Worked examples included (question, answer).
    pub examples: Vec<(String, String)>,
}

/// Compose a persona from the pool for this task, within `budget_chars` of
/// prompt text. Deterministic: lexical scoring over piece terms, kind-weighted,
/// ties broken by piece id.
pub fn assemble(
    library: &PieceLibrary,
    task: &TaskContext,
    budget_chars: usize,
) -> AssembledPersona {
    let terms = task.terms();
    let mut scored: Vec<(f32, &AgentPiece)> = library
        .pieces()
        .iter()
        .filter_map(|piece| {
            let score = score_piece(piece, &terms);
            (score > 0.0).then_some((score, piece))
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.id.cmp(&b.1.id))
    });

    let mut used: Vec<&AgentPiece> = Vec::new();
    let mut spent = 0usize;
    let mut behaviors = 0usize;
    let mut examples = 0usize;
    for (_, piece) in &scored {
        let cost = piece.body.len() + piece.title.len() + 16;
        match piece.kind {
            PieceKind::Behavior if behaviors >= 2 => continue,
            PieceKind::Example if examples >= 3 => continue,
            _ => {}
        }
        if spent + cost > budget_chars && !used.is_empty() {
            continue;
        }
        spent += cost;
        match piece.kind {
            PieceKind::Behavior => behaviors += 1,
            PieceKind::Example => examples += 1,
            _ => {}
        }
        used.push(piece);
    }
    // Always start from at least one behaviour so the persona has a method.
    if behaviors == 0
        && let Some(fallback) = library
            .pieces()
            .iter()
            .filter(|p| p.kind == PieceKind::Behavior)
            .min_by_key(|p| p.id.clone())
    {
        used.insert(0, fallback);
    }
    used.sort_by_key(|p| kind_rank(p.kind));

    let mut domains: Vec<(String, usize)> = Vec::new();
    for piece in &used {
        for d in &piece.domains {
            if terms.contains(d) {
                if let Some(entry) = domains.iter_mut().find(|(name, _)| name == d) {
                    entry.1 += 1;
                } else {
                    domains.push((d.clone(), 1));
                }
            }
        }
    }
    domains.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let domains: Vec<String> = domains.into_iter().map(|(d, _)| d).take(4).collect();
    let name = if domains.is_empty() {
        "generalist".to_string()
    } else {
        format!("{} specialist", domains.join("/"))
    };

    let mut prompt = String::new();
    prompt.push_str(&format!(
        "You are a {name}, assembled for this task from shared expertise. Ground every claim \
         in the context below; when you are unsure, say what to check and how.\n\n"
    ));
    for kind in [
        PieceKind::Behavior,
        PieceKind::Knowledge,
        PieceKind::Capability,
    ] {
        let section: Vec<&&AgentPiece> = used.iter().filter(|p| p.kind == kind).collect();
        if section.is_empty() {
            continue;
        }
        prompt.push_str(match kind {
            PieceKind::Behavior => "## How you work\n",
            PieceKind::Knowledge => "## What you know\n",
            PieceKind::Capability => "## Tools you may direct\n",
            PieceKind::Example => "",
        });
        for piece in section {
            prompt.push_str(&format!("### {}\n{}\n\n", piece.title, piece.body.trim()));
        }
    }
    if !task.executors.is_empty() {
        prompt.push_str("## Executors available through the bridge\n");
        for executor in &task.executors {
            prompt.push_str(&format!("- `{executor}`\n"));
        }
        prompt.push('\n');
    }
    if !task.activity.is_empty() {
        prompt.push_str("## Recent session activity\n");
        for line in task.activity.iter().take(12) {
            prompt.push_str(&format!("- {}\n", line.trim()));
        }
        prompt.push('\n');
    }
    if !task.snippets.is_empty() {
        prompt.push_str("## Retrieved context\n");
        for snippet in task.snippets.iter().take(12) {
            prompt.push_str(&format!("- {}\n", snippet.trim()));
        }
        prompt.push('\n');
    }
    let examples: Vec<(String, String)> = used
        .iter()
        .filter(|p| p.kind == PieceKind::Example)
        .map(|p| (p.title.clone(), p.body.clone()))
        .collect();
    if !examples.is_empty() {
        prompt.push_str("## Worked examples\n");
        for (question, answer) in &examples {
            prompt.push_str(&format!("Q: {question}\nA: {answer}\n\n"));
        }
    }

    AssembledPersona {
        name,
        domains,
        piece_ids: used.iter().map(|p| p.id.clone()).collect(),
        sources: used.iter().map(|p| p.source.clone()).collect(),
        system_prompt: prompt.trim_end().to_string(),
        examples,
    }
}

fn score_piece(piece: &AgentPiece, terms: &BTreeSet<String>) -> f32 {
    let piece_terms = piece.terms();
    let mut score = 0.0f32;
    for term in terms {
        if piece.domains.iter().any(|d| d == term) {
            score += 3.0;
        } else if piece_terms.contains(term) {
            score += 1.0;
        }
    }
    if score == 0.0 {
        return 0.0;
    }
    score
        * match piece.kind {
            PieceKind::Behavior => 1.2,
            PieceKind::Knowledge => 1.0,
            PieceKind::Capability => 0.9,
            PieceKind::Example => 0.6,
        }
        * match piece.source {
            // Runtime/memory pieces are the freshest shared knowledge.
            PieceSource::Memory { .. } | PieceSource::Runtime => 1.3,
            PieceSource::Markdown { .. } => 1.1,
            PieceSource::Catalog { .. } => 1.0,
        }
}

fn kind_rank(kind: PieceKind) -> u8 {
    match kind {
        PieceKind::Behavior => 0,
        PieceKind::Knowledge => 1,
        PieceKind::Capability => 2,
        PieceKind::Example => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> PieceLibrary {
        let mut lib = PieceLibrary::new();
        for (id, kind, domains, title, body) in [
            (
                "rust-behavior",
                PieceKind::Behavior,
                vec!["rust", "cargo"],
                "Rust method",
                "Read the compiler error first; prefer cargo check.",
            ),
            (
                "dbus-behavior",
                PieceKind::Behavior,
                vec!["dbus"],
                "D-Bus method",
                "Introspect the object before calling it.",
            ),
            (
                "rust-borrow",
                PieceKind::Knowledge,
                vec!["rust"],
                "Borrow checker",
                "E0502: a mutable borrow conflicts with a live shared borrow.",
            ),
            (
                "ovs-flows",
                PieceKind::Knowledge,
                vec!["network", "openflow"],
                "OpenFlow tables",
                "Table 0 classifies; drops in table 44 are normal.",
            ),
            (
                "cargo-exec",
                PieceKind::Capability,
                vec!["rust", "cargo"],
                "rust executor",
                "rust_pro can run check/build/test/clippy.",
            ),
        ] {
            lib.upsert(AgentPiece {
                id: id.into(),
                kind,
                domains: domains.into_iter().map(String::from).collect(),
                tags: vec![],
                title: title.into(),
                body: body.into(),
                source: PieceSource::Runtime,
            });
        }
        lib
    }

    #[test]
    fn selects_pieces_by_task_domain_and_orders_behavior_first() {
        let task = TaskContext {
            question: "error[E0502]: cannot borrow as mutable in snowball.rs".into(),
            path: Some("crates/op-snowball/src/snowball.rs".into()),
            executors: vec!["rust_pro".into()],
            ..Default::default()
        };
        let persona = assemble(&library(), &task, 4_000);
        assert_eq!(persona.piece_ids[0], "rust-behavior");
        assert!(persona.piece_ids.contains(&"rust-borrow".to_string()));
        assert!(persona.piece_ids.contains(&"cargo-exec".to_string()));
        assert!(!persona.piece_ids.contains(&"ovs-flows".to_string()));
        assert!(persona.name.contains("rust"));
        assert!(persona.system_prompt.contains("## How you work"));
        assert!(persona.system_prompt.contains("`rust_pro`"));
    }

    #[test]
    fn different_task_same_pool_yields_different_persona() {
        let task = TaskContext {
            question: "why are packets dropped in openflow table 44 on ovsbr0?".into(),
            domain_hints: vec!["network".into()],
            ..Default::default()
        };
        let persona = assemble(&library(), &task, 4_000);
        assert!(persona.piece_ids.contains(&"ovs-flows".to_string()));
        assert!(!persona.piece_ids.contains(&"rust-borrow".to_string()));
        // No network behaviour exists, so the deterministic fallback provides one.
        assert!(persona.piece_ids.iter().any(|id| id.ends_with("-behavior")));
    }

    #[test]
    fn memory_pieces_outrank_catalog_pieces_and_budget_is_respected() {
        let mut lib = library();
        lib.add_from_memory_entry(
            "agent_pieces",
            "rust_async_lock",
            &serde_json::json!({"kind":"knowledge","domains":["rust"],"title":"Async lock rule","body":"Never hold a std Mutex across an await."}),
        );
        let task = TaskContext {
            question: "rust mutex held across await".into(),
            ..Default::default()
        };
        let persona = assemble(&lib, &task, 4_000);
        let memory_pos = persona
            .piece_ids
            .iter()
            .position(|id| id == "memory:agent_pieces:rust_async_lock");
        let catalog_pos = persona.piece_ids.iter().position(|id| id == "rust-borrow");
        assert!(memory_pos.is_some());
        assert!(catalog_pos.is_none() || memory_pos < catalog_pos);

        let tiny = assemble(&lib, &task, 60);
        assert_eq!(
            tiny.piece_ids.len(),
            1,
            "budget keeps only the best piece: {:?}",
            tiny.piece_ids
        );
    }

    #[test]
    fn catalog_seed_produces_shared_pieces() {
        let lib = PieceLibrary::seeded_from_catalog();
        assert!(lib.len() > 50, "catalog seeded {} pieces", lib.len());
        assert!(lib.pieces().iter().any(|p| p.kind == PieceKind::Behavior));
        assert!(lib.pieces().iter().any(|p| p.kind == PieceKind::Capability));
    }

    #[test]
    fn markdown_definition_becomes_pieces() {
        let md = "---\nname: ovs-flow-doctor\ndescription: OpenFlow drop analysis\nmodel: any\n---\n\n## Purpose\nFind why flows drop.\n\n## Capabilities\n### Analysis\n- read ovs-ofctl dumps\n\n## Behavioral Traits\n- cite table numbers\n\n## Knowledge Base\n- Table 44 drops are loopback prevention\n\n## Example Interactions\n- Q: why table 44? A: loopback prevention\n";
        let mut lib = PieceLibrary::new();
        let n = lib.add_from_markdown(md).expect("parse");
        assert!(n >= 4, "pieces: {n}");
        assert!(lib.pieces().iter().any(|p| p.kind == PieceKind::Capability));
        assert!(
            lib.pieces()
                .iter()
                .any(|p| matches!(p.source, PieceSource::Markdown { .. }))
        );
    }
}
