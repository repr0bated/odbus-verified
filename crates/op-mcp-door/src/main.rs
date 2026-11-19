//! `op-mcp-door` command line.

use anyhow::Result;
use clap::{Parser, Subcommand};
use op_mcp_door::catalog::Catalog;
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "op-mcp-door",
    about = "TLS-only MCP endpoint on the door; FUSE call files behind it"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Toolsets file operations.
    Toolsets {
        #[command(subcommand)]
        command: ToolsetsCommand,
    },
}

#[derive(Subcommand)]
enum ToolsetsCommand {
    /// Generate the toolsets file from a mounted catalog.
    Generate {
        #[arg(long)]
        mount: PathBuf,
        /// Output file; stdout when absent.
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

/// Operational messages go to stderr as JSON lines; stdout is the audit log.
fn event(kind: &str, detail: Value) {
    eprintln!(
        "{}",
        json!({"door_event": kind, "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true), "detail": detail})
    );
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Toolsets {
            command: ToolsetsCommand::Generate { mount, out },
        } => {
            let catalog = Catalog::scan(&mount)?;
            let file = op_mcp_door::toolsets::generate(&catalog);
            let mut classes = std::collections::BTreeMap::<&str, usize>::new();
            for tool in &catalog.tools {
                *classes.entry(tool.class.as_str()).or_default() += 1;
            }
            event(
                "catalog",
                json!({"tools": catalog.tools.len(), "skipped": catalog.skipped.len(), "skipped_reasons": catalog.skipped,
                "classes": classes, "toolsets": file.toolsets.len(), "missing": file.missing}),
            );
            match out {
                Some(path) => op_mcp_door::toolsets::write_file(&path, &file)?,
                None => println!("{}", serde_json::to_string_pretty(&file)?),
            }
            Ok(())
        }
    }
}
