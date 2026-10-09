mod dispatch;
mod fuse_fs;
mod mcp;
mod model;
mod mounted;
mod namespace;
mod source;
mod store;
mod zeroclaw_surface;

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
        /// Capture only sealed declarations. State is explicitly unavailable,
        /// allowing boot before the authoritative state owner is ready.
        #[arg(long)]
        schema_only: bool,
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
        /// Use an existing identity-bound FUSE namespace, without a direct transport.
        #[arg(long, conflicts_with_all = ["view", "workspace", "bus_address", "store"])]
        mount: Option<PathBuf>,
        #[arg(long, requires = "mount")]
        expected_session: Option<String>,
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
    /// Invoke one canonical mount-relative operation path through FUSE.
    Invoke {
        /// plugins/<encoded-plugin>/operations/<encoded-method>/call
        path: String,
        #[arg(long, conflicts_with = "store")]
        mount: PathBuf,
        #[arg(long)]
        expected_session: Option<String>,
        #[arg(long, conflicts_with = "args_file", default_value = "{}")]
        args: String,
        #[arg(long)]
        args_file: Option<PathBuf>,
        #[arg(long)]
        confirm_side_effects: bool,
    },
    /// Discover operation declarations from an identity-bound mounted namespace.
    Describe {
        #[arg(long, conflicts_with = "store")]
        mount: PathBuf,
        #[arg(long)]
        expected_session: String,
        #[arg(long)]
        plugin: Option<String>,
    },
    /// Mount a pinned immutable view or writable COW workspace through FUSE.
    Mount {
        mountpoint: PathBuf,
        #[command(flatten)]
        target: TargetArgs,
        #[arg(long, default_value = DEFAULT_STATE_DIR)]
        state: PathBuf,
        /// Active sealed catalog whose policy must still match the pinned view.
        #[arg(long, default_value = DEFAULT_CATALOG_DIR)]
        catalog: PathBuf,
        #[arg(long)]
        allow_other: bool,
        /// Exact local UID allowed to write call/workspace files. Only a root
        /// mount owner may delegate to another UID; backend identity is unchanged.
        #[arg(long)]
        call_owner_uid: Option<u32>,
        /// Ask libfuse to unmount on exit (may require user_allow_other).
        #[arg(long)]
        auto_unmount: bool,
        /// Mount as one identity's lungs: every method gets a `call` file,
        /// and a write is dispatched as that identity using the SID1 at this
        /// path on its own device.
        #[arg(long, conflicts_with_all = ["workspace", "lungs_socket"])]
        lungs_credential: Option<PathBuf>,
        /// Mount as one identity's lungs, dispatching every `call` write over
        /// the bridge's shared lungs socket. Without --lungs-session the
        /// bridge derives the current container identity from its kernel peer.
        #[arg(long, conflicts_with = "workspace")]
        lungs_socket: Option<PathBuf>,
        /// Host root only: the session these lungs breathe for.
        #[arg(long, requires = "lungs_socket")]
        lungs_session: Option<String>,
        #[arg(long, default_value = DEFAULT_BUS_ADDRESS)]
        bus_address: String,
    },
    /// Serve this identity's MCP on stdio: sealed methods as tools, every
    /// plugin's schema and state as resources, each call dispatched as the
    /// identity. `--mount` reads and calls only through that FUSE namespace.
    /// Launch it from an IDE's MCP server configuration.
    Mcp {
        #[command(flatten)]
        target: TargetArgs,
        #[command(flatten)]
        lungs: LungsArgs,
        /// Identity-bound FUSE mount. Tools, resources and calls use this
        /// namespace and do not open the object store, SHM state or lungs socket.
        #[arg(
            long,
            conflicts_with_all = [
                "view",
                "workspace",
                "lungs_socket",
                "lungs_credential",
                "state",
                "catalog",
                "store"
            ]
        )]
        mount: Option<PathBuf>,
        #[arg(long, requires = "mount")]
        expected_session: Option<String>,
        #[arg(long, default_value = DEFAULT_STATE_DIR)]
        state: PathBuf,
        #[arg(long, default_value = DEFAULT_CATALOG_DIR)]
        catalog: PathBuf,
        /// List every method as its own tool instead of toolsets.
        #[arg(long, conflicts_with = "compact")]
        all: bool,
        /// Four meta-tools (list, search, schema, execute) for small-context
        /// clients.
        #[arg(long)]
        compact: bool,
        /// Toolsets (plugin ids) loaded when the connection opens.
        #[arg(long = "load", value_name = "TOOLSET")]
        load: Vec<String>,
        /// Publish the toolset manifest to EMQX on start, as this identity.
        #[arg(long)]
        announce_toolsets: bool,
    },
    /// Print the toolset manifest derived from the sealed catalog, and with
    /// --publish feed it to EMQX through `emqx.publish_toolsets`.
    Toolsets {
        #[command(flatten)]
        target: TargetArgs,
        #[command(flatten)]
        lungs: LungsArgs,
        #[arg(long)]
        publish: bool,
    },
}

