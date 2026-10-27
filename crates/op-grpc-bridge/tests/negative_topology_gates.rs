//! Negative topology gates: filesystem scans enforcing repudiated-mechanism
//! absence and inventory pins (validation-contract VAL-GATE-001..018).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const GATE_SELF_REL: &str = "crates/op-grpc-bridge/tests/negative_topology_gates.rs";

/// Repudiated mechanism tokens checked against `boundaries.md` (VAL-GATE-015).
const REPUDIATED_MECHANISM_DOC_TOKENS: &[&str] = &[
    "wg-lan",
    "op-identity-shuttle",
    "TransportBindingIndex",
    "NXM_NX_REG",
    "identity containers",
    "header injection",
];

/// Five repudiated-mechanism token families enforced in `crates/` (VAL-GATE-003..006).
const REPUDIATED_CRATES_TOKEN_FAMILIES: &[&str] =
    &["wg-lan", "TransportBindingIndex", "op-identity-shuttle"];

const OPENFLOW_COMPOUND_TOKENS: &[&str] = &[
    "identity_tag",
    "identity-tag",
    "peer_identity",
    "identity_flow",
];

const PINNED_PROTO_FILES: &[&str] = &[
    "crates/op-assistant-grpc/proto/assistant/agent.proto",
    "crates/op-assistant-grpc/proto/assistant/common.proto",
    "crates/op-assistant-grpc/proto/assistant/cron.proto",
    "crates/op-assistant-grpc/proto/assistant/memory.proto",
    "crates/op-assistant-grpc/proto/assistant/model.proto",
    "crates/op-assistant-grpc/proto/assistant/namespace.proto",
    "crates/op-assistant-grpc/proto/assistant/session.proto",
    "crates/op-assistant-grpc/proto/assistant/soul.proto",
    "crates/op-assistant-grpc/proto/assistant/task.proto",
    "crates/op-cache/proto/op_cache.proto",
    "crates/op-chat/proto/agents.proto",
    "crates/op-chat/proto/chat.proto",
    "crates/op-chat/proto/orchestration.proto",
    "crates/op-cognitive/proto/cognitive.proto",
    "crates/op-grpc-bridge/proto/emqx_exhook_v3.proto",
    "crates/op-grpc-bridge/proto/mail.proto",
    "crates/op-grpc-bridge/proto/operation.proto",
    "crates/op-grpc-bridge/proto/privacy_network.proto",
    "crates/op-grpc-bridge/proto/registration.proto",
    "crates/op-grpc-bridge/proto/registry.proto",
    "crates/op-grpc-bridge/src/grpc/chat_router.proto",
    "crates/op-mcp/proto/internal_agents.proto",
    "crates/op-mcp/proto/mcp.proto",
    "crates/op-services/proto/services.proto",
    "crates/op-xray-daemon/proto/app/log/command/config.proto",
    "crates/op-xray-daemon/proto/app/proxyman/command/command.proto",
    "crates/op-xray-daemon/proto/app/proxyman/config.proto",
    "crates/op-xray-daemon/proto/app/router/command/command.proto",
    "crates/op-xray-daemon/proto/app/stats/command/command.proto",
    "crates/op-xray-daemon/proto/common/net/address.proto",
    "crates/op-xray-daemon/proto/common/net/network.proto",
    "crates/op-xray-daemon/proto/common/net/port.proto",
    "crates/op-xray-daemon/proto/common/protocol/headers.proto",
    "crates/op-xray-daemon/proto/common/protocol/server_spec.proto",
    "crates/op-xray-daemon/proto/common/protocol/user.proto",
    "crates/op-xray-daemon/proto/common/serial/typed_message.proto",
    "crates/op-xray-daemon/proto/core/config.proto",
    "crates/op-xray-daemon/proto/proxy/trojan/config.proto",
    "crates/op-xray-daemon/proto/proxy/vless/account.proto",
    "crates/op-xray-daemon/proto/proxy/vless/inbound/config.proto",
    "crates/op-xray-daemon/proto/proxy/vmess/account.proto",
    "crates/op-xray-daemon/proto/proxy/vmess/inbound/config.proto",
    "crates/op-xray-daemon/proto/transport/internet/config.proto",
    "crates/op-xray-daemon/proto/transport/internet/tls/config.proto",
];

