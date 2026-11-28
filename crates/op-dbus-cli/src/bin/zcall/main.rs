//! zcall — operator CLI over the sealed plugin catalog.
//!
//! The command tree (plugins → methods → argument flags → closed values) is
//! read at run time from the identity's mounted 3tchedFS namespace, the live
//! mirror of the sealed blobs; there is no per-plugin code. Calls go only through
//! the identity's mounted 3tchedFS namespace (`plugins/<p>/operations/<m>/call`),
//! which the bridge admits and receipts. `zcall ask` uses JEV to fill a call
//! from a sentence and prints it; it never calls.

mod args;
mod ask;
mod catalog;
mod complete;
mod mount;

use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use catalog::{Catalog, closed_values, type_label};

const USAGE: &str = "\
zcall — call sealed op-dbus plugin methods through 3tchedFS

Usage:
  zcall <plugin> <method> [--flag value ...] [--yes]   call a method
  zcall list                                         sealed plugins
  zcall methods <plugin>                             methods with effect and capability
  zcall help <plugin> [<method>]                     a method's flags, types and values
  zcall state <plugin>                               live state through 3tchedFS
  zcall paths <plugin> <method>                      the mount-relative operation path
  zcall expand <plugin> <method> [flags]             show path and JSON, do not call
  zcall ask \"<what you want done>\" [--show-requests] JEV fills in a zcall line
  zcall completion bash|zsh|fish                     print the completion script

Call options:
  --<arg> <value>   one flag per method argument, typed by the sealed schema;
                    switches take no value; enum flags complete their values
  --json <JSON>     the whole argument object instead of flags
  --yes             required to run a mutating method
  --print           show the operation path and arguments instead of calling