/// An identity's lungs: the shared socket authenticates the current container
/// unless host root explicitly supplies a session.
#[derive(Debug, Args, Clone)]
struct LungsArgs {
    #[arg(long, conflicts_with = "lungs_credential")]
    lungs_socket: Option<PathBuf>,
    /// Host root only: the session this identity breathes for.
    #[arg(long, requires = "lungs_socket")]
    lungs_session: Option<String>,
    /// The identity's SID1 on its own device, registered per call.
    #[arg(long)]
    lungs_credential: Option<PathBuf>,
    #[arg(long, default_value = DEFAULT_BUS_ADDRESS)]
    bus_address: String,
}

impl LungsArgs {
    fn lungs(self) -> Option<Lungs> {
        match (self.lungs_socket, self.lungs_credential) {
            (Some(path), _) => Some(Lungs::Socket {
                path,
                session: self.lungs_session.unwrap_or_default(),
            }),
            (None, Some(credential)) => Some(Lungs::Credential {
                bus_address: self.bus_address,
                credential,
            }),
            (None, None) => None,
        }
    }
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
    // Mounted mode must never open/create an ObjectStore or resolve SHM state.
    if let Command::Describe {
        mount,
        expected_session,
        plugin,
    } = &cli.command
    {
        let namespace = mounted::MountedNamespace::open(mount, Some(expected_session))?;
        return print_json(&namespace.describe(plugin.as_deref())?);
    }
    if let Command::Call {
        mount: Some(path),
        expected_session,
        plugin,
        method,
        args,
        args_file,
        confirm_side_effects,
        ..
    } = &cli.command
    {
        let namespace = mounted::MountedNamespace::open(path, expected_session.as_deref())?;
        let decl = namespace.method_decl(plugin, method)?;
        if decl.side_effect != model::SideEffect::Read && !confirm_side_effects {
            bail!("operation requires --confirm-side-effects");
        }
        let arguments = input_json(Some(args), args_file.as_ref())?;
        return print_json(&namespace.call(plugin, method, &decl, arguments)?);
    }
    if let Command::Invoke {
        path,
        mount,
        expected_session,
        args,
        args_file,
        confirm_side_effects,
    } = &cli.command
    {
        let namespace = mounted::MountedNamespace::open(mount, expected_session.as_deref())?;
        let arguments = input_json(Some(args), args_file.as_ref())?;
        return print_json(&namespace.invoke(path, arguments, *confirm_side_effects)?);
    }
    if let Command::Mcp {
        mount: Some(mount),
        expected_session,
        all,
        compact,
        load,
        announce_toolsets,
        ..
    } = &cli.command
    {
        let namespace = mounted::MountedNamespace::open(mount, expected_session.as_deref())?;
        let surface = match (*all, *compact) {
            (true, _) => mcp::Surface::Full,
            (_, true) => mcp::Surface::Compact,
            _ => mcp::Surface::Toolsets,
        };
        let server = mcp::IdentityMcp::from_mount(namespace, surface)?;
        server.preload(load)?;
        if *announce_toolsets {
            match server.announce_toolsets().await {
                Ok(receipt) => eprintln!("3tchedfs: toolsets announced: {receipt}"),
                Err(error) => eprintln!("3tchedfs: toolsets not announced: {error:#}"),
            }
        }
        return server.serve_stdio().await;
    }
    let store = ObjectStore::open(&cli.store)?;

