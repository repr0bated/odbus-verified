mod dispatch;
mod fuse_fs;
mod model;
mod namespace;
mod source;
mod store;

use std::fs;
use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use fuser::MountOption;
use serde::Serialize;
use serde_json::Value;

use crate::dispatch::DEFAULT_BUS_ADDRESS;
use crate::fuse_fs::{Lungs, TypedFuse};
use crate::namespace::{Namespace, NodeKind, ProjectionTarget};
use crate::source::{DEFAULT_CATALOG_DIR, DEFAULT_STATE_DIR, SourceCatalog};
use crate::store::{DEFAULT_STORE_DIR, ObjectStore};

#[derive(Debug, Parser)]
#[command(
    name = "3tchedfs",
    version,
    about = "Typed blob overlay filesystem for OP-DBUS"
)]
struct Cli {
    /// Immutable object/view store. Production defaults to tmpfs.
    #[arg(long, global = true, default_value = DEFAULT_STORE_DIR)]
    store: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Capture one coherent typed view from the live SHM source.
    Capture {
        #[arg(long, default_value = DEFAULT_CATALOG_DIR)]
        catalog: PathBuf,
        #[arg(long, default_value = DEFAULT_STATE_DIR)]
        state: PathBuf,
        /// Write the view without advancing the current pointer.
        #[arg(long)]
        no_activate: bool,
    },
    /// Inspect an immutable view, workspace, or plugin object.
    Inspect {
        #[command(flatten)]
        target: TargetArgs,
        #[arg(long)]
        plugin: Option<String>,
    },
    /// List immutable content-addressed views.
    Views,
    /// Print the deterministic projected namespace without mounting.
    Tree {
        #[command(flatten)]
        target: TargetArgs,
    },
    /// Manage sparse copy-on-write workspaces.
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommand,
    },
    /// Manage immutable named view references.
    Snapshot {
        #[command(subcommand)]
        command: SnapshotCommand,
    },
    /// Verify all content hashes and typed objects in a view.
    Verify {
        #[command(flatten)]
        target: TargetArgs,
    },
    /// Dispatch a schema-declared operation through PluginV1.Call.
    Call {
        plugin: String,
        method: String,
        #[command(flatten)]
        target: TargetArgs,
        /// JSON argument object, or '-' to read stdin.
        #[arg(long, conflicts_with = "args_file", default_value = "{}")]
        args: String,
        /// Read the JSON argument object from a file.
        #[arg(long)]
        args_file: Option<PathBuf>,
        #[arg(long, default_value = DEFAULT_BUS_ADDRESS)]
        bus_address: String,
        /// Required for mutation or external operations.
        #[arg(long)]
        confirm_side_effects: bool,
    },
    /// Mount a pinned immutable view or writable COW workspace through FUSE.
    Mount {
        mountpoint: PathBuf,
        #[command(flatten)]
        target: TargetArgs,
        #[arg(long, default_value = DEFAULT_STATE_DIR)]
        state: PathBuf,
        #[arg(long)]
        allow_other: bool,
        /// Ask libfuse to unmount on exit (may require user_allow_other).
        #[arg(long)]
        auto_unmount: bool,
        /// Mount as one identity's lungs: every method gets a `call` file,
        /// and a write is dispatched as that identity using the SID1 at this
        /// path on its own device.
        #[arg(long, conflicts_with = "workspace")]
        lungs_credential: Option<PathBuf>,
        #[arg(long, default_value = DEFAULT_BUS_ADDRESS)]
        bus_address: String,
    },
}

#[derive(Debug, Args, Clone)]
struct TargetArgs {
    /// Pin an immutable view by content hash.
    #[arg(long, conflicts_with = "workspace")]
    view: Option<String>,
    /// Project a writable copy-on-write workspace.
    #[arg(long, conflicts_with = "view")]
    workspace: Option<String>,
}