Global options (before the command):
  --mount <dir>            3tchedFS mount (env ZCALL_MOUNT; default /run/mounts/3tchedFS,
                           else the only readable /run/mounts/by-container/*/3tchedFS)
  --identity-mode <mode>   expected mount identity (default: the mount's declaration)

Completion:
  bash: eval \"$(zcall completion bash)\"     zsh: source <(zcall completion zsh)
  fish: zcall completion fish | source
";

struct Globals {
    mount: Option<String>,
    identity_mode: Option<String>,
}

fn main() -> ExitCode {
    // Behave like other CLIs when piped into `head`: exit quietly on EPIPE.
    // SAFETY: resetting a signal disposition at startup, before any threads.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("zcall: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let mut argv: Vec<String> = std::env::args().skip(1).collect();

    // Completion runs on every TAB: reads from the mount only, no network, no calls.
    if argv.first().map(String::as_str) == Some("__complete") {
        let shell = argv.get(1).and_then(|s| complete::Shell::parse(s));
        let line = argv.get(2).cloned().unwrap_or_default();
        if let Some(shell) = shell {
            complete::run(shell, &line);
        }
        return Ok(());
    }

    let mut globals = Globals {
        mount: std::env::var("ZCALL_MOUNT").ok(),
        identity_mode: None,
    };
    while let Some(first) = argv.first().cloned() {
        if !first.starts_with('-') {
            break;
        }
        argv.remove(0);
        match first.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("zcall {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--mount" | "--identity-mode" => {
                let value = (!argv.is_empty()).then(|| argv.remove(0)).with_context(|| format!("{first} needs a value"))?;
                match first.as_str() {
                    "--mount" => globals.mount = Some(value),
                    _ => globals.identity_mode = Some(value),
                }
            }
            other => bail!("unknown option {other} (see zcall --help)"),
        }
    }
    let Some(command) = argv.first().cloned() else {
        print!("{USAGE}");
        return Ok(());
    };
    let rest = &argv[1..];
    match command.as_str() {
        "help" if rest.is_empty() => {
            print!("{USAGE}");
            return Ok(());
        }
        "completion" => {
            let shell = rest
                .first()
                .and_then(|s| complete::Shell::parse(s))
                .context("usage: zcall completion bash|zsh|fish")?;
            print!("{}", complete::script(shell));
            return Ok(());
        }
        _ => {}
    }
    // Everything else discovers through, and calls through, the one mount.
    let catalog = Catalog::open(globals.mount.as_deref(), globals.identity_mode.as_deref())?;
    match command.as_str() {
        "list" => {
            let plugins = catalog.plugins();
            if plugins.is_empty() {
                bail!("the 3tchedFS mount {} lists no plugins", catalog.path().display());
            }
            for plugin in plugins {
                println!("{plugin}");
            }
        }
        "methods" => {
            let plugin = plugin_arg(&catalog, rest.first())?;
            let methods = catalog.methods(&plugin);
            let width = methods.keys().map(String::len).max().unwrap_or(6).max(6);
            println!("{:width$}  {:8}  {}", "METHOD", "EFFECT", "CAPABILITY");
            for method in methods.values() {
                println!(
                    "{:width$}  {:8}  {}",
                    method.name,
                    method.side_effect,
                    method.capability.as_deref().unwrap_or("-")
                );
            }
        }
        "help" => {
            let plugin = plugin_arg(&catalog, rest.first())?;
            match rest.get(1) {
                None => {
                    for method in catalog.methods(&plugin).values() {
                        println!("{}  {}", method.name, method.summary());
                    }
                }
                Some(typed) => print_method_help(&catalog, &plugin, typed)?,
            }
        }
        "paths" => {
            let plugin = plugin_arg(&catalog, rest.first())?;
            let method = method_arg(&catalog, &plugin, rest.get(1))?;
            println!("{}", mount::call_path(&plugin, &method.name));
        }
        "state" => {
            let plugin = plugin_arg(&catalog, rest.first())?;
            println!("{}", serde_json::to_string_pretty(&mount::state(catalog.namespace(), &plugin)?)?);
        }
        "ask" => {
            let show = rest.iter().any(|w| w == "--show-requests");
            let request: Vec<&str> = rest.iter().filter(|w| *w != "--show-requests").map(String::as_str).collect();
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(ask::run(&catalog, &request.join(" "), show))?;
        }
        "expand" => call(&catalog, rest, true)?,
        _ => call(&catalog, &argv, false)?,
    }
    Ok(())
}

fn plugin_arg(catalog: &Catalog, typed: Option<&String>) -> Result<String> {
    let typed = typed.context("missing plugin name (zcall list)")?;
    catalog
        .resolve_plugin(typed)
        .with_context(|| format!("no plugin {typed} in the 3tchedFS mount {} (zcall list)", catalog.path().display()))
}

fn method_arg(catalog: &Catalog, plugin: &str, typed: Option<&String>) -> Result<catalog::Method> {
    let typed = typed.with_context(|| format!("missing method name (zcall methods {plugin})"))?;
    catalog
        .resolve_method(plugin, typed)
        .with_context(|| format!("{plugin} declares no method {typed} (zcall methods {plugin})"))
}

fn print_method_help(catalog: &Catalog, plugin: &str, typed: &String) -> Result<()> {
    let method = method_arg(catalog, plugin, Some(typed))?;
    println!("{plugin} {}  ({})", method.name, method.summary());
    println!("subid: {}", method.subid);
    println!("path:  {}", mount::call_path(plugin, &method.name));
    let flags = method.flags();
    if flags.is_empty() {
        println!("takes no arguments");
    }
    for flag in flags {
        let values = closed_values(&flag.schema);
        println!(
            "  {:28} {}{}{}{}",
            flag.flag,
            type_label(&flag.schema),
            if flag.required { ", required" } else { "" },
            if values.is_empty() { String::new() } else { format!(" [{}]", values.join("|")) },
            flag.schema["description"]
                .as_str()
                .map(|d| format!(" — {}", d.split_whitespace().collect::<Vec<_>>().join(" ")))
                .unwrap_or_default(),
        );
    }
    Ok(())
}

fn call(catalog: &Catalog, words: &[String], expand: bool) -> Result<()> {
    let plugin = plugin_arg(catalog, words.first())?;
    let method = method_arg(catalog, &plugin, words.get(1))?;
    let (arguments, options) = args::build(&method, &words[2..])?;
    if expand || options.print {
        println!("{}", mount::call_path(&plugin, &method.name));
        println!("{}", serde_json::to_string_pretty(&arguments)?);
        return Ok(());
    }
    if method.side_effect == "mutation" && !options.confirm {
        bail!("{plugin} {} mutates state; add --yes to run it", method.name);
    }
    let receipt = mount::call(catalog.namespace(), &plugin, &method.name, arguments, options.confirm)?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}
