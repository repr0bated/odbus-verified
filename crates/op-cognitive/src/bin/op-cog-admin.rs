//! Minimal admin CLI for the op-cognitive cozo store.
//!
//! Examples:
//!   op-cog-admin --db /var/lib/op-dbus/cognitive.db user-list
//! Identity mutations use the identity-bound mounted operation surface.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use op_cozo_store::CozoGraphShuttle;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "op-cog-admin", about = "Cozo store admin for op-cognitive")]
struct Cli {
    #[arg(
        long,
        env = "COGNITIVE_DB_PATH",
        default_value = "/var/lib/op-dbus/cognitive.db"
    )]
    db: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Unsupported: use the canonical identity-bound operation surface.
    UserAdd { wg_pubkey: String },
    /// List all users
    UserList,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::UserAdd { wg_pubkey: _ } => {
            anyhow::bail!("direct user-add is unsupported; discover and invoke the authorized identity operation through the identity-bound mounted namespace");
        }
        Cmd::UserList => {
            let shuttle = CozoGraphShuttle::new_persistent(PathBuf::from(&cli.db))
                .with_context(|| format!("opening cozo at {}", cli.db))?;
            let json = shuttle.run_query(
                "?[wg_pubkey, created_at] := *users[wg_pubkey, created_at]",
                None,
            )?;
            println!("{}", serde_json::to_string_pretty(&json)?);
        }
    }
    Ok(())
}
