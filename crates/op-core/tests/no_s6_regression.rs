//! Guard against s6 returning to the tree.
//!
//! This host boots runit as PID 1 and has no s6 installed, so any code that
//! spawns an s6 binary or hardcodes the s6 runtime tree is dead on arrival.
//! The `runit-sv-migration` spec removed every such site; this test fails if one
//! comes back, which is cheaper than discovering it when a service silently
//! never starts.
//!
//! Runs under plain `cargo test` — no CI infrastructure required.
//!
//! OSCAL subid: `obs.software.runit.s6-regression-guard@v1`

use std::path::{Path, PathBuf};

/// Spawning any of these fails at runtime: the binaries do not exist.
const FORBIDDEN_SPAWNS: &[&str] = &[
    "Command::new(\"s6-rc\")",
    "Command::new(\"s6-svc\")",
    "Command::new(\"s6-svstat\")",
    "Command::new(\"s6-svscan\")",
    "Command::new(\"s6-logwatch\")",
    "Command::new(\"s6d\")",
    "Command::new(\"service6\")",
    "Command::new(\"systemctl\")",
];

/// The s6 layout. Runit uses `/run/runit/service` and `/etc/runit/sv`.
const FORBIDDEN_PATHS: &[&str] = &["\"/run/service", "\"/etc/s6/sv", "\"/run/s6-rc"];

/// Files allowed to mention the forbidden strings, and why.
fn is_allowed(path: &Path) -> bool {
    let name = path.to_string_lossy();
    // This test necessarily contains the strings it forbids.
    if name.ends_with("no_s6_regression.rs") {
        return true;
    }
    // The path module documents what the s6 tree was, and asserts against it.
    if name.ends_with("op-core/src/runit.rs") {
        return true;
    }
    false
}

fn workspace_crates_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR is <workspace>/crates/op-core.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("op-core has a parent directory")
        .to_path_buf()
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            // Skip build output and vendored trees.
            if matches!(name.as_str(), "target" | "node_modules" | ".git") {
                continue;
            }
            rust_sources(&path, out);
        } else if name.ends_with(".rs") {
            // Generated protobuf code is not hand-maintained source.
            if path.to_string_lossy().contains("/proto/") {
                continue;
            }
            out.push(path);
        }
    }
}

#[test]
fn no_crate_spawns_an_s6_binary() {
    let mut files = Vec::new();
    rust_sources(&workspace_crates_dir(), &mut files);
    assert!(!files.is_empty(), "found no Rust sources to scan");

    let mut findings = Vec::new();
    for file in &files {
        if is_allowed(file) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for (idx, line) in text.lines().enumerate() {
            for needle in FORBIDDEN_SPAWNS {
                if line.contains(needle) {
                    findings.push(format!("{}:{}: {}", file.display(), idx + 1, needle));
                }
            }
        }
    }

    assert!(
        findings.is_empty(),
        "s6-era binaries are not installed on this host; these spawns would fail \
         at runtime. Use `sv` (see op_core::runit) instead:\n  {}",
        findings.join("\n  ")
    );
}

#[test]
fn no_crate_hardcodes_the_s6_service_tree() {
    let mut files = Vec::new();
    rust_sources(&workspace_crates_dir(), &mut files);

    let mut findings = Vec::new();
    for file in &files {
        if is_allowed(file) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for (idx, line) in text.lines().enumerate() {
            for needle in FORBIDDEN_PATHS {
                if line.contains(needle) {
                    findings.push(format!("{}:{}: {}", file.display(), idx + 1, needle));
                }
            }
        }
    }

    assert!(
        findings.is_empty(),
        "these are s6 paths; runit uses /etc/runit/sv and /run/runit/service. \
         Import op_core::runit instead of hardcoding:\n  {}",
        findings.join("\n  ")
    );
}

/// The agent-facing tool family must not advertise s6 verbs.
#[test]
fn agent_tools_are_named_sv_not_s6() {
    let mut files = Vec::new();
    rust_sources(&workspace_crates_dir(), &mut files);

    let mut findings = Vec::new();
    for file in &files {
        if is_allowed(file) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for (idx, line) in text.lines().enumerate() {
            if line.contains("\"s6_") {
                findings.push(format!("{}:{}", file.display(), idx + 1));
            }
        }
    }

    assert!(
        findings.is_empty(),
        "s6_* tool names were removed in favour of sv_*; a stale name in the \
         registry is exactly what makes a model reach for a dead tool:\n  {}",
        findings.join("\n  ")
    );
}
