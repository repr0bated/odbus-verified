//! dbus-plugin-cli — Dynamic CLI for org.opdbus.v1.plugins
//!
//! Introspects the live D-Bus tree and generates subcommands for every
//! discovered method. Supports:
//! - `dbus-plugin-cli tree` — show full plugin tree
//! - `dbus-plugin-cli list` — list plugins
//! - `dbus-plugin-cli <plugin> <method> [ARGS...]` — call a method
//! - `dbus-plugin-cli <plugin> get <property>` — read a property
//! - `dbus-plugin-cli completions <shell>` — generate shell completions

mod identity;
mod introspect;
mod socket;

use anyhow::{bail, Context, Result};
use identity::CliIdentity;
use introspect::PluginTreeAdapter;
use std::process::ExitCode;
use tracing::info;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const SERVICE_PREFIX: &str = "org.opdbus.v1.plugins";
const BASE_PATH: &str = "/org/opdbus/v1/plugins";

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> ExitCode {
    // Initialize tracing (quiet unless RUST_LOG is set)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .without_time()
        .init();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {:#}", e);
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // Fast-path: no args or --help
    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" {
        print_top_level_help().await?;
        return Ok(());
    }

    match args[1].as_str() {
        "tree" => cmd_tree(args.get(2).map(|s| s.as_str())).await,
        "list" => cmd_list().await,
        "identity" => cmd_identity().await,
        "completions" => cmd_completions(args.get(2).map(|s| s.as_str())).await,
        "--version" | "-V" => {
            println!("dbus-plugin-cli {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        plugin => {
            // Dynamic plugin subcommand
            if args.len() < 3 || args[2] == "--help" || args[2] == "-h" {
                cmd_plugin_help(plugin).await
            } else {
                let method = &args[2];
                let method_args = &args[3..];
                cmd_call(plugin, method, method_args).await
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Show the full D-Bus plugin tree with all methods/properties/signals
async fn cmd_tree(filter: Option<&str>) -> Result<()> {
    let adapter = PluginTreeAdapter::new();
    let tree = adapter.introspect_tree().await?;

    if tree.plugins.is_empty() {
        println!("No org.opdbus.v1.plugins.* services found on system bus.");
        println!("Is the opdbus service running?");
        return Ok(());
    }

    for (name, plugin) in &tree.plugins {
        // Apply optional filter
        if let Some(f) = filter {
            if !name.contains(f) {
                continue;
            }
        }

        println!("╭─ {} ({})", name, plugin.service_name);
        println!("│  path: {}", plugin.object_path);

        for iface in &plugin.interfaces {
            println!("│");
            println!("│  interface: {}", iface.name);

            if !iface.methods.is_empty() {
                println!("│    Methods:");
                for m in &iface.methods {
                    println!("│      .{} {}", m.name, m.signature_display);
                }
            }

            if !iface.properties.is_empty() {
                println!("│    Properties:");
                for p in &iface.properties {
                    println!("│      .{} : {} [{}]", p.name, p.type_display, p.access);
                }
            }

            if !iface.signals.is_empty() {
                println!("│    Signals:");
                for s in &iface.signals {
                    let args_str: String = s
                        .args
                        .iter()
                        .map(|a| {
                            if a.name.is_empty() {
                                a.type_display.clone()
                            } else {
                                format!("{}: {}", a.name, a.type_display)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    println!("│      .{} ({})", s.name, args_str);
                }
            }
        }

        if !plugin.children.is_empty() {
            println!("│");
            println!("│  Children:");
            for child in &plugin.children {
                println!("│    {}", child);
            }
        }

        println!("╰─");
        println!();
    }

    println!(
        "Summary: {} plugins, {} methods, {} properties, {} signals",
        tree.stats.plugin_count,
        tree.stats.total_methods,
        tree.stats.total_properties,
        tree.stats.total_signals,
    );

    Ok(())
}

/// List all discovered plugins
async fn cmd_list() -> Result<()> {
    let adapter = PluginTreeAdapter::new();
    let tree = adapter.introspect_tree().await?;

    if tree.plugins.is_empty() {
        println!("No org.opdbus.v1.plugins.* services found on system bus.");
        return Ok(());
    }

    println!("{:<24} {:<50} METHODS", "PLUGIN", "SERVICE");
    println!("{}", "─".repeat(80));

    for (name, plugin) in &tree.plugins {
        let method_count: usize = plugin.interfaces.iter().map(|i| i.methods.len()).sum();
        println!("{:<24} {:<50} {}", name, plugin.service_name, method_count);
    }

    Ok(())
}

/// Show help for a specific plugin (all its methods)
async fn cmd_plugin_help(plugin_name: &str) -> Result<()> {
    let adapter = PluginTreeAdapter::new();
    let tree = adapter.introspect_tree().await?;

    // Normalize: try both underscore and hyphen forms
    let normalized = plugin_name.replace('-', "_");
    let plugin = tree
        .plugins
        .get(&normalized)
        .or_else(|| tree.plugins.get(plugin_name));

    let plugin = match plugin {
        Some(p) => p,
        None => {
            eprintln!("Error: Plugin '{}' not found on D-Bus.\n", plugin_name);
            eprintln!("Available plugins:");
            for name in tree.plugins.keys() {
                eprintln!("  {}", name);
            }
            bail!("Plugin not found");
        }
    };

    println!("Plugin: {} ({})", plugin.name, plugin.service_name);
    println!("Path:   {}", plugin.object_path);
    println!();

    for iface in &plugin.interfaces {
        println!("Interface: {}", iface.name);
        println!();

        if !iface.methods.is_empty() {
            println!("  Methods:");
            for m in &iface.methods {
                println!("    {} {}", m.name, m.signature_display);
                if !m.in_args.is_empty() {
                    for a in &m.in_args {
                        let label = if a.name.is_empty() {
                            "arg".to_string()
                        } else {
                            a.name.clone()
                        };
                        println!(
                            "      --{:<16} {} (required, type: {})",
                            label, a.type_display, a.signature
                        );
                    }
                }
                println!();
            }
        }

        if !iface.properties.is_empty() {
            println!("  Properties:");
            for p in &iface.properties {
                println!("    {:<24} {} [{}]", p.name, p.type_display, p.access);
            }
            println!();
        }

        if !iface.signals.is_empty() {
            println!("  Signals:");
            for s in &iface.signals {
                let args_str: String = s
                    .args
                    .iter()
                    .map(|a| format!("{}: {}", a.name, a.type_display))
                    .collect::<Vec<_>>()
                    .join(", ");
                println!("    {} ({})", s.name, args_str);
            }
            println!();
        }
    }

    println!("Usage:");
    println!(
        "  dbus-plugin-cli {} <method> [--arg VALUE ...]",
        plugin_name
    );
    println!("  dbus-plugin-cli {} get <property>", plugin_name);

    Ok(())
}

/// Call a method on a plugin via D-Bus
async fn cmd_call(plugin_name: &str, method_name: &str, args: &[String]) -> Result<()> {
    // Handle "get" subcommand for properties
    if method_name == "get" {
        return cmd_get_property(plugin_name, args).await;
    }

    // Pre-flight: ensure gRPC socket is live and permissions are correct
    socket::ensure_socket_ready()?;

    let normalized = plugin_name.replace('-', "_");
    let service_name = format!("{}.{}", SERVICE_PREFIX, normalized);
    let object_path = format!("{}/{}", BASE_PATH, normalized);

    // Connect to system bus
    let connection = zbus::Connection::session()
        .await
        .context("Failed to connect to session bus")?;

    // First introspect to find the correct interface for this method
    let adapter = PluginTreeAdapter::new();
    let tree = adapter.introspect_tree().await?;
    let plugin = tree
        .plugins
        .get(&normalized)
        .or_else(|| tree.plugins.get(plugin_name));

    let plugin = match plugin {
        Some(p) => p,
        None => bail!(
            "Plugin '{}' not found. Run 'dbus-plugin-cli list' to see available plugins.",
            plugin_name
        ),
    };

    // Find the method and its interface
    let mut target_interface = None;
    let mut target_method = None;

    let method_normalized = method_name.replace('-', "_");

    for iface in &plugin.interfaces {
        for m in &iface.methods {
            if m.name == method_name || m.name.to_lowercase() == method_normalized.to_lowercase() {
                target_interface = Some(iface.name.clone());
                target_method = Some(m.clone());
                break;
            }
        }
        if target_interface.is_some() {
            break;
        }
    }

    let iface_name = match target_interface {
        Some(i) => i,
        None => {
            eprintln!(
                "Error: Method '{}' not found on plugin '{}'.\n",
                method_name, plugin_name
            );
            eprintln!("Available methods:");
            for iface in &plugin.interfaces {
                for m in &iface.methods {
                    eprintln!("  {} {}", m.name, m.signature_display);
                }
            }
            bail!("Method not found");
        }
    };

    let method = target_method.unwrap();

    // Validate argument count
    let required_args = method.in_args.len();
    if args.len() < required_args {
        eprintln!(
            "Error: Missing required argument(s) for '{}'.\n",
            method.name
        );
        eprintln!(
            "Usage: dbus-plugin-cli {} {} {}",
            plugin_name,
            method.name,
            method
                .in_args
                .iter()
                .map(|a| {
                    let label = if a.name.is_empty() {
                        "ARG".to_string()
                    } else {
                        a.name.to_uppercase()
                    };
                    format!("<{}>", label)
                })
                .collect::<Vec<_>>()
                .join(" ")
        );
        eprintln!();
        eprintln!("Arguments:");
        for a in &method.in_args {
            let label = if a.name.is_empty() { "arg" } else { &a.name };
            eprintln!("  {:<16} {} (type: {})", label, a.type_display, a.signature);
        }
        bail!("Missing required arguments");
    }

    // Build the D-Bus method call
    let in_signature: String = method
        .in_args
        .iter()
        .map(|a| a.signature.as_str())
        .collect();

    info!(
        "Calling {}.{} on {} at {} with signature '{}'",
        iface_name, method.name, service_name, object_path, in_signature
    );

    // Call via zbus — for now all args are passed as strings
    // (appropriate for the common 's' signature case)
    let reply = if args.is_empty() {
        connection
            .call_method(
                Some(service_name.as_str()),
                object_path.as_str(),
                Some(iface_name.as_str()),
                method.name.as_str(),
                &(),
            )
            .await
    } else if args.len() == 1 {
        connection
            .call_method(
                Some(service_name.as_str()),
                object_path.as_str(),
                Some(iface_name.as_str()),
                method.name.as_str(),
                &args[0].as_str(),
            )
            .await
    } else {
        // Multiple args — pass as tuple of strings
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        connection
            .call_method(
                Some(service_name.as_str()),
                object_path.as_str(),
                Some(iface_name.as_str()),
                method.name.as_str(),
                &arg_refs,
            )
            .await
    };

    match reply {
        Ok(reply) => {
            // Try to extract the reply body as a string
            let body = reply.body();
            if let Ok(s) = body.deserialize::<String>() {
                println!("{}", s);
            } else if let Ok(strings) = body.deserialize::<Vec<String>>() {
                for s in strings {
                    println!("{}", s);
                }
            } else {
                println!("(method returned successfully)");
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("D-Bus error calling {}.{}:", iface_name, method.name);
            eprintln!("  {}", e);
            bail!("D-Bus method call failed: {}", e);
        }
    }
}

/// Get a property from a plugin
async fn cmd_get_property(plugin_name: &str, args: &[String]) -> Result<()> {
    if args.is_empty() {
        bail!(
            "Usage: dbus-plugin-cli {} get <property-name>\n\nRun 'dbus-plugin-cli {} --help' to see available properties.",
            plugin_name, plugin_name
        );
    }

    let property_name = &args[0];
    let normalized = plugin_name.replace('-', "_");
    let service_name = format!("{}.{}", SERVICE_PREFIX, normalized);
    let object_path = format!("{}/{}", BASE_PATH, normalized);

    // Introspect to find the correct interface
    let adapter = PluginTreeAdapter::new();
    let tree = adapter.introspect_tree().await?;
    let plugin = tree
        .plugins
        .get(&normalized)
        .or_else(|| tree.plugins.get(plugin_name));

    let plugin = match plugin {
        Some(p) => p,
        None => bail!("Plugin '{}' not found", plugin_name),
    };

    // Find the property's interface
    let mut prop_interface = None;
    for iface in &plugin.interfaces {
        for p in &iface.properties {
            if p.name == *property_name {
                prop_interface = Some(iface.name.clone());
                break;
            }
        }
    }

    let iface_name = match prop_interface {
        Some(i) => i,
        None => {
            eprintln!(
                "Error: Property '{}' not found on plugin '{}'.\n",
                property_name, plugin_name
            );
            eprintln!("Available properties:");
            for iface in &plugin.interfaces {
                for p in &iface.properties {
                    eprintln!("  {:<24} {} [{}]", p.name, p.type_display, p.access);
                }
            }
            bail!("Property not found");
        }
    };

    // Read the property via org.freedesktop.DBus.Properties.Get
    let connection = zbus::Connection::session()
        .await
        .context("Failed to connect to session bus")?;

    let reply = connection
        .call_method(
            Some(service_name.as_str()),
            object_path.as_str(),
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(iface_name.as_str(), property_name.as_str()),
        )
        .await;

    match reply {
        Ok(reply) => {
            let body = reply.body();
            if let Ok(val) = body.deserialize::<zbus::zvariant::OwnedValue>() {
                println!("{}: {:?}", property_name, val);
            } else {
                println!("(property read successful but could not display value)");
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("D-Bus error reading property '{}':", property_name);
            eprintln!("  {}", e);
            bail!("Property read failed: {}", e);
        }
    }
}

/// Show current identity (read from projection)
async fn cmd_identity() -> Result<()> {
    match CliIdentity::read() {
        Some(id) => {
            id.display();
            Ok(())
        }
        None => {
            eprintln!("No valid identity found.");
            eprintln!("Check that /dev/shm/opdbus/state/identity_sled.json exists");
            eprintln!("and contains an active sled with a non-empty footprint.");
            bail!("Identity not available");
        }
    }
}

/// Generate shell completions
async fn cmd_completions(shell: Option<&str>) -> Result<()> {
    match shell.unwrap_or("bash") {
        "bash" => print_bash_completions().await,
        "fish" => {
            println!("# Fish completions not yet implemented");
            Ok(())
        }
        "zsh" => {
            println!("# Zsh completions not yet implemented");
            Ok(())
        }
        other => bail!("Unsupported shell: {}. Use bash, fish, or zsh.", other),
    }
}

/// Print top-level help with dynamically discovered plugins
async fn print_top_level_help() -> Result<()> {
    println!("dbus-plugin-cli — Dynamic CLI for org.opdbus.v1.plugins\n");
    println!("USAGE:");
    println!("  dbus-plugin-cli <command> [options]\n");
    println!("COMMANDS:");
    println!("  tree [filter]         Show full D-Bus plugin tree (methods, properties, signals)");
    println!("  list                  List all discovered plugins");
    println!("  identity              Show current GhostBridge identity (footprint/trace)");
    println!("  completions [shell]   Generate shell completions (bash, fish, zsh)");
    println!("  <plugin> --help       Show methods/properties for a specific plugin");
    println!("  <plugin> <method>     Call a method on a plugin");
    println!("  <plugin> get <prop>   Read a property from a plugin\n");

    // Try to list available plugins
    let adapter = PluginTreeAdapter::new();
    match adapter.introspect_tree().await {
        Ok(tree) if !tree.plugins.is_empty() => {
            println!("AVAILABLE PLUGINS:");
            for (name, plugin) in &tree.plugins {
                let method_count: usize = plugin.interfaces.iter().map(|i| i.methods.len()).sum();
                println!("  {:<24} ({} methods)", name, method_count);
            }
            println!();
        }
        _ => {
            println!("(No plugins discovered — is the opdbus service running?)\n");
        }
    }

    println!("OPTIONS:");
    println!("  -h, --help       Show this help");
    println!("  -V, --version    Show version");
    println!();
    println!("ENVIRONMENT:");
    println!("  RUST_LOG         Set log level (e.g., RUST_LOG=debug)");

    Ok(())
}

/// Generate bash completion script
async fn print_bash_completions() -> Result<()> {
    println!(
        r#"# Bash completion for dbus-plugin-cli
# Source this file or add to /etc/bash_completion.d/

_dbus_plugin_cli() {{
    local cur prev words cword
    COMPREPLY=()
    _get_comp_words_by_ref -n : cur prev words cword 2>/dev/null || {{
        cur="${{COMP_WORDS[COMP_CWORD]}}"
        prev="${{COMP_WORDS[COMP_CWORD-1]}}"
        words=("${{COMP_WORDS[@]}}")
        cword="$COMP_CWORD"
    }}

    # Top-level commands
    if (( cword == 1 )); then
        local plugins
        plugins=$(dbus-plugin-cli list 2>/dev/null | tail -n +3 | awk '{{print $1}}')
        COMPREPLY=( $(compgen -W "tree list completions --help --version $plugins" -- "$cur") )
        return 0
    fi

    local first="${{words[1]}}"

    case "$first" in
        tree|list|completions|--help|--version)
            return 0
            ;;
    esac

    # Plugin subcommands (methods)
    if (( cword == 2 )); then
        local methods
        methods=$(dbus-plugin-cli "$first" --help 2>/dev/null | grep -A999 'Methods:' | grep '^\s' | awk '{{print $1}}' | tr -d '.')
        COMPREPLY=( $(compgen -W "get --help $methods" -- "$cur") )
        return 0
    fi
}}

complete -F _dbus_plugin_cli dbus-plugin-cli
"#
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Argument conversion helpers (future: type-aware conversion)
// ---------------------------------------------------------------------------
