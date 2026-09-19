//! Verified-source gate: every `.rs` under a crate's `src/` must be reachable
//! from a compile root, and no editor/patch artifacts may sit next to source.
//!
//! The 2026-09-19 review found ~41k LOC of orphaned files (never compiled,
//! never tested, contradicting "verified source") and 26 `.patch`/`.bak`/
//! `.sql` strays. This gate keeps the tree honest: a file that is not part of
//! the build cannot be verified and must not be committed as if it were.
//!
//! Reachability follows rustc: `lib.rs`, `main.rs`, `src/bin/*.rs`,
//! `src/bin/*/main.rs` and `[[bin]] path` entries are roots; `mod name;`
//! resolves to `name.rs` or `name/mod.rs` in the module's directory (or
//! `#[path = "…"]`); `include!("…")` pulls a file in. `#[cfg]`-gated modules
//! count as reachable — they are still compiled under some configuration.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Non-source files allowed next to source (deliberate, referenced).
const ALLOWED_NON_SOURCE_SUFFIXES: &[&str] = &[
    ".rs",
    ".proto",
    ".md",
    ".json",
    ".toml",
    ".txt",
    ".inc",
    ".template",
    ".gitkeep",
    ".ovsschema",
    ".pem",
    ".crt",
    ".key",
    ".yaml",
    ".yml",
    ".sh",
    ".html",
    ".css",
    ".js",
    ".csv",
    ".jsonl",
    ".cozo",
    ".py",
    ".snap",
    ".golden",
    ".xml",
    ".conf",
    ".service",
    ".ron",
];

/// Suffixes that are always artifacts.
const STRAY_SUFFIXES: &[&str] = &[
    ".bak",
    ".patch",
    ".backup",
    ".copied",
    ".stub",
    ".fix",
    ".orig",
    ".rej",
    ".swp",
    ".grpc-additions",
    ".code-workspace",
    ".sql",
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "target" || name == ".git" {
            continue;
        }
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `mod name;` declarations with an optional preceding `#[path = "…"]`.
fn mod_declarations(source: &str) -> Vec<(Option<String>, String)> {
    let mut out = Vec::new();
    let mut pending_path: Option<String> = None;
    for raw in source.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("#[path") {
            if let Some(start) = rest.find('"') {
                if let Some(end) = rest[start + 1..].find('"') {
                    pending_path = Some(rest[start + 1..start + 1 + end].to_string());
                }
            }
            // `#[path = "x"] mod y;` on one line falls through below.
            if !line.contains(" mod ") {
                continue;
            }
        }
        let without_attr = match line.rfind(']') {
            Some(i) if line.starts_with("#[") => line[i + 1..].trim(),
            _ => line,
        };
        let stripped = without_attr
            .strip_prefix("pub(crate) ")
            .or_else(|| without_attr.strip_prefix("pub(super) "))
            .or_else(|| without_attr.strip_prefix("pub "))
            .unwrap_or(without_attr);
        if let Some(rest) = stripped.strip_prefix("mod ") {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            let tail = rest[name.len()..].trim_start();
            if !name.is_empty() && tail.starts_with(';') {
                out.push((pending_path.take(), name));
                continue;
            }
        }
        if !line.starts_with("#[") {
            pending_path = None;
        }
    }
    out
}

/// `include!("relative.rs")` targets.
fn include_targets(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = source;
    while let Some(i) = rest.find("include!(\"") {
        let after = &rest[i + "include!(\"".len()..];
        if let Some(end) = after.find('"') {
            let target = &after[..end];
            if target.ends_with(".rs") && !target.contains("OUT_DIR") {
                out.push(target.to_string());
            }
            rest = &after[end..];
        } else {
            break;
        }
    }
    out
}

fn crate_roots(crate_dir: &Path) -> Vec<PathBuf> {
    let src = crate_dir.join("src");
    let mut roots = Vec::new();
    for candidate in ["lib.rs", "main.rs"] {
        let path = src.join(candidate);
        if path.exists() {
            roots.push(path);
        }
    }
    let bin = src.join("bin");
    if let Ok(entries) = fs::read_dir(&bin) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "rs") {
                roots.push(path);
            } else if path.join("main.rs").exists() {
                roots.push(path.join("main.rs"));
            }
        }
    }
    if let Ok(manifest) = fs::read_to_string(crate_dir.join("Cargo.toml")) {
        for line in manifest.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("path") {
                let rest = rest.trim_start().strip_prefix('=').unwrap_or("").trim();
                let value = rest.trim_matches('"');
                if value.ends_with(".rs") {
                    let path = crate_dir.join(value);
                    if path.exists() && normalize(&path).starts_with(normalize(&src)) {
                        roots.push(path);
                    }
                }
            }
        }
    }
    roots.into_iter().map(|p| normalize(&p)).collect()
}

