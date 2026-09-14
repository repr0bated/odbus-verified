//! RAG ingest CLI — embed repomix zip content into Qdrant via Voyage.
//!
//! Usage:
//!   rag-ingest --zip ~/repomix.zip --repo rust-analyzer
//!   rag-ingest --zip ~/repomix.zip --all
//!   rag-ingest --zip ~/repomix.zip --list

use anyhow::{Context, Result};
use clap::Parser;
use op_cognitive_mcp::rag_pipeline::default_collection_from_env;
use op_cognitive_mcp::rag_pipeline::RagPipeline;
use std::path::PathBuf;
use tracing::{error, info};

// Voyage embedding pricing estimate: per million tokens, charged after free tier
// exhausted. Rough figure for budgeting; adjust to the current voyage-4 tier.
const VOYAGE_COST_PER_MILLION: f64 = 0.18;
// Free tier per model per month (tokens) — overage is billed at VOYAGE_COST_PER_MILLION
const VOYAGE_FREE_TIER_TOKENS: usize = 200_000_000;
// Rough average tokens per chunk (embed_text header + ~300 content tokens)
const AVG_TOKENS_PER_CHUNK: usize = 400;
// Default hard cap: stop before $10 of paid overage (55M tokens beyond free tier)
const DEFAULT_MAX_TOKENS: usize = VOYAGE_FREE_TIER_TOKENS + 55_000_000; // free + ~$10

#[derive(Parser)]
#[command(name = "rag-ingest")]
#[command(about = "Ingest repomix content into Qdrant with Voyage embeddings")]
struct Cli {
    /// Path to the repomix zip file
    #[arg(long, default_value = "~/repomix.zip")]
    zip: PathBuf,

    /// Repo to ingest (e.g. "rust-analyzer"). Can be specified multiple times.
    #[arg(long)]
    repo: Vec<String>,

    /// Ingest all repos in the zip (slow — hundreds of MB)
    #[arg(long)]
    all: bool,

    /// List available repos in the zip and exit
    #[arg(long)]
    list: bool,

    /// Qdrant collection name
    #[arg(long, env = "COGNITIVE_MCP_RAG_COLLECTION")]
    collection: Option<String>,

    /// Skip repos whose names contain this substring
    #[arg(long)]
    skip: Vec<String>,

    /// Estimate cost and chunk count, then exit without embedding
    #[arg(long)]
    dry_run: bool,

    /// Maximum tokens to embed across the entire run (budget guard).
    /// Ingest aborts once this limit is reached. Default: 27M (~$4.86).
    #[arg(long, default_value_t = DEFAULT_MAX_TOKENS)]
    max_tokens: usize,

    /// Skip the cost confirmation prompt (use in CI or when you know the cost)
    #[arg(long)]
    yes: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
        .init();

    let mut cli = Cli::parse();
    let collection = cli
        .collection
        .clone()
        .unwrap_or_else(default_collection_from_env);

    // Expand ~ in zip path
    if let Some(home) = std::env::var_os("HOME") {
        let zip_str = cli.zip.to_string_lossy();
        if zip_str.starts_with("~/") {
            cli.zip = PathBuf::from(home).join(zip_str.trim_start_matches("~/"));
        }
    }

    let zip_path = cli
        .zip
        .canonicalize()
        .with_context(|| format!("zip not found: {}", cli.zip.display()))?;

    // List available entries
    let entries = list_repomix_entries(&zip_path)?;

    if cli.list {
        println!("Repomix entries in {}:", zip_path.display());
        for (entry, repo) in &entries {
            println!("  {repo:40} ({entry})");
        }
        return Ok(());
    }

    // Determine what to ingest
    let targets: Vec<(String, String)> = if cli.all {
        entries.clone()
    } else if !cli.repo.is_empty() {
        // Match by repo name substring
        entries
            .into_iter()
            .filter(|(_, repo)| cli.repo.iter().any(|r| repo.contains(r.as_str())))
            .collect()
    } else {
        anyhow::bail!("Specify --repo <name>, --all, or --list");
    };

    // Apply skip filter
    let targets: Vec<_> = targets
        .into_iter()
        .filter(|(_, repo)| !cli.skip.iter().any(|s| repo.contains(s.as_str())))
        .collect();

    if targets.is_empty() {
        anyhow::bail!("No matching entries found. Use --list to see available repos.");
    }

