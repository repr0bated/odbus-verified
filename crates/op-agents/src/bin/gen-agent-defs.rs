//! Emit dynamic agent definitions from the Rust catalog.
//!
//! The Rust catalog (`builtin_agent_descriptors`) is the source of truth; the
//! markdown under `.agents/generated/` is derived output.
//!
//! Output follows the frontmatter + section layout that `src/generator/`
//! defines (`name`/`description`/`model`, then `## Purpose`, `## Capabilities`,
//! `## Behavioral Traits`, `## Knowledge Base`). Note that `src/generator/` is
//! currently orphaned — it is not declared in `lib.rs`, so it never compiles.
//! The layout is matched deliberately so that module can be revived as a reader
//! without reformatting anything here, but nothing verifies the round trip yet.
//!
//!   gen-agent-defs            write definitions
//!   gen-agent-defs --check    exit 1 if on-disk output is stale (CI drift gate)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use op_agents::{builtin_agent_descriptors, AgentDescriptor};

const OUT_DIR: &str = ".agents/generated";
const DEFAULT_MODEL: &str = "sonnet";

fn main() -> Result<()> {
    let check_only = std::env::args().any(|a| a == "--check");
    let root = repo_root()?;
    let out_dir = root.join(OUT_DIR);

    // BTreeMap keyed on agent_type so output ordering is stable across runs —
    // the catalog builds from trait objects and need not be deterministic.
    let rendered: BTreeMap<String, String> = builtin_agent_descriptors()
        .iter()
        .map(|d| (d.agent_type.clone(), render(d)))
        .collect();

    if check_only {
        return check(&out_dir, &rendered);
    }

    // Clear stale files first: an agent removed from the catalog must not
    // linger on disk as a definition nothing backs.
    if out_dir.exists() {
        for entry in std::fs::read_dir(&out_dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "md") {
                std::fs::remove_file(&path)?;
            }
        }
    }
    std::fs::create_dir_all(&out_dir).with_context(|| format!("creating {}", out_dir.display()))?;

    for (agent_type, body) in &rendered {
        let path = out_dir.join(format!("{agent_type}.md"));
        std::fs::write(&path, body).with_context(|| format!("writing {}", path.display()))?;
    }

    println!("wrote {} agent definitions to {}", rendered.len(), OUT_DIR);
    Ok(())
}

fn check(out_dir: &Path, rendered: &BTreeMap<String, String>) -> Result<()> {
    let mut stale = Vec::new();

    for (agent_type, body) in rendered {
        let path = out_dir.join(format!("{agent_type}.md"));
        match std::fs::read_to_string(&path) {
            Ok(on_disk) if &on_disk == body => {}
            Ok(_) => stale.push(format!("{agent_type}: out of date")),
            Err(_) => stale.push(format!("{agent_type}: missing")),
        }
    }

    // Definitions on disk with no backing agent in the catalog.
    if out_dir.exists() {
        for entry in std::fs::read_dir(out_dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "md") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if !rendered.contains_key(stem) {
                        stale.push(format!("{stem}: orphaned (not in catalog)"));
                    }
                }
            }
        }
    }

    if stale.is_empty() {
        println!("{} agent definitions up to date", rendered.len());
        return Ok(());
    }

    eprintln!("agent definitions are stale; run `cargo run -p op-agents --bin gen-agent-defs`:");
    for s in &stale {
        eprintln!("  {s}");
    }
    std::process::exit(1);
}

/// Render one descriptor as markdown in the layout `md_parser` expects:
/// YAML frontmatter (name, description, model) then `## Purpose`,
/// `## Capabilities`, `## Behavioral Traits`, `## Knowledge Base`.
fn render(d: &AgentDescriptor) -> String {
    let mut out = String::new();

    out.push_str("---\n");
    out.push_str(&format!("name: {}\n", yaml_scalar(&d.agent_type)));
    out.push_str(&format!("description: {}\n", yaml_scalar(&d.description)));
    out.push_str(&format!("model: {DEFAULT_MODEL}\n"));
    out.push_str(&format!("category: {:?}\n", d.category));
    out.push_str("generated: true\n");
    out.push_str("---\n\n");

    out.push_str(&format!("# {}\n\n", d.name));
    out.push_str(
        "<!-- Generated from crates/op-agents/src/agent_catalog.rs. Do not edit by hand;\n     \
         edit the agent's Rust implementation and re-run gen-agent-defs. -->\n\n",
    );

    out.push_str("## Purpose\n\n");
    out.push_str(&format!("{}\n\n", d.description));

    out.push_str("## Capabilities\n\n");
    if d.operations.is_empty() {
        out.push_str("_No operations declared._\n\n");
    } else {
        for op in &d.operations {
            match d.schema_for(op) {
                Some(schema) => match op_detail(&schema.description, &d.description, op) {
                    Some(detail) => out.push_str(&format!("- {op} — {detail}\n")),
                    None => out.push_str(&format!("- {op}\n")),
                },
                None => out.push_str(&format!("- {op}\n")),
            }
        }
        out.push('\n');
    }

    out.push_str("## Behavioral Traits\n\n");
    out.push_str(&format!("- Category: {:?}\n", d.category));
    out.push_str(&format!("- Agent type: {}\n\n", d.agent_type));

    out.push_str("## Knowledge Base\n\n");
    for op in &d.operations {
        if let Some(schema) = d.schema_for(op) {
            out.push_str(&format!(
                "- `{}` input: {}\n",
                op,
                compact_json(&schema.input_schema)
            ));
        }
    }
    if d.operations.is_empty() {
        out.push_str("- _None._\n");
    }

    out
}

/// Per-operation schema descriptions are built as
/// `"<agent description> — <detail>"`, so reprinting them whole repeats the
/// agent blurb on every bullet. Keep only the part that distinguishes the
/// operation, and drop it entirely when nothing is left.
fn op_detail(schema_description: &str, agent_description: &str, op: &str) -> Option<String> {
    let trim_noise = |s: &str| s.trim_start_matches([' ', '—', '-']).trim().to_string();

    let mut detail = trim_noise(
        schema_description
            .strip_prefix(agent_description)
            .unwrap_or(schema_description),
    );
    // The remainder usually restates the operation name; the bullet already has it.
    detail = trim_noise(detail.strip_prefix(op).unwrap_or(&detail));

    (!detail.is_empty() && detail != agent_description).then_some(detail)
}

/// Quote a YAML scalar when it contains anything that would change the parse.
fn yaml_scalar(s: &str) -> String {
    let needs_quoting = s.is_empty()
        || s.contains([':', '#', '\n', '"', '\'', '{', '}', '[', ']', ','])
        || s.starts_with(' ')
        || s.ends_with(' ');

    if needs_quoting {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

/// One-line JSON so a schema never breaks the markdown list item.
/// `simd_json`'s owned `Value` renders as compact JSON via `Display`.
fn compact_json(v: &simd_json::OwnedValue) -> String {
    v.to_string()
}

/// Walk up from the manifest dir to the workspace root (the dir holding `.agents`).
fn repo_root() -> Result<PathBuf> {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if dir.join(".agents").exists() {
            return Ok(dir);
        }
        if !dir.pop() {
            anyhow::bail!("could not locate workspace root containing .agents/");
        }
    }
}
