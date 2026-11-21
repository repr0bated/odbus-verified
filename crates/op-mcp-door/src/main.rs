//! `op-mcp-door` command line.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use op_mcp_door::audit::AuditLog;
use op_mcp_door::catalog::Catalog;
use op_mcp_door::config::Config;
use op_mcp_door::fusecall::{FsOpener, Mount, MountSpec};
use op_mcp_door::grants::{
    ArgsConstraint, EphemeralGrant, GrantStore, MAX_TICKET_LIFETIME, Ticket, TicketStore, now_unix,
};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;

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
    /// Serve /mcp/full and /mcp/loader over TLS (configuration from the environment).
    Serve,
    /// Toolsets file operations.
    Toolsets {
        #[command(subcommand)]
        command: ToolsetsCommand,
    },
    /// Ephemeral grants (root; a human issues these).
    Grant {
        #[command(subcommand)]
        command: GrantCommand,
    },
    /// Per-call elevation tickets (root; a human issues these).
    Ticket {
        #[command(subcommand)]
        command: TicketCommand,
    },
    /// Vector readiness of the hot coding and knowledge tools.
    Vectors {
        #[command(subcommand)]
        command: VectorsCommand,
    },
}

#[derive(Subcommand)]
enum VectorsCommand {
    /// Probe the configured Qdrant collections (read-only GETs; never
    /// creates, writes or re-vectorizes anything). Exit 0 when every gate
    /// is ready, 3 while any is pending re-vectorization.
    Status,
}

#[derive(Subcommand)]
enum ToolsetsCommand {
    /// Generate the toolsets file from a mounted catalog.
    Generate {
        #[arg(long)]
        mount: PathBuf,
        /// Expected mount identity, e.g. '{"kind":"guest_kernel"}'.
        #[arg(long, default_value = r#"{"kind":"guest_kernel"}"#)]
        mode: String,
        /// Output file; stdout when absent.
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum GrantCommand {
    Issue {
        #[arg(long)]
        principal: String,
        /// `<plugin>.<method>` or `toolset:<id>`.
        #[arg(long)]
        tool: String,
        #[arg(long)]
        ttl: i64,
        #[arg(long)]
        max_calls: u64,
        /// Bind the grant to these exact arguments (JSON).
        #[arg(long)]
        exact_args: Option<String>,
        #[arg(long, default_value = "root")]
        issued_by: String,
    },
    List,
    Revoke {
        #[arg(long)]
        id: String,
    },
}

#[derive(Subcommand)]
enum TicketCommand {
    Issue {
        #[arg(long)]
        principal: String,
        /// `<plugin>.<method>`.
        #[arg(long)]
        tool: String,
        /// The exact arguments the ticket allows (JSON object).
        #[arg(long)]
        args_json: String,
        #[arg(long, default_value_t = MAX_TICKET_LIFETIME)]
        ttl: i64,
        #[arg(long, default_value = "root")]
        issued_by: String,
    },
}

/// Operational messages go to stderr as JSON lines; stdout is the audit log.
fn event(kind: &str, detail: Value) {
    eprintln!(
        "{}",
        json!({"door_event": kind, "ts": op_mcp_door::audit::now_rfc3339(), "detail": detail})
    );
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve => serve(),
        Command::Toolsets {
            command: ToolsetsCommand::Generate { mount, mode, out },
        } => {
            let mode: Value = serde_json::from_str(&mode).context("--mode must be JSON")?;
            let mount = Mount::open(&MountSpec { path: mount, mode })
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let catalog = Catalog::scan(mount.root())?;
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
        Command::Grant { command } => grant(command),
        Command::Vectors {
            command: VectorsCommand::Status,
        } => {
            let config = Config::from_env()?;
            let readiness = op_mcp_door::app::build_readiness(&config)?;
            let snapshot = readiness.snapshot();
            println!("{}", serde_json::to_string_pretty(&snapshot)?);
            if snapshot.iter().all(|s| s.is_ready()) {
                Ok(())
            } else {
                std::process::exit(3)
            }
        }
        Command::Ticket {
            command:
                TicketCommand::Issue {
                    principal,
                    tool,
                    args_json,
                    ttl,
                    issued_by,
                },
        } => {
            let config = Config::from_env()?;
            let args: Value =
                serde_json::from_str(&args_json).context("--args-json must be JSON")?;
            if !args.is_object() {
                bail!("--args-json must be a JSON object");
            }
            let now = now_unix();
            let ticket = Ticket {
                ticket_id: format!("t-{}", uuid::Uuid::new_v4().simple()),
                principal_id: principal,
                tool,
                args_sha256: op_mcp_door::canonical::args_sha256(&args),
                issued_at: now,
                expires_at: now + ttl,
                issued_by,
            };
            TicketStore::new(config.tickets_dir()).issue(&ticket)?;
            println!("{}", ticket.ticket_id);
            Ok(())
        }
    }
}

fn grant(command: GrantCommand) -> Result<()> {
    let config = Config::from_env()?;
    let store = GrantStore::new(config.grants_dir());
    match command {
        GrantCommand::Issue {
            principal,
            tool,
            ttl,
            max_calls,
            exact_args,
            issued_by,
        } => {
            let args_constraint = match exact_args {
                Some(text) => {
                    let args: Value =
                        serde_json::from_str(&text).context("--exact-args must be JSON")?;
                    Some(ArgsConstraint {
                        exact_args_sha256: Some(op_mcp_door::canonical::args_sha256(&args)),
                        properties: None,
                    })
                }
                None => None,
            };
            let now = now_unix();
            let grant = EphemeralGrant {
                grant_id: format!("g-{}", uuid::Uuid::new_v4().simple()),
                principal_id: principal,
                tool,
                args_constraint,
                not_before: now,
                expires_at: now + ttl,
                max_calls,
                issued_by,
                approval_ticket_ref: None,
            };
            store.issue(&grant)?;
            println!("{}", grant.grant_id);
        }
        GrantCommand::List => println!("{}", serde_json::to_string_pretty(&store.list())?),
        GrantCommand::Revoke { id } => store.revoke(&id)?,
    }
    Ok(())
}

fn serve() -> Result<()> {
    let config = Config::from_env()?;
    // TLS first: no certificate, no listener (Requirement 1.2).
    let tls = op_mcp_door::server::load_tls(&config.tls_cert, &config.tls_key)?;
    let (door, warnings) =
        op_mcp_door::app::build_door(&config, AuditLog::stdout(), Arc::new(FsOpener))?;
    for warning in warnings {
        event("warning", json!(warning));
    }
    let tools = door.catalog.tools.len();
    let app = op_mcp_door::mcp::router(Arc::new(door));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(config.listen)
            .await
            .with_context(|| format!("bind {}", config.listen))?;
        event(
            "listening",
            json!({"addr": config.listen.to_string(), "tls": true, "tools": tools}),
        );
        let shutdown = async {
            let mut term =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("SIGTERM handler");
            tokio::select! {
                _ = term.recv() => {},
                _ = tokio::signal::ctrl_c() => {},
            }
        };
        op_mcp_door::server::serve(listener, tls, app, shutdown).await
    })
}