const PINNED_GRPC_PACKAGES: &[&str] = &[
    "assistant.v1",
    "emqx.exhook.v3",
    "op.mcp.v1",
    "op_agents",
    "op_cache",
    "op_chat.agents",
    "op_chat.chat",
    "op_chat.orchestration",
    "opdbus.services.v1",
    "operation.cognitive.v1",
    "operation.mail.v1",
    "operation.privacy.v1",
    "operation.registration.v1",
    "operation.registry.v1",
    "operation.v1",
    "chat_router",
    "xray.app.log.command",
    "xray.app.proxyman",
    "xray.app.proxyman.command",
    "xray.app.router.command",
    "xray.app.stats.command",
    "xray.common.net",
    "xray.common.protocol",
    "xray.common.serial",
    "xray.core",
    "xray.proxy.trojan",
    "xray.proxy.vless",
    "xray.proxy.vless.inbound",
    "xray.proxy.vmess",
    "xray.proxy.vmess.inbound",
    "xray.transport.internet",
    "xray.transport.internet.tls",
];

/// Packages that declare a gRPC service, separate from message-only dependency
/// packages. This pin keeps new service surfaces visible even when a vendored
/// protocol dependency closure legitimately expands.
const PINNED_GRPC_SERVICE_PACKAGES: &[&str] = &[
    "assistant.v1",
    "emqx.exhook.v3",
    "op.mcp.v1",
    "op_agents",
    "op_cache",
    "op_chat.agents",
    "op_chat.chat",
    "op_chat.orchestration",
    "opdbus.services.v1",
    "operation.cognitive.v1",
    "operation.mail.v1",
    "operation.privacy.v1",
    "operation.registration.v1",
    "operation.registry.v1",
    "operation.v1",
    "chat_router",
    "xray.app.log.command",
    "xray.app.proxyman.command",
    "xray.app.router.command",
    "xray.app.stats.command",
];

const PINNED_PYTHON_FILES: &[&str] = &[
    "scripts/check_qdrant.py",
    "scripts/export-llm-sessions-to-notebooklm-sources.py",
    "scripts/gemini-mcp-server.py",
    "scripts/generate_llm_specs.py",
    "scripts/intercept-and-exchange-oauth.py",
    "scripts/notebook-sources-cleanup.py",
    "scripts/or-fusion-archive.py",
    "scripts/oscal-vectorize.py",
    "scripts/query-vertex-billing-api.py",
    "scripts/runit-tui.py",
    "scripts/scan-codebase.py",
    "scripts/setup-gemini-oauth.py",
    "scripts/test-antigravity-gemini.py",
    "scripts/uv-tools/notebook_sync.py",
    "scripts/uv-tools/test_notebook_sync.py",
    "scripts/vectorize/vectorize_code.py",
    "scripts/vectorize/vectorize_compliance.py",
    "scripts/vectorize/vectorize_lsp.py",
    "scripts/vectorize_oscal.py",
    "scripts/vectorize_rust_repos.py",
];

const COMMAND_NEW_PATTERNS: &[&str] = &[
    "Command::new",
    "std::process::Command",
    "tokio::process::Command",
];

const BACKGROUND_FORBIDDEN: &[&str] = &[
    "tokio::spawn",
    "task::spawn",
    "std::thread::spawn",
    "thread::Builder",
    "tokio::time::interval",
    "tokio::time::sleep",
    "std::thread::sleep",
    "add_match",
    "receive_signal",
    "watch::",
    "notify::",
];

const SPAWN_BLOCKING_ALLOWED: &str = "crates/op-grpc-bridge/src/human_principal_dispatch.rs";

const IDENTITY_ANCHORS: &[&str] = &[
    "crates/op-grpc-bridge/src/interceptor.rs",
    "crates/op-identity/src/session.rs",
];

const MIN_CRATES_FILE_FLOOR: usize = 500;