#[derive(Debug, Subcommand)]
enum WorkspaceCommand {
    New {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        base_view: Option<String>,
    },
    Show {
        name: String,
    },
    List,
    Stage {
        name: String,
        plugin: String,
        /// JSON pointer within the plugin value. Empty means the full value.
        #[arg(long, default_value = "")]
        pointer: String,
        #[arg(long, conflicts_with = "file")]
        json: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
    },
    Commit {
        name: String,
        /// Create the immutable view without advancing current.
        #[arg(long)]
        no_activate: bool,
    },
    Abort {
        name: String,
    },
}

#[derive(Debug, Subcommand)]
enum SnapshotCommand {
    Create {
        name: String,
        #[arg(long)]
        view: Option<String>,
    },
    Show {
        name: String,
    },
    List,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let store = ObjectStore::open(&cli.store)?;

    match cli.command {
        Command::Capture {
            catalog,
            state,
            no_activate,
        } => {
            let source = SourceCatalog::new(catalog, state);
            print_json(&store.capture(&source, !no_activate)?)?;
        }
        Command::Inspect { target, plugin } => {
            let target = resolve_target(&store, &target)?;
            match (&target, plugin) {
                (ProjectionTarget::View(id), Some(plugin)) => {
                    let view = store.read_view(id)?;
                    print_json(&store.object_for_view(&view, &plugin)?)?;
                }
                (ProjectionTarget::Workspace(id), Some(plugin)) => {
                    let workspace = store.read_workspace(id)?;
                    print_json(&store.workspace_object(&workspace, &plugin)?)?;
                }
                (ProjectionTarget::View(id), None) => print_json(&store.read_view(id)?)?,
                (ProjectionTarget::Workspace(id), None) => print_json(&store.read_workspace(id)?)?,
            }
        }
        Command::Views => print_json(&store.list_views()?)?,
        Command::Tree { target } => {
            let target = resolve_target(&store, &target)?;
            let namespace = Namespace::build(&store, target, None)?;
            let mut nodes: Vec<_> = namespace.nodes.values().collect();
            nodes.sort_by(|left, right| left.path.cmp(&right.path));
            for node in nodes {
                let marker = match node.kind {
                    NodeKind::Directory => "d",
                    NodeKind::File(_) if node.write_target.is_some() => "w",
                    NodeKind::File(_) | NodeKind::LiveFile { .. } | NodeKind::Outcome { .. } => "f",
                    NodeKind::CallFile { .. } => "c",
                    NodeKind::Symlink(_) => "l",
                };
                println!("{marker} {}", node.path);
            }
        }
        Command::Workspace { command } => match command {
            WorkspaceCommand::New { name, base_view } => {
                print_json(&store.create_workspace(name.as_deref(), base_view.as_deref())?)?;
            }
            WorkspaceCommand::Show { name } => print_json(&store.read_workspace(&name)?)?,
            WorkspaceCommand::List => print_json(&store.list_workspaces()?)?,
            WorkspaceCommand::Stage {
                name,
                plugin,
                pointer,
                json,
                file,
            } => {
                let value = input_json(json.as_deref(), file.as_ref())?;
                let hash = store.stage_pointer(&name, &plugin, &pointer, value)?;
                print_json(&serde_json::json!({
                    "workspace": name,
                    "plugin": plugin,
                    "pointer": pointer,
                    "object_hash": hash,
                }))?;
            }
            WorkspaceCommand::Commit { name, no_activate } => {
                print_json(&store.commit_workspace(&name, !no_activate)?)?;
            }
            WorkspaceCommand::Abort { name } => {
                store.abort_workspace(&name)?;
                print_json(&serde_json::json!({"aborted": name}))?;
            }
        },
        Command::Snapshot { command } => match command {
            SnapshotCommand::Create { name, view } => {
                print_json(&store.snapshot(&name, view.as_deref())?)?;
            }
            SnapshotCommand::Show { name } => print_json(&store.read_snapshot(&name)?)?,
            SnapshotCommand::List => print_json(&store.list_snapshots()?)?,
        },
        Command::Verify { target } => {
            let target = resolve_target(&store, &target)?;
            let (view_id, objects) = match target {
                ProjectionTarget::View(id) => {
                    let view = store.read_view(&id)?;
                    (view.view_id, view.objects)
                }
                ProjectionTarget::Workspace(id) => {
                    let workspace = store.read_workspace(&id)?;
                    let view = store.read_view(&workspace.base_view)?;
                    let mut objects = view.objects;
                    objects.extend(workspace.staged_objects);
                    (workspace.base_view, objects)
                }
            };
            for hash in objects.values() {
                store.read_object(hash)?;
            }
            print_json(&serde_json::json!({
                "valid": true,
                "view": view_id,
                "objects_verified": objects.len(),
            }))?;
        }
        Command::Call {
            plugin,
            method,
            target,
            args,
            args_file,
            bus_address,
            confirm_side_effects,
        } => {
            let target = resolve_target(&store, &target)?;
            let arguments = input_json(Some(&args), args_file.as_ref())?;
            let receipt = dispatch::call(
                &store,
                &target,
                &bus_address,
                &plugin,
                &method,
                arguments,
                confirm_side_effects,
            )
            .await?;
            print_json(&receipt)?;
        }
        Command::Mount {
            mountpoint,
            target,
            state,
            allow_other,
            auto_unmount,
            lungs_credential,
            bus_address,
        } => {
            if !mountpoint.is_dir() {
                bail!("mountpoint {} is not a directory", mountpoint.display());
            }
            let target = resolve_target(&store, &target)?;
            let lungs = lungs_credential.is_some();
            // Lungs call files need a writable mount; every other node still
            // refuses writes on its own.
            let writable = lungs || matches!(target, ProjectionTarget::Workspace(_));
            // View mounts read live state from SHM; workspace mounts use
            // frozen COW objects and don't need the state directory.
            let filesystem = match lungs_credential {
                Some(credential) => TypedFuse::lungs(
                    store,
                    target,
                    state,
                    Lungs {
                        bus_address,
                        credential,
                    },
                )?,
                None if matches!(target, ProjectionTarget::Workspace(_)) => {
                    TypedFuse::new(store, target)?
                }
                None => TypedFuse::with_state_dir(store, target, state)?,
            };
            let mut options = vec![
                MountOption::FSName("3tchedFS".to_string()),
                MountOption::Subtype("3tchedFS".to_string()),
                MountOption::DefaultPermissions,
                MountOption::NoDev,
                MountOption::NoSuid,
                MountOption::NoExec,
                MountOption::NoAtime,
                if writable {
                    MountOption::RW
                } else {
                    MountOption::RO
                },
            ];
            if allow_other {
                options.push(MountOption::AllowOther);
            }
            if auto_unmount {
                options.push(MountOption::AutoUnmount);
            }
            fuser::mount2(filesystem, &mountpoint, &options)
                .with_context(|| format!("mount 3tchedFS at {}", mountpoint.display()))?;
        }
    }
    Ok(())
}

fn resolve_target(store: &ObjectStore, args: &TargetArgs) -> Result<ProjectionTarget> {
    match (&args.view, &args.workspace) {
        (Some(view), None) => {
            store.read_view(view)?;
            Ok(ProjectionTarget::View(view.clone()))
        }
        (None, Some(workspace)) => {
            store.read_workspace(workspace)?;
            Ok(ProjectionTarget::Workspace(workspace.clone()))
        }
        (None, None) => {
            Ok(ProjectionTarget::View(store.current_view_id()?.context(
                "no current view; run `3tchedfs capture` first",
            )?))
        }
        (Some(_), Some(_)) => unreachable!("clap rejects conflicting targets"),
    }
}

fn input_json(inline: Option<&str>, file: Option<&PathBuf>) -> Result<Value> {
    let text = match (inline, file) {
        (_, Some(path)) => fs::read_to_string(path)
            .with_context(|| format!("read JSON input {}", path.display()))?,
        (Some("-"), None) => {
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text)?;
            text
        }
        (Some(text), None) => text.to_string(),
        (None, None) => bail!("provide --json or --file"),
    };
    serde_json::from_str(&text).context("parse JSON input")
}

fn print_json(value: &impl Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
