//! Runit Introspection Adapter
//!
//! Introspects the runit service supervisor, discovering:
//! - The `sv` command set (up, down, once, status, restart, check, …)
//! - Which signal each signalling command sends
//! - The supervisor binaries and what each one is for
//!
//! Modelled on [`crate::gcloud`]. The difference is where the structure lives:
//! gcloud publishes a rich `--help` tree, whereas `sv help` prints a single
//! usage line and runit documents itself in roff man pages. So this parses
//! `man/*.8` from a runit source checkout rather than shelling out — which also
//! means it needs no runit installed to run, and yields the same answer on any
//! machine for a given source tree.
//!
//! # Usage
//!
//! ```rust,no_run
//! use op_inspector::runit::RunitParser;
//!
//! let parser = RunitParser::new();
//! let schema = parser.introspect_source("/home/admin/git/runit")?;
//! for cmd in &schema.commands {
//!     println!("{:12} {}", cmd.name, cmd.description);
//! }
//! # Ok::<(), anyhow::Error>(())
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Everything discovered about a runit installation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunitSchema {
    /// Commands accepted by `sv`, in man-page order.
    pub commands: Vec<RunitCommand>,
    /// Supervisor binaries, keyed by name, valued by their one-line purpose.
    pub binaries: BTreeMap<String, String>,
    pub stats: RunitStats,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunitStats {
    pub command_count: usize,
    pub signalling_command_count: usize,
    pub binary_count: usize,
}

/// One `sv` command.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunitCommand {
    /// Command as typed, e.g. `force-restart`.
    pub name: String,
    /// Prose from the man page, flattened to a single line.
    pub description: String,
    /// For commands that only deliver a signal, which one — e.g. `hup` → `HUP`.
    /// `None` for commands with richer behaviour such as `restart` or `check`.
    pub signal: Option<String>,
    /// True when the man page documents this as waiting for the service to
    /// reach the requested state. These are the ones a caller must give a
    /// timeout, and the reason `sv` has `-w`.
    pub waits: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RunitParser;

impl RunitParser {
    pub fn new() -> Self {
        Self
    }

    /// Introspect a runit source checkout — the directory containing `man/`.
    pub fn introspect_source(&self, root: impl AsRef<Path>) -> Result<RunitSchema> {
        let root = root.as_ref();
        let man = root.join("man");
        let sv = std::fs::read_to_string(man.join("sv.8"))
            .with_context(|| format!("read {}", man.join("sv.8").display()))?;

        let commands = self.parse_commands(&sv);

        let mut binaries = BTreeMap::new();
        for entry in std::fs::read_dir(&man)
            .with_context(|| format!("read dir {}", man.display()))?
            .flatten()
        {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("8") {
                continue;
            }
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(_) => continue,
            };
            if let Some((name, purpose)) = self.parse_name_section(&text) {
                binaries.insert(name, purpose);
            }
        }

        let stats = RunitStats {
            command_count: commands.len(),
            signalling_command_count: commands.iter().filter(|c| c.signal.is_some()).count(),
            binary_count: binaries.len(),
        };

        Ok(RunitSchema {
            commands,
            binaries,
            stats,
        })
    }

    /// Parse the `.SH COMMANDS` section into individual commands.
    ///
    /// Two shapes have to be handled. Most entries are one `.B name` followed by
    /// prose. But signalling commands are collapsed onto a single line —
    /// `.B pause cont hup alarm interrupt quit 1 2 term kill` — sharing one
    /// description that lists the signals positionally. Those are expanded into
    /// one command each, with its own signal attached.
    pub fn parse_commands(&self, man: &str) -> Vec<RunitCommand> {
        let mut out = Vec::new();
        let mut in_commands = false;
        let mut pending: Option<(Vec<String>, Vec<String>)> = None;

        let flush = |pending: &mut Option<(Vec<String>, Vec<String>)>,
                     out: &mut Vec<RunitCommand>| {
            let Some((names, body)) = pending.take() else {
                return;
            };
            let description = flatten_roff(&body);
            let signals = extract_signal_list(&description);
            let waits = description.contains("wait") || description.contains("up to");
            for (i, name) in names.iter().enumerate() {
                out.push(RunitCommand {
                    name: name.clone(),
                    description: description.clone(),
                    // Only a collapsed signalling line maps positionally.
                    signal: if names.len() > 1 {
                        signals.get(i).cloned()
                    } else {
                        None
                    },
                    waits,
                });
            }
        };

        for line in man.lines() {
            if let Some(rest) = line.strip_prefix(".SH ") {
                if in_commands {
                    flush(&mut pending, &mut out);
                    break; // COMMANDS ended
                }
                in_commands = rest.trim() == "COMMANDS";
                continue;
            }
            if !in_commands {
                continue;
            }
            if let Some(rest) = line.strip_prefix(".B ") {
                flush(&mut pending, &mut out);
                let names: Vec<String> = rest
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                // `.B sv` inside COMMANDS is a cross-reference to the tool
                // itself ("sv actually looks only at the first character…"),
                // not a command. Fold it into the running description instead.
                if names == ["sv"] {
                    if let Some((_, body)) = pending.as_mut() {
                        body.push("sv".to_string());
                    }
                    continue;
                }
                if !names.is_empty() {
                    pending = Some((names, Vec::new()));
                }
                continue;
            }
            if let Some((_, body)) = pending.as_mut() {
                // `.TP`/`.P` are pure layout. `.B`/`.BR`/`.I`/`.IR` carry the
                // referenced word itself — dropping them turns "Same as up but
                // wait…" into "Same as but wait…", losing which command is meant.
                if let Some(rest) = line
                    .strip_prefix(".BR ")
                    .or_else(|| line.strip_prefix(".IR "))
                    .or_else(|| line.strip_prefix(".I "))
                {
                    if let Some(word) = rest.split_whitespace().next() {
                        body.push(word.to_string());
                    }
                } else if !line.starts_with('.') {
                    body.push(line.to_string());
                }
            }
        }
        flush(&mut pending, &mut out);
        // `status` is documented twice — once fully, once as a cross-reference
        // in the LSB-compatibility block. Keep the first, richer definition.
        let mut seen = std::collections::HashSet::new();
        out.retain(|c| seen.insert(c.name.clone()));
        out
    }

    /// Pull `name \- purpose` out of a man page's `.SH NAME` section.
    fn parse_name_section(&self, man: &str) -> Option<(String, String)> {
        let mut lines = man.lines();
        while let Some(line) = lines.next() {
            if line.trim() == ".SH NAME" {
                let body = lines.next()?.trim().to_string();
                let (name, purpose) = body.split_once("\\-")?;
                return Some((
                    name.trim().to_string(),
                    purpose.trim().trim_end_matches('.').to_string(),
                ));
            }
        }
        None
    }
}