    // ── Cost estimate ────────────────────────────────────────────────────────
    // Count chunks per target by reading zip entry sizes (fast, no embedding).
    let mut estimated_chunks = 0usize;
    {
        let file = std::fs::File::open(&zip_path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        for (entry_name, _repo) in &targets {
            if let Ok(entry) = archive.by_name(entry_name) {
                // ~1 chunk per 1.5 KB of source (empirical from past runs)
                estimated_chunks += (entry.size() as usize / 1500).max(1);
            }
        }
    }
    let estimated_tokens = estimated_chunks * AVG_TOKENS_PER_CHUNK;
    let estimated_cost = estimated_tokens as f64 / 1_000_000.0 * VOYAGE_COST_PER_MILLION;

    println!("\n=== Cost estimate ===");
    println!("  Repos          : {}", targets.len());
    println!("  Est. chunks    : ~{estimated_chunks}");
    println!("  Est. tokens    : ~{}M", estimated_tokens / 1_000_000);
    println!(
        "  Est. cost      : ~${estimated_cost:.2}  (Voyage @ ${VOYAGE_COST_PER_MILLION}/M tokens)"
    );
    println!(
        "  Token cap      : {}M  (--max-tokens)",
        cli.max_tokens / 1_000_000
    );

    if cli.dry_run {
        println!("\n[dry-run] Exiting without embedding.");
        return Ok(());
    }

    if estimated_tokens > cli.max_tokens {
        anyhow::bail!(
            "Estimated tokens ({estimated_tokens}) exceed --max-tokens cap ({}). \
             Reduce scope with --repo, raise --max-tokens, or use --dry-run to preview.",
            cli.max_tokens
        );
    }

    if !cli.yes {
        print!("\nProceed with ingest? [y/N] ");
        std::io::Write::flush(&mut std::io::stdout())?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if !answer.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    // ── Ingest ───────────────────────────────────────────────────────────────
    info!(
        count = targets.len(),
        collection = %collection,
        "Starting ingest"
    );

    let pipeline =
        RagPipeline::from_env().context("Failed to init pipeline — is VOYAGE_API_KEY set?")?;

    let mut total_files = 0usize;
    let mut total_chunks = 0usize;
    let mut total_errors = 0usize;
    let mut total_tokens = 0usize;

    'outer: for (entry_name, repo) in &targets {
        info!(repo = %repo, "Processing");

        match pipeline
            .ingest_repomix_entry(&zip_path, entry_name, &collection)
            .await
        {
            Ok(stats) => {
                info!(
                    repo = %repo,
                    files = stats.files_parsed,
                    chunks = stats.chunks_upserted,
                    errors = stats.errors,
                    "Done"
                );
                total_files += stats.files_parsed;
                total_chunks += stats.chunks_upserted;
                total_errors += stats.errors;
                total_tokens += stats.chunks_upserted * AVG_TOKENS_PER_CHUNK;

                if total_tokens >= cli.max_tokens {
                    println!(
                        "\n⚠  Token cap reached ({} / {} tokens). Stopping early.",
                        total_tokens, cli.max_tokens
                    );
                    break 'outer;
                }
            }
            Err(e) => {
                error!(repo = %repo, error = %e, "Ingest failed");
                total_errors += 1;
            }
        }
    }

    let actual_cost = total_tokens as f64 / 1_000_000.0 * VOYAGE_COST_PER_MILLION;
    println!("\n=== Ingest summary ===");
    println!("  Repos processed : {}", targets.len());
    println!("  Source files    : {total_files}");
    println!("  Chunks upserted : {total_chunks}");
    println!("  Errors          : {total_errors}");
    println!("  Est. tokens used: ~{}M", total_tokens / 1_000_000);
    println!("  Est. cost       : ~${actual_cost:.2}");
    println!("  Collection      : {collection}");

    Ok(())
}

fn list_repomix_entries(zip_path: &std::path::Path) -> Result<Vec<(String, String)>> {
    // Open archive to list entries
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    let mut entries: Vec<(String, String)> = (0..archive.len())
        .filter_map(|i| {
            let entry = archive.by_index(i).ok()?;
            let name = entry.name().to_string();
            if name.ends_with(".md") || name.ends_with(".xml") {
                let repo = repo_name_from_entry(&name);
                Some((name, repo))
            } else {
                None
            }
        })
        .collect();

    entries.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(entries)
}

fn repo_name_from_entry(entry_name: &str) -> String {
    let base = entry_name.trim_end_matches(".md").trim_end_matches(".xml");
    let base = if let Some(pos) = base.rfind("-repomix") {
        &base[..pos]
    } else {
        base
    };
    base.to_string()
}