fn reachable_sources(crate_dir: &Path) -> BTreeSet<PathBuf> {
    let roots = crate_roots(crate_dir);
    let root_set: BTreeSet<PathBuf> = roots.iter().cloned().collect();
    let mut seen = BTreeSet::new();
    let mut stack = roots;
    while let Some(file) = stack.pop() {
        if !seen.insert(file.clone()) || !file.exists() {
            continue;
        }
        let Ok(source) = fs::read_to_string(&file) else {
            continue;
        };
        let dir = file.parent().unwrap().to_path_buf();
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        let module_dir = if name == "lib.rs"
            || name == "main.rs"
            || name == "mod.rs"
            || root_set.contains(&file)
        {
            dir.clone()
        } else {
            dir.join(name.trim_end_matches(".rs"))
        };
        for target in include_targets(&source) {
            stack.push(normalize(&dir.join(&target)));
        }
        for (explicit, module) in mod_declarations(&source) {
            match explicit {
                Some(path) => {
                    // rustc resolves `#[path]` relative to the current file's
                    // directory for lib/main/mod.rs, else the module directory;
                    // accept either so a moved file cannot hide.
                    stack.push(normalize(&dir.join(&path)));
                    stack.push(normalize(&module_dir.join(&path)));
                }
                None => {
                    stack.push(normalize(&module_dir.join(format!("{module}.rs"))));
                    stack.push(normalize(&module_dir.join(&module).join("mod.rs")));
                }
            }
        }
    }
    seen
}

#[test]
fn every_source_file_under_src_is_reachable_from_a_compile_root() {
    let root = normalize(&workspace_root());
    let crates_dir = root.join("crates");
    let mut orphans: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in fs::read_dir(&crates_dir).expect("crates dir").flatten() {
        let crate_dir = normalize(&entry.path());
        let src = crate_dir.join("src");
        if !src.is_dir() {
            continue;
        }
        let reachable = reachable_sources(&crate_dir);
        let mut all = Vec::new();
        walk(&src, &mut all);
        for file in all {
            let file = normalize(&file);
            if file.extension().is_some_and(|e| e == "rs") && !reachable.contains(&file) {
                orphans
                    .entry(rel(&root, &crate_dir))
                    .or_default()
                    .push(rel(&root, &file));
            }
        }
    }
    assert!(
        orphans.is_empty(),
        "unreachable source files (never compiled, so never verified) — delete them or declare the module:\n{}",
        orphans
            .iter()
            .map(|(c, files)| format!("  {c}:\n{}", files.iter().map(|f| format!("    {f}")).collect::<Vec<_>>().join("\n")))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn no_editor_or_patch_artifacts_live_next_to_source() {
    let root = normalize(&workspace_root());
    let mut files = Vec::new();
    walk(&root.join("crates"), &mut files);
    let mut strays = Vec::new();
    for file in files {
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        let path = rel(&root, &file);
        let is_stray = STRAY_SUFFIXES.iter().any(|s| name.ends_with(s))
            || (name.contains(".rs.")
                && !ALLOWED_NON_SOURCE_SUFFIXES
                    .iter()
                    .any(|s| name.ends_with(s)));
        if is_stray && !path.contains("/tests/fixtures/") && !path.contains("/testdata/") {
            strays.push(path);
        }
    }
    assert!(
        strays.is_empty(),
        "artifacts committed next to verified source — delete them:\n{}",
        strays.join("\n")
    );
}

#[test]
fn the_gate_sees_through_path_attributes_and_includes() {
    let dir = tempfile::tempdir().unwrap();
    let crate_dir = dir.path().join("fixture");
    let src = crate_dir.join("src");
    fs::create_dir_all(src.join("nested")).unwrap();
    fs::write(
        crate_dir.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\n",
    )
    .unwrap();
    fs::write(
        src.join("lib.rs"),
        "mod plain;\n#[cfg(test)]\nmod tests;\n#[path = \"nested/renamed.rs\"]\nmod moved;\npub mod wrapper;\n",
    )
    .unwrap();
    fs::write(src.join("plain.rs"), "").unwrap();
    fs::write(src.join("tests.rs"), "").unwrap();
    fs::write(src.join("nested/renamed.rs"), "").unwrap();
    fs::write(src.join("wrapper.rs"), "include!(\"generated_body.rs\");\n").unwrap();
    fs::write(src.join("generated_body.rs"), "").unwrap();
    fs::write(src.join("orphan.rs"), "").unwrap();
    let reachable = reachable_sources(&normalize(&crate_dir));
    let names: BTreeSet<String> = reachable
        .iter()
        .map(|p| rel(&normalize(&crate_dir), p))
        .collect();
    for expected in [
        "src/lib.rs",
        "src/plain.rs",
        "src/tests.rs",
        "src/nested/renamed.rs",
        "src/wrapper.rs",
        "src/generated_body.rs",
    ] {
        assert!(
            names.contains(expected),
            "{expected} missing from {names:?}"
        );
    }
    assert!(!names.contains("src/orphan.rs"));
}