#[derive(Debug, Clone)]
struct Violation {
    path: String,
    detail: String,
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn is_gate_self(rel: &str) -> bool {
    rel == GATE_SELF_REL
}

fn should_skip_tree_entry(name: &str) -> bool {
    name == "target" || name == ".git"
}

fn collect_files_under(root: &Path, subdir: &str) -> Result<Vec<PathBuf>, String> {
    let base = root.join(subdir);
    if !base.is_dir() {
        return Err(format!("missing scan directory: {}", base.display()));
    }
    let mut files = Vec::new();
    collect_files_recursive(&base, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read_dir entry {}: {e}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|e| format!("file_type {}: {e}", path.display()))?;
        if file_type.is_dir() {
            if should_skip_tree_entry(&entry.file_name().to_string_lossy()) {
                continue;
            }
            collect_files_recursive(&path, out)?;
        } else if file_type.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

fn read_text(path: &Path) -> Result<String, String> {
    match fs::read(path) {
        Ok(bytes) => {
            String::from_utf8(bytes).map_err(|_| format!("skip non-utf8 file: {}", path.display()))
        }
        Err(e) => Err(format!("read {}: {e}", path.display())),
    }
}

fn read_text_or_skip(path: &Path) -> Option<String> {
    read_text(path).ok()
}

/// Shared scanner entry point for token-family gates and the self-test (VAL-GATE-011).
fn scan_crates_for_literal_token(
    root: &Path,
    token: &str,
    exclude_gate_self: bool,
) -> Result<Vec<Violation>, String> {
    let files = collect_files_under(root, "crates")?;
    let mut violations = Vec::new();
    for path in files {
        let rel = rel_path(root, &path);
        if exclude_gate_self && is_gate_self(&rel) {
            continue;
        }
        let Some(text) = read_text_or_skip(&path) else {
            continue;
        };
        if text.contains(token) {
            violations.push(Violation {
                path: rel,
                detail: format!("forbidden token `{token}`"),
            });
        }
    }
    Ok(violations)
}

fn assert_scan_root_sane(root: &Path) -> Result<usize, String> {
    let cargo_toml = root.join("Cargo.toml");
    if !cargo_toml.is_file() {
        return Err(format!(
            "resolved workspace root missing Cargo.toml: {}",
            root.display()
        ));
    }
    let files = collect_files_under(root, "crates")?;
    if files.is_empty() {
        return Err("crates/ scan set is empty".into());
    }
    if files.len() < MIN_CRATES_FILE_FLOOR {
        return Err(format!(
            "crates/ scan set too small ({} files, floor {MIN_CRATES_FILE_FLOOR})",
            files.len()
        ));
    }
    for sentinel in [
        "crates/op-identity/src/lib.rs",
        "crates/op-grpc-bridge/src/lib.rs",
    ] {
        if !root.join(sentinel).is_file() {
            return Err(format!("missing sentinel file: {sentinel}"));
        }
    }
    Ok(files.len())
}

fn strip_cfg_test_modules(source: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < source.len() {
        if source[i..].starts_with("#[cfg(test)]") {
            i += "#[cfg(test)]".len();
            while i < source.len() && source.as_bytes()[i].is_ascii_whitespace() {
                i += 1;
            }
            if source[i..].starts_with("mod ")
                && let Some(end) = find_balanced_block_end(source, i)
            {
                i = end;
                continue;
            }
        }
        let ch = source[i..].chars().next().expect("valid utf-8");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn find_balanced_block_end(source: &str, start: usize) -> Option<usize> {
    let brace_start = source[start..].find('{')? + start;
    let mut depth = 0usize;
    for (offset, ch) in source[brace_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(brace_start + offset + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn glob_identity_src_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let src_root = root.join("crates");
    if !src_root.is_dir() {
        return Err("missing crates/".into());
    }
    let mut matched = Vec::new();
    collect_glob_matches(&src_root, &mut matched)?;
    matched.sort();
    Ok(matched)
}

fn collect_glob_matches(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("read_dir entry: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            if should_skip_tree_entry(&entry.file_name().to_string_lossy()) {
                continue;
            }
            collect_glob_matches(&path, out)?;
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name.contains("oracle_assertion") || name.contains("human_principal") {
            let components: Vec<_> = path.components().map(|c| c.as_os_str()).collect();
            if components.iter().any(|c| *c == "src") {
                out.push(path);
            }
        }
    }
    Ok(())
}

fn identity_command_new_scope(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = glob_identity_src_files(root)?;
    for anchor in IDENTITY_ANCHORS {
        let p = root.join(anchor);
        if !p.is_file() {
            return Err(format!("missing identity anchor: {anchor}"));
        }
        if !paths.iter().any(|existing| existing == &p) {
            paths.push(p);
        }
    }
    if paths.is_empty() {
        return Err("identity glob scope is empty".into());
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn scan_openflow_identity_tagging(root: &Path) -> Result<Vec<Violation>, String> {
    let files = collect_files_under(root, "crates")?;
    let mut violations = Vec::new();
    for path in files {
        let rel = rel_path(root, &path);
        if is_gate_self(&rel) {
            continue;
        }
        let Some(text) = read_text_or_skip(&path) else {
            continue;
        };
        let lower = text.to_ascii_lowercase();
        for token in OPENFLOW_COMPOUND_TOKENS {
            if lower.contains(token) {
                violations.push(Violation {
                    path: rel.clone(),
                    detail: format!("forbidden OpenFlow compound token `{token}`"),
                });
            }
        }
        for (line_no, line) in text.lines().enumerate() {
            if !line.contains("NXM_NX_REG") {
                continue;
            }
            let line_lower = line.to_ascii_lowercase();
            if line_lower.contains("identity")
                || line_lower.contains("pubkey")
                || line_lower.contains("footprint")
                || line_lower.contains("principal")
                || line_lower.contains("session")
            {
                violations.push(Violation {
                    path: rel.clone(),
                    detail: format!(
                        "NXM_NX_REG identity-coupled line {}: {}",
                        line_no + 1,
                        line.trim()
                    ),
                });
            }
        }
    }
    Ok(violations)
}

fn inventory_set(
    root: &Path,
    subdirs: &[&str],
    extension: &str,
) -> Result<BTreeSet<String>, String> {
    let mut set = BTreeSet::new();
    for sub in subdirs {
        let base = root.join(sub);
        if !base.is_dir() {
            continue;
        }
        let files = collect_files_under(root, sub)?;
        for path in files {
            if path.extension().and_then(|e| e.to_str()) != Some(extension) {
                continue;
            }
            let rel = rel_path(root, &path);
            if rel.contains("/.venv/") || rel.contains("/target/") || rel.contains("/node_modules/")
            {
                continue;
            }
            set.insert(rel);
        }
    }
    Ok(set)
}

fn collect_grpc_packages(root: &Path) -> Result<BTreeSet<String>, String> {
    let protos = inventory_set(root, &["crates"], "proto")?;
    let mut packages = BTreeSet::new();
    for rel in protos {
        let text = read_text(&root.join(&rel))?;
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("package ") {
                let name = rest.trim_end_matches(';').trim();
                if !name.is_empty() {
                    packages.insert(name.to_string());
                }
            }
        }
    }
    Ok(packages)
}

fn collect_grpc_service_packages(root: &Path) -> Result<BTreeSet<String>, String> {
    let protos = inventory_set(root, &["crates"], "proto")?;
    let mut packages = BTreeSet::new();
    for rel in protos {
        let text = read_text(&root.join(&rel))?;
        let package = text.lines().find_map(|line| {
            line.trim()
                .strip_prefix("package ")
                .map(|rest| rest.trim_end_matches(';').trim())
                .filter(|name| !name.is_empty())
        });
        let declares_service = text.lines().any(|line| {
            line.trim()
                .strip_prefix("service ")
                .and_then(|rest| rest.chars().next())
                .is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        });
        if declares_service {
            let package = package
                .ok_or_else(|| format!("gRPC service declaration without package in {rel}"))?;
            packages.insert(package.to_string());
        }
    }
    Ok(packages)
}

fn scan_identity_command_new(root: &Path) -> Result<Vec<Violation>, String> {
    let paths = identity_command_new_scope(root)?;
    let mut violations = Vec::new();
    for path in paths {
        let rel = rel_path(root, &path);
        let Some(text) = read_text_or_skip(&path) else {
            continue;
        };
        for pattern in COMMAND_NEW_PATTERNS {
            if text.contains(pattern) {
                violations.push(Violation {
                    path: rel.clone(),
                    detail: format!("forbidden subprocess pattern `{pattern}`"),
                });
            }
        }
    }
    Ok(violations)
}

fn line_matches_xray_daemon_forbidden(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    const KEYWORDS: &[&str] = &[
        "identity",
        "session",
        "assertion",
        "footprint",
        "principal",
        "x-ghostbridge-footprint",
        "x-wireguard-pubkey",
        "oia1",
        "x-oracle-identity-assertion-bin",
    ];
    KEYWORDS.iter().any(|kw| lower.contains(kw))
}

fn scan_xray_daemon(root: &Path) -> Result<Vec<Violation>, String> {
    let base = root.join("crates/op-xray-daemon/src");
    if !base.is_dir() {
        return Err("missing crates/op-xray-daemon/src".into());
    }
    let mut files = Vec::new();
    collect_files_recursive(&base, &mut files)?;
    let mut violations = Vec::new();
    for path in files {
        let rel = rel_path(root, &path);
        let Some(text) = read_text_or_skip(&path) else {
            continue;
        };
        for (line_no, line) in text.lines().enumerate() {
            if line_matches_xray_daemon_forbidden(line) {
                violations.push(Violation {
                    path: rel.clone(),
                    detail: format!(
                        "forbidden identity vocabulary line {}: {}",
                        line_no + 1,
                        line.trim()
                    ),
                });
            }
        }
    }
    Ok(violations)
}

fn scrub_permitted_background_constructs(text: &str) -> String {
    text.replace(
        "tokio::task::spawn_blocking",
        "tokio::task::blocked_offload",
    )
    .replace("task::spawn_blocking", "task::blocked_offload")
}

fn scan_background_tasks(root: &Path) -> Result<Vec<Violation>, String> {
    let paths = identity_command_new_scope(root)?;
    let mut violations = Vec::new();
    for path in paths {
        let rel = rel_path(root, &path);
        let Some(text) = read_text_or_skip(&path) else {
            continue;
        };
        let stripped = strip_cfg_test_modules(&text);
        let had_spawn_blocking = stripped.contains("spawn_blocking");
        let text = scrub_permitted_background_constructs(&stripped);
        for pattern in BACKGROUND_FORBIDDEN {
            if text.contains(pattern) {
                violations.push(Violation {
                    path: rel.clone(),
                    detail: format!("forbidden background construct `{pattern}`"),
                });
            }
        }
        if had_spawn_blocking && rel != SPAWN_BLOCKING_ALLOWED {
            violations.push(Violation {
                path: rel.clone(),
                detail: "spawn_blocking outside allowed file".into(),
            });
        }
    }
    Ok(violations)
}

fn scan_xray_in_identity_glob(root: &Path) -> Result<Vec<Violation>, String> {
    let paths = glob_identity_src_files(root)?;
    if paths.is_empty() {
        return Err("xray-agnostic glob scope is empty".into());
    }
    let mut violations = Vec::new();
    for path in paths {
        let rel = rel_path(root, &path);
        let Some(text) = read_text_or_skip(&path) else {
            continue;
        };
        let lower = text.to_ascii_lowercase();
        if lower.contains("xray") {
            violations.push(Violation {
                path: rel,
                detail: "identity path must remain xray-agnostic".into(),
            });
        }
    }
    Ok(violations)
}

fn assert_violations_empty(label: &str, violations: Vec<Violation>) {
    if violations.is_empty() {
        return;
    }
    let mut msg = format!("{label} failed with {} violation(s):\n", violations.len());
    for v in violations {
        msg.push_str(&format!("  {} - {}\n", v.path, v.detail));
    }
    panic!("{msg}");
}

fn boundaries_path(root: &Path) -> PathBuf {
    root.join("docs/src/architecture/identity-boundaries.md")
}

fn ttl_900s_in_issuance_context(text: &str) -> bool {
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if !lower.contains("900 s") {
            continue;
        }
        if lower.contains("ttl") || lower.contains("issuance") || lower.contains("lifetime") {
            return true;
        }
    }
    false
}

#[test]
fn scan_root_non_vacuous() {
    let root = workspace_root()
        .canonicalize()
        .expect("canonicalize workspace root");
    let count = assert_scan_root_sane(&root).expect("scan root sanity");
    eprintln!("negative_topology_gates: scanned {count} files under crates/");
    let empty = tempfile::tempdir().expect("tempdir");
    let err = assert_scan_root_sane(empty.path()).unwrap_err();
    assert!(
        err.contains("Cargo.toml") || err.contains("empty") || err.contains("missing"),
        "empty root must hard-fail: {err}"
    );
}

#[test]
fn retired_waypipe_is_not_a_build_or_proto_input() {
    let root = workspace_root().canonicalize().unwrap();
    assert!(!root.join("crates/op-waypipe-grpc/Cargo.toml").exists());
    for manifest in ["Cargo.toml", "Cargo.lock"] {
        let text = fs::read_to_string(root.join(manifest)).unwrap();
        assert!(
            !text.contains("op-waypipe-grpc"),
            "retired Waypipe in {manifest}"
        );
    }
    assert!(!PINNED_GRPC_PACKAGES.contains(&"op.waypipe.v1"));
    assert!(!PINNED_GRPC_SERVICE_PACKAGES.contains(&"op.waypipe.v1"));
}

#[test]
fn host_is_never_a_wireguard_peer_of_the_decoy() {
    // The former `mkdrt` runit service made this host a peer on the decoy's
    // identity WireGuard (inside WARP). Identity is minted at the human's
    // WireGuard ingress on the decoy and carried by the client over WARP;
    // the host only validates. No host-side WireGuard peer link may return.
    let root = workspace_root();
    assert!(
        !root.join("deploy/runit/mkdrt").exists(),
        "deploy/runit/mkdrt must stay retired"
    );
    let enabled = fs::read_to_string(root.join("deploy/runit/enabled-services")).unwrap();
    assert!(
        !enabled.lines().any(|line| line.trim() == "mkdrt"),
        "mkdrt must not be an enabled service"
    );
    let retired = fs::read_to_string(root.join("deploy/runit/retired-services")).unwrap();
    assert!(
        retired.lines().any(|line| line.trim() == "mkdrt"),
        "mkdrt must be listed in retired-services"
    );
    for run in ["deploy/runit/op-grpc-bridge/run", "deploy/runit/op-web/run"] {
        let text = fs::read_to_string(root.join(run)).unwrap();
        assert!(!text.contains("mkdrt"), "{run} still references mkdrt");
    }
}

#[test]
fn retired_standalone_adapters_are_not_build_or_proto_inputs() {
    let root = workspace_root().canonicalize().unwrap();
    assert!(!root.join("crates/op-grpc-adapters/Cargo.toml").exists());
    for manifest in [
        "Cargo.toml",
        "Cargo.lock",
        "crates/op-grpc-bridge/Cargo.toml",
    ] {
        let text = fs::read_to_string(root.join(manifest)).unwrap();
        assert!(
            !text.contains("op-grpc-adapters"),
            "retired standalone adapters in {manifest}"
        );
    }
    assert!(!PINNED_GRPC_PACKAGES.contains(&"op.adapters.v1"));
    assert!(!PINNED_GRPC_SERVICE_PACKAGES.contains(&"op.adapters.v1"));
}

#[test]
fn forbidden_token_wg_lan_absent() {
    let root = workspace_root().canonicalize().unwrap();
    let violations = scan_crates_for_literal_token(&root, "wg-lan", true).unwrap();
    assert_violations_empty("wg-lan", violations);
}

#[test]
fn forbidden_token_transport_binding_index_absent() {
    let root = workspace_root().canonicalize().unwrap();
    let violations = scan_crates_for_literal_token(&root, "TransportBindingIndex", true).unwrap();
    assert_violations_empty("TransportBindingIndex", violations);
}

#[test]
fn op_identity_shuttle_absent() {
    let root = workspace_root().canonicalize().unwrap();
    let violations = scan_crates_for_literal_token(&root, "op-identity-shuttle", true).unwrap();
    assert_violations_empty("op-identity-shuttle", violations);
}

#[test]
fn no_per_peer_openflow_identity_tagging() {
    let root = workspace_root().canonicalize().unwrap();
    let violations = scan_openflow_identity_tagging(&root).unwrap();
    assert_violations_empty("OpenFlow identity tagging", violations);
}

#[test]
fn proto_inventory_matches_pinned_baseline() {
    let root = workspace_root().canonicalize().unwrap();
    let found = inventory_set(&root, &["crates"], "proto").unwrap();
    let expected: BTreeSet<String> = PINNED_PROTO_FILES
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    assert_eq!(found, expected, "proto inventory drift");
}

#[test]
fn grpc_package_set_matches_pinned_baseline() {
    let root = workspace_root().canonicalize().unwrap();
    let found = collect_grpc_packages(&root).unwrap();
    let expected: BTreeSet<_> = PINNED_GRPC_PACKAGES
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    assert_eq!(found, expected, "gRPC package set drift");
}

#[test]
fn grpc_service_package_set_matches_pinned_baseline() {
    let root = workspace_root().canonicalize().unwrap();
    let found = collect_grpc_service_packages(&root).unwrap();
    let expected: BTreeSet<_> = PINNED_GRPC_SERVICE_PACKAGES
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    assert_eq!(found, expected, "gRPC service package set drift");
}

#[test]
fn grpc_service_scanner_detects_new_authored_service_package() {
    let fixture = tempfile::tempdir().unwrap();
    let proto_dir = fixture.path().join("crates/demo/proto");
    fs::create_dir_all(&proto_dir).unwrap();
    fs::write(
        proto_dir.join("messages.proto"),
        "syntax = \"proto3\";\npackage fixture.messages.v1;\nmessage Item {}\n",
    )
    .unwrap();
    fs::write(
        proto_dir.join("authored.proto"),
        "syntax = \"proto3\";\npackage fixture.authored.v1;\nservice NewSurface {}\n",
    )
    .unwrap();

    let found = collect_grpc_service_packages(fixture.path()).unwrap();
    assert_eq!(found, BTreeSet::from(["fixture.authored.v1".to_string()]));
}

#[test]
fn no_command_new_in_identity_paths() {
    let root = workspace_root().canonicalize().unwrap();
    let violations = scan_identity_command_new(&root).unwrap();
    assert_violations_empty("Command::new in identity paths", violations);
}

#[test]
fn op_xray_daemon_contains_no_identity_logic() {
    let root = workspace_root().canonicalize().unwrap();
    let violations = scan_xray_daemon(&root).unwrap();
    assert_violations_empty("op-xray-daemon identity vocabulary", violations);
}

#[test]
fn scanner_self_test_trips_on_forbidden_token() {
    let clean = tempfile::tempdir().unwrap();
    fs::create_dir_all(clean.path().join("crates/demo/src")).unwrap();
    fs::write(clean.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    fs::create_dir_all(clean.path().join("crates/op-identity/src")).unwrap();
    fs::write(
        clean.path().join("crates/op-identity/src/lib.rs"),
        "// sentinel\n",
    )
    .unwrap();
    fs::create_dir_all(clean.path().join("crates/op-grpc-bridge/src")).unwrap();
    fs::write(
        clean.path().join("crates/op-grpc-bridge/src/lib.rs"),
        "// sentinel\n",
    )
    .unwrap();
    fs::write(
        clean.path().join("crates/demo/src/demo.rs"),
        "// clean file\n",
    )
    .unwrap();

    let clean_v = scan_crates_for_literal_token(clean.path(), "wg-lan", false).unwrap();
    assert!(clean_v.is_empty(), "clean fixture must not trip");

    let dirty = tempfile::tempdir().unwrap();
    fs::create_dir_all(dirty.path().join("crates/demo/src")).unwrap();
    fs::write(dirty.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    fs::create_dir_all(dirty.path().join("crates/op-identity/src")).unwrap();
    fs::write(
        dirty.path().join("crates/op-identity/src/lib.rs"),
        "// sentinel\n",
    )
    .unwrap();
    fs::create_dir_all(dirty.path().join("crates/op-grpc-bridge/src")).unwrap();
    fs::write(
        dirty.path().join("crates/op-grpc-bridge/src/lib.rs"),
        "// sentinel\n",
    )
    .unwrap();
    fs::write(
        dirty.path().join("crates/demo/src/demo.rs"),
        "let iface = \"wg-lan\";\n",
    )
    .unwrap();

    let dirty_v = scan_crates_for_literal_token(dirty.path(), "wg-lan", false).unwrap();
    assert!(!dirty_v.is_empty(), "dirty fixture must trip");
    assert!(dirty_v.iter().any(|v| v.detail.contains("wg-lan")));
    assert!(dirty_v.iter().any(|v| v.path.contains("demo.rs")));

    let empty_err = assert_scan_root_sane(clean.path()).unwrap_err();
    assert!(!empty_err.is_empty());
}

#[test]
fn boundary_documentation_covers_external_families() {
    let root = workspace_root().canonicalize().unwrap();
    let doc = boundaries_path(&root);
    assert!(doc.is_file(), "boundaries.md missing");
    let text = read_text(&doc).unwrap();

    let required = [
        "sole incoming WireGuard termination",
        "No NetMaker transport remains",
        "standalone `wgcf-ingress`",
        "trusted-decoy-signature",
        "Passthrough only",
        "/etc/xray/xray_config.json",
        "OP_DECOY_TRUST_STORE",
        "trust store",
    ];
    for phrase in required {
        assert!(
            text.contains(phrase),
            "boundaries.md missing phrase: {phrase}"
        );
    }
    assert!(
        ttl_900s_in_issuance_context(&text),
        "boundaries.md missing TTL <= 900 s issuance context"
    );

    let sections: Vec<&str> = text.split("### ").collect();
    let mut family_markers = 0usize;
    for section in &sections {
        let upper = section.to_ascii_uppercase();
        let has_external = upper.contains("EXTERNAL") || section.contains("NOT deployed");
        let covers_a = section.contains("sole incoming WireGuard termination")
            || section.contains("WireGuard tunnel");
        let covers_b =
            section.contains("Exactly one WARP") || section.contains("trusted-decoy-signature");
        let covers_c =
            section.contains("Passthrough only") || section.contains("/etc/xray/xray_config.json");
        let covers_d = section.contains("OP_DECOY_TRUST_STORE") || section.contains("trust store");
        if has_external && (covers_a || covers_b || covers_c || covers_d) {
            family_markers += 1;
        }
    }
    assert!(
        family_markers >= 3,
        "expected EXTERNAL/not-deployed markers for external assumption families, got {family_markers}"
    );
}

#[test]
fn documentation_gate_token_set_consistency() {
    let root = workspace_root().canonicalize().unwrap();
    let text = read_text(&boundaries_path(&root)).unwrap();
    for token in REPUDIATED_MECHANISM_DOC_TOKENS {
        assert!(text.contains(token), "boundaries.md missing token: {token}");
    }
    for token in REPUDIATED_CRATES_TOKEN_FAMILIES {
        assert!(
            text.contains(token),
            "boundaries.md missing crates token family: {token}"
        );
    }
}

#[test]
fn no_background_tasks_in_identity_path() {
    let root = workspace_root().canonicalize().unwrap();
    let violations = scan_background_tasks(&root).unwrap();
    assert_violations_empty("background tasks in identity path", violations);
}

#[test]
fn identity_path_is_xray_agnostic() {
    let root = workspace_root().canonicalize().unwrap();
    let violations = scan_xray_in_identity_glob(&root).unwrap();
    assert_violations_empty("xray token in identity glob", violations);
}

#[test]
fn python_inventory_matches_pinned_baseline() {
    let root = workspace_root().canonicalize().unwrap();
    let found = inventory_set(&root, &["crates", "scripts"], "py").unwrap();
    let expected: BTreeSet<String> = PINNED_PYTHON_FILES
        .iter()
        // The source exporter omits these historical credential-bearing
        // scripts. They may remain in the original checkout, but are not
        // required build inputs. Every other pinned path remains mandatory,
        // and paths outside the pinned set are still rejected.
        .filter(|path| {
            !matches!(
                **path,
                "scripts/test-antigravity-gemini.py" | "scripts/vectorize_rust_repos.py"
            ) || found.contains(**path)
        })
        .map(|s| (*s).to_string())
        .collect();
    assert_eq!(found, expected, "python inventory drift");
}
