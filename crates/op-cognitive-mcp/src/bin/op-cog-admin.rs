//! Minimal admin CLI for the op-cognitive-mcp cozo store.
//!
//! Examples:
//!   op-cog-admin --db /var/lib/op-dbus/cognitive.db user-add <wg_pubkey>
//!   op-cog-admin --db /var/lib/op-dbus/cognitive.db user-list

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use op_cozo_store::CozoGraphShuttle;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "op-cog-admin", about = "Cozo store admin for op-cognitive-mcp")]
struct Cli {
    #[arg(
        long,
        env = "COGNITIVE_MCP_DB_PATH",
        default_value = "/var/lib/op-dbus/cognitive.db"
    )]
    db: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Insert or refresh a user keyed by wg_pubkey
    UserAdd { wg_pubkey: String },
    /// List all users
    UserList,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let shuttle = CozoGraphShuttle::new_persistent(PathBuf::from(&cli.db))
        .with_context(|| format!("opening cozo at {}", cli.db))?;

    match cli.cmd {
        Cmd::UserAdd { wg_pubkey } => {
            shuttle.upsert_user(&wg_pubkey)?;
            println!("ok: user {} upserted", wg_pubkey);
        }
        Cmd::UserList => {
            let json = shuttle.run_query(
                "?[wg_pubkey, created_at] := *users[wg_pubkey, created_at]",
                None,
            )?;
            println!("{}", serde_json::to_string_pretty(&json)?);
        }
    }
    Ok(())
}