    match cli.command {
        Command::Invoke { .. } | Command::Describe { .. } | Command::Mcp { mount: Some(_), .. } => {
            unreachable!("mounted invocation returned before opening an ObjectStore")
        }
        Command::Capture {
            catalog,
            state,
            schema_only,
            no_activate,
        } => {
            let source = SourceCatalog::new(catalog, state);
            let snapshot = if schema_only {
                source.capture_schema_only()?
            } else {
                source.capture()?
            };
            print_json(&store.capture_snapshot(snapshot, !no_activate)?)?;
        }
        Command::Inspect { target, plugin } => {
            let target = resolve_target(&store, &target)?;
            match (&target, plugin) {
                (ProjectionTarget::View(id), Some(plugin)) => {
                    let view = store.read_view(id)?;
                    print_json(&store.object_for_view(&view, &plugin)?.public_projection()?)?;
                }
                (ProjectionTarget::Workspace(id), Some(plugin)) => {
                    let workspace = store.read_workspace(id)?;
                    print_json(
                        &store
                            .workspace_object(&workspace, &plugin)?
                            .public_projection()?,
                    )?;
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
                    NodeKind::File(_) | NodeKind::LiveFile { .. } => "f",
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
            mount: _,
            expected_session: _,
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
        Command::Mcp {
            target,
            lungs,
            mount: None,
            state,
            catalog,
            all,
            compact,
            load,
            announce_toolsets,
            ..
        } => {
            let target = resolve_target(&store, &target)?;
            let lungs = lungs
                .lungs()
                .context("the MCP server needs this identity's lungs")?;
            let surface = match (all, compact) {
                (true, _) => mcp::Surface::Full,
                (_, true) => mcp::Surface::Compact,
                _ => mcp::Surface::Toolsets,
            };
            let server = mcp::IdentityMcp::new(store, target, state, lungs, surface)?
                .with_catalog_dir(catalog);
            server.preload(&load)?;
            if announce_toolsets {
                // stdout is the MCP stream; report on stderr and keep serving.
                match server.announce_toolsets().await {
                    Ok(receipt) => eprintln!("3tchedfs: toolsets announced: {receipt}"),
                    Err(error) => eprintln!("3tchedfs: toolsets not announced: {error:#}"),
                }
            }
            server.serve_stdio().await?;
        }
        Command::Toolsets {
            target,
            lungs,
            publish,
        } => {
            let target = resolve_target(&store, &target)?;
            let lungs = lungs.lungs();
            if publish && lungs.is_none() {
                bail!("--publish needs this identity's lungs");
            }
            // Without lungs the manifest is only printed; nothing dispatches.
            let lungs = lungs.unwrap_or(Lungs::Socket {
                path: PathBuf::new(),
                session: String::new(),
            });
            let server = mcp::IdentityMcp::new(
                store,
                target,
                PathBuf::from(DEFAULT_STATE_DIR),
                lungs,
                mcp::Surface::Toolsets,
            )?;
            if publish {
                print_json(&server.announce_toolsets().await?)?;
            } else {
                print_json(&server.toolsets())?;
            }
        }
        Command::Mount {
            mountpoint,
            target,
            state,
            catalog,
            allow_other,
            call_owner_uid,
            auto_unmount,
            lungs_credential,
            lungs_socket,
            lungs_session,
            bus_address,
        } => {
            if !mountpoint.is_dir() {
                bail!("mountpoint {} is not a directory", mountpoint.display());
            }
            let target = resolve_target(&store, &target)?;
            let lungs = match (lungs_socket, lungs_credential) {
                (Some(path), _) => Some(Lungs::Socket {
                    path,
                    session: lungs_session.unwrap_or_default(),
                }),
                (None, Some(credential)) => Some(Lungs::Credential {
                    bus_address,
                    credential,
                }),
                (None, None) => None,
            };
            // Lungs call files need a writable mount; every other node still
            // refuses writes on its own.
            let writable = lungs.is_some() || matches!(target, ProjectionTarget::Workspace(_));
            // View mounts read live state from SHM; workspace mounts use
            // frozen COW objects and don't need the state directory.
            let mut filesystem = match lungs {
                Some(lungs) => TypedFuse::lungs(store, target, state, lungs)?,
                None if matches!(target, ProjectionTarget::Workspace(_)) => {
                    TypedFuse::new(store, target)?
                }
                None => TypedFuse::with_state_dir(store, target, state)?,
            };
            if let Some(uid) = call_owner_uid {
                filesystem = filesystem.with_call_owner_uid(uid)?;
            }
            filesystem = filesystem.with_catalog_dir(catalog)?;
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
    const MAX_INPUT: usize = 1024 * 1024;
    fn bounded_text(reader: impl std::io::Read) -> Result<String> {
        let mut text = String::new();
        reader
            .take((MAX_INPUT + 1) as u64)
            .read_to_string(&mut text)?;
        anyhow::ensure!(text.len() <= MAX_INPUT, "JSON input exceeds 1 MiB");
        Ok(text)
    }
    let text = match (inline, file) {
        (_, Some(path)) => bounded_text(
            fs::File::open(path).with_context(|| format!("read JSON input {}", path.display()))?,
        )?,
        (Some("-"), None) => bounded_text(std::io::stdin().lock())?,
        (Some(text), None) => {
            anyhow::ensure!(text.len() <= MAX_INPUT, "JSON input exceeds 1 MiB");
            text.to_string()
        }
        (None, None) => bail!("provide --json or --file"),
    };
    serde_json::from_str(&text).context("parse JSON input")
}

fn print_json(value: &impl Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn mounted_commands_preserve_identity_and_reject_transport_fallback() {
        let args = vec![
            "threetched-fs",
            "call",
            "demo",
            "get_status",
            "--mount",
            "/run/mount/3tchedFS",
            "--expected-session",
            "human",
        ];
        assert!(Cli::try_parse_from(&args).is_ok());
        for fallback in [
            vec!["--view", "other"],
            vec!["--workspace", "other"],
            vec!["--bus-address", "unix:path=/alternate"],
            vec!["--store", "/alternate"],
        ] {
            let mut conflict = args.clone();
            conflict.extend(fallback);
            assert!(Cli::try_parse_from(conflict).is_err());
        }
        assert!(
            Cli::try_parse_from(["threetched-fs", "mcp", "--expected-session", "human"]).is_err()
        );
        assert!(matches!(
            Cli::try_parse_from([
                "threetched-fs",
                "mcp",
                "--mount",
                "/run/mount/3tchedFS",
                "--expected-session",
                "human",
            ])
            .unwrap()
            .command,
            Command::Mcp { mount: Some(_), .. }
        ));
        assert!(
            Cli::try_parse_from([
                "threetched-fs",
                "mcp",
                "--mount",
                "/run/mount/3tchedFS",
                "--lungs-socket",
                "/run/opdbus/lungs/lungs.sock",
            ])
            .is_err()
        );
    }

    #[test]
    fn invoke_requires_mount_and_has_no_direct_transport_selectors() {
        let args = [
            "threetched-fs",
            "invoke",
            "plugins/demo/operations/get_status/call",
            "--mount",
            "/run/mount/3tchedFS",
            "--expected-session",
            "human",
        ];
        assert!(matches!(
            Cli::try_parse_from(args).unwrap().command,
            Command::Invoke { .. }
        ));
        assert!(
            Cli::try_parse_from([
                "threetched-fs",
                "invoke",
                "plugins/demo/operations/get_status/call"
            ])
            .is_err()
        );
        for alternate in [
            "--view",
            "--workspace",
            "--store",
            "--bus-address",
            "--lungs-socket",
        ] {
            let mut conflict = args.to_vec();
            conflict.extend([alternate, "alternate"]);
            assert!(Cli::try_parse_from(conflict).is_err());
        }
    }

    #[test]
    fn guest_mcp_socket_requires_no_session_input() {
        let cli = Cli::try_parse_from([
            "threetched-fs",
            "--store",
            "/usr/share/op-dbus/lungs-view",
            "mcp",
            "--lungs-socket",
            "/run/opdbus/lungs/lungs.sock",
            "--all",
        ])
        .unwrap();
        let Command::Mcp { lungs, .. } = cli.command else {
            panic!("MCP command expected")
        };
        let Some(Lungs::Socket { session, .. }) = lungs.lungs() else {
            panic!("socket expected")
        };
        assert!(session.is_empty());
    }

    #[test]
    fn host_session_still_requires_socket_and_preserves_selected_identity() {
        assert!(
            Cli::try_parse_from(["threetched-fs", "mcp", "--lungs-session", "session"]).is_err()
        );
        let cli = Cli::try_parse_from([
            "threetched-fs",
            "mcp",
            "--lungs-socket",
            "/run/opdbus/lungs/lungs.sock",
            "--lungs-session",
            "session",
        ])
        .unwrap();
        let Command::Mcp { lungs, .. } = cli.command else {
            panic!("MCP command expected")
        };
        let Some(Lungs::Socket { session, .. }) = lungs.lungs() else {
            panic!("socket expected")
        };
        assert_eq!(session, "session");
    }
}