/// Collapse roff body lines into one readable sentence.
fn flatten_roff(body: &[String]) -> String {
    let joined = body.join(" ");
    let mut out = String::with_capacity(joined.len());
    let mut chars = joined.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                // `\-` is a literal hyphen; drop other escapes.
                if chars.peek() == Some(&'-') {
                    chars.next();
                    out.push('-');
                }
            }
            _ => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Signals named in a collapsed signalling description, in the order listed.
fn extract_signal_list(description: &str) -> Vec<String> {
    const KNOWN: &[&str] = &[
        "STOP", "CONT", "HUP", "ALRM", "INT", "QUIT", "USR1", "USR2", "TERM", "KILL",
    ];
    let mut seen = Vec::new();
    for word in description.split(|c: char| !c.is_ascii_alphanumeric()) {
        if KNOWN.contains(&word) && !seen.iter().any(|s| s == word) {
            seen.push(word.to_string());
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_root() -> Option<std::path::PathBuf> {
        let p = std::path::PathBuf::from("/home/admin/git/runit");
        p.join("man/sv.8").exists().then_some(p)
    }

    #[test]
    fn parses_the_real_sv_man_page() {
        let Some(root) = source_root() else {
            eprintln!("runit source not present; skipping");
            return;
        };
        let schema = RunitParser::new().introspect_source(&root).unwrap();
        let names: Vec<&str> = schema.commands.iter().map(|c| c.name.as_str()).collect();

        // The lifecycle commands the control plane actually issues.
        for expected in [
            "status", "up", "down", "once", "start", "stop", "restart", "check",
        ] {
            assert!(
                names.contains(&expected),
                "missing `{expected}` in {names:?}"
            );
        }
        // force-restart matters specifically: plain `restart` does not cycle a
        // logger, because svlogd blocks on the still-open pipe.
        assert!(names.contains(&"force-restart"));
        assert!(schema.stats.command_count >= 15, "got {names:?}");
    }

    #[test]
    fn expands_the_collapsed_signalling_line_into_one_command_each() {
        // `.B pause cont hup alarm interrupt quit 1 2 term kill` is a single
        // man-page entry covering ten commands.
        let Some(root) = source_root() else { return };
        let schema = RunitParser::new().introspect_source(&root).unwrap();
        let by_name: BTreeMap<_, _> = schema
            .commands
            .iter()
            .map(|c| (c.name.as_str(), c))
            .collect();

        for (cmd, signal) in [
            ("pause", "STOP"),
            ("cont", "CONT"),
            ("hup", "HUP"),
            ("term", "TERM"),
        ] {
            let got = by_name
                .get(cmd)
                .unwrap_or_else(|| panic!("`{cmd}` not parsed"));
            assert_eq!(
                got.signal.as_deref(),
                Some(signal),
                "`{cmd}` should map to {signal}"
            );
        }
    }

    #[test]
    fn discovers_the_supervisor_binaries() {
        let Some(root) = source_root() else { return };
        let schema = RunitParser::new().introspect_source(&root).unwrap();
        for expected in ["sv", "runsv", "runsvdir", "svlogd", "chpst"] {
            assert!(
                schema.binaries.contains_key(expected),
                "missing `{expected}` in {:?}",
                schema.binaries.keys().collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn descriptions_are_readable_prose() {
        let Some(root) = source_root() else { return };
        let schema = RunitParser::new().introspect_source(&root).unwrap();
        let up = schema.commands.iter().find(|c| c.name == "up").unwrap();
        assert!(up.description.contains("start"), "got: {}", up.description);
        assert!(
            !up.description.contains(".TP"),
            "roff leaked: {}",
            up.description
        );
        assert!(
            !up.description.contains('\\'),
            "escape leaked: {}",
            up.description
        );
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;

    fn schema() -> Option<RunitSchema> {
        let root = std::path::PathBuf::from("/home/admin/git/runit");
        root.join("man/sv.8")
            .exists()
            .then(|| RunitParser::new().introspect_source(&root).unwrap())
    }

    #[test]
    fn does_not_invent_sv_as_a_command() {
        // `.B sv` appears inside COMMANDS as a cross-reference to the tool
        // itself; a naive parse emits it three times as a bogus command.
        let Some(s) = schema() else { return };
        assert!(!s.commands.iter().any(|c| c.name == "sv"));
    }

    #[test]
    fn command_names_are_unique() {
        // `status` is documented twice — fully, then again in the
        // LSB-compatibility block.
        let Some(s) = schema() else { return };
        let mut seen = std::collections::HashSet::new();
        for c in &s.commands {
            assert!(seen.insert(&c.name), "duplicate command `{}`", c.name);
        }
    }

    #[test]
    fn cross_referenced_command_names_survive_into_descriptions() {
        // Dropping roff `.B`/`.BR` lines turns "Same as up but wait…" into
        // "Same as but wait…", losing which command is being referenced.
        let Some(s) = schema() else { return };
        let by: std::collections::BTreeMap<_, _> =
            s.commands.iter().map(|c| (c.name.as_str(), c)).collect();
        assert!(
            by["start"].description.contains("Same as up"),
            "{}",
            by["start"].description
        );
        assert!(
            by["stop"].description.contains("Same as down"),
            "{}",
            by["stop"].description
        );
        assert!(
            by["reload"].description.contains("hup"),
            "{}",
            by["reload"].description
        );
    }

    #[test]
    fn waiting_commands_are_flagged() {
        // These are the ones needing a timeout — the reason `sv` has `-w`.
        let Some(s) = schema() else { return };
        let by: std::collections::BTreeMap<_, _> =
            s.commands.iter().map(|c| (c.name.as_str(), c)).collect();
        for w in ["start", "stop", "restart", "check", "force-restart"] {
            assert!(by[w].waits, "`{w}` should be flagged as waiting");
        }
        for nw in ["up", "down", "once", "status"] {
            assert!(!by[nw].waits, "`{nw}` should not be flagged as waiting");
        }
    }
}
