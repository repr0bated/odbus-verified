//! Principal-bound capability-grant materializer.
//!
//! The durable document lives under `/etc`; consumers read a root-only atomic
//! projection in tmpfs. Validation happens before every projection so wildcard
//! and legacy footprint authority can never be published accidentally.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use op_grpc_bridge::mcp_policy::{parse_audience_policy, parse_toolset_manifest};
use serde_json::Value;

const DEFAULT_SOURCE: &str = "/etc/opdbus/capability-grants.json";
const DEFAULT_TARGET: &str = "/dev/shm/opdbus/capability-grants.json";
const DEFAULT_READY: &str = "/run/opdbus/runit-ready/opdbus-grants";
const INVALID_DOCUMENT: &[u8] = b"{}\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReconcileState {
    Fresh,
    Materialized,
}

fn prohibited_identity_key(key: &str) -> bool {
    key == "*" || (key.len() == 64 && key.as_bytes().iter().all(|byte| byte.is_ascii_hexdigit()))
}

fn validate_document(bytes: &[u8]) -> Result<()> {
    let document: Value = serde_json::from_slice(bytes).context("grant document is not JSON")?;
    let entries = document
        .as_object()
        .context("grant document must be a JSON object keyed by principal_id")?;

    for (principal_id, entry) in entries {
        if principal_id.is_empty() {
            bail!("grant document contains an empty principal_id");
        }
        if prohibited_identity_key(principal_id) {
            bail!("prohibited wildcard or legacy footprint grant key: {principal_id}");
        }
        let entry = entry
            .as_object()
            .with_context(|| format!("grant entry for {principal_id} must be an object"))?;
        let capabilities = entry
            .get("capabilities")
            .and_then(Value::as_array)
            .with_context(|| {
                format!("grant entry for {principal_id} must own a capabilities array")
            })?;
        let mut unique = HashSet::with_capacity(capabilities.len());
        for capability in capabilities {
            let capability = capability.as_str().with_context(|| {
                format!("grant entry for {principal_id} contains a non-string capability")
            })?;
            if capability.trim().is_empty() {
                bail!("grant entry for {principal_id} contains an empty capability");
            }
            if !unique.insert(capability) {
                bail!("grant entry for {principal_id} repeats capability {capability}");
            }
        }
        if let Some(alias) = entry.get("display_alias") {
            if !alias.is_string() {
                bail!("display_alias for {principal_id} must be a string");
            }
        }
    }
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("create projection directory {}", parent.display()))?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("projection path has no UTF-8 file name")?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(".{file_name}.{}.{nonce}.tmp", std::process::id()));

    let write_result = (|| -> Result<()> {
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&temporary)
            .with_context(|| format!("create temporary projection {}", temporary.display()))?;
        output
            .write_all(bytes)
            .with_context(|| format!("write temporary projection {}", temporary.display()))?;
        output
            .sync_all()
            .with_context(|| format!("sync temporary projection {}", temporary.display()))?;
        fs::rename(&temporary, path).with_context(|| {
            format!(
                "atomically replace {} with {}",
                path.display(),
                temporary.display()
            )
        })?;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .with_context(|| format!("set permissions on {}", path.display()))?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .with_context(|| format!("sync projection directory {}", parent.display()))?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result
}

fn remove_ready(ready: &Path) {
    match fs::remove_file(ready) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => eprintln!("grants materializer: remove {}: {error}", ready.display()),
    }
}

fn mark_ready(ready: &Path) -> Result<()> {
    atomic_write(ready, b"ready\n", 0o644)
}

fn read_valid_source(source: &Path) -> Result<Vec<u8>> {
    let bytes = fs::read(source)
        .with_context(|| format!("durable grant source unreadable: {}", source.display()))?;
    validate_document(&bytes)?;
    Ok(bytes)
}

fn reconcile(source: &Path, target: &Path, ready: &Path) -> Result<ReconcileState> {
    let source_bytes = match read_valid_source(source) {
        Ok(bytes) => bytes,
        Err(error) => {
            remove_ready(ready);
            if fs::read(target).ok().as_deref() != Some(INVALID_DOCUMENT) {
                atomic_write(target, INVALID_DOCUMENT, 0o600)
                    .context("publish fail-closed empty grant projection")?;
            }
            return Err(error);
        }
    };

    let target_fresh = fs::read(target)
        .map(|bytes| bytes == source_bytes)
        .unwrap_or(false);
    let target_mode = fs::metadata(target)
        .map(|metadata| metadata.permissions().mode() & 0o777)
        .ok();
    let state = if target_fresh && target_mode == Some(0o600) {
        ReconcileState::Fresh
    } else {
        atomic_write(target, &source_bytes, 0o600)?;
        ReconcileState::Materialized
    };
    mark_ready(ready)?;
    Ok(state)
}

fn check(source: &Path, target: &Path) -> Result<bool> {
    let source_bytes = read_valid_source(source)?;
    let target_bytes = fs::read(target)
        .with_context(|| format!("grant projection unreadable: {}", target.display()))?;
    validate_document(&target_bytes).context("grant projection is invalid")?;
    let mode = fs::metadata(target)?.permissions().mode() & 0o777;
    Ok(source_bytes == target_bytes && mode == 0o600)
}

fn paths() -> (PathBuf, PathBuf, PathBuf) {
    (
        std::env::var_os("OP_GRANTS_SOURCE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_SOURCE)),
        std::env::var_os("OP_GRANTS_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_TARGET)),
        std::env::var_os("OP_GRANTS_READY")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_READY)),
    )
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let command = args
        .next()
        .and_then(|value| value.into_string().ok())
        .unwrap_or_default();
    let (source, target, ready) = paths();

    match command.as_str() {
        "validate" => {
            let path = args
                .next()
                .map(PathBuf::from)
                .unwrap_or_else(|| source.clone());
            if args.next().is_some() {
                bail!("validate accepts at most one path");
            }
            validate_document(&fs::read(&path).with_context(|| path.display().to_string())?)?;
            println!("Valid");
        }
        "validate-audience" => {
            let path = args
                .next()
                .map(PathBuf::from)
                .context("validate-audience requires one path")?;
            if args.next().is_some() {
                bail!("validate-audience accepts exactly one path");
            }
            parse_audience_policy(&fs::read(&path).with_context(|| path.display().to_string())?)?;
            println!("Valid");
        }
        "validate-toolsets" => {
            let path = args
                .next()
                .map(PathBuf::from)
                .context("validate-toolsets requires one path")?;
            if args.next().is_some() {
                bail!("validate-toolsets accepts exactly one path");
            }
            parse_toolset_manifest(&fs::read(&path).with_context(|| path.display().to_string())?)?;
            println!("Valid");
        }
        "check" => {
            if args.next().is_some() {
                bail!("check accepts no arguments");
            }
            if check(&source, &target)? {
                println!("Fresh");
            } else {
                println!("Stale");
                std::process::exit(1);
            }
        }
        "once" => {
            if args.next().is_some() {
                bail!("once accepts no arguments");
            }
            println!("{:?}", reconcile(&source, &target, &ready)?);
        }
        "run" => {
            if args.next().is_some() {
                bail!("run accepts no arguments");
            }
            let mut last_message = String::new();
            loop {
                let message = match reconcile(&source, &target, &ready) {
                    Ok(ReconcileState::Fresh) => "Fresh".to_string(),
                    Ok(ReconcileState::Materialized) => "Grants materialized".to_string(),
                    Err(error) => format!("Fail-closed: {error:#}"),
                };
                if message != last_message {
                    eprintln!("grants materializer: {message}");
                    last_message = message;
                }
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => break,
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                }
            }
        }
        _ => bail!(
            "unknown command '{command}'; expected run, check, once, validate [path], validate-audience <path>, or validate-toolsets <path>"
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_exact_principal_grants() {
        validate_document(
            br#"{
                "937d6d2b-ecae-ed53-f3a2-d7bd09f544ff": {
                    "display_alias": "jeremy-laptop",
                    "capabilities": ["cognitive_mcp.read", "cognitive_mcp.invoke"]
                }
            }"#,
        )
        .unwrap();
    }

    #[test]
    fn rejects_wildcard_and_legacy_footprint_keys() {
        assert!(validate_document(br#"{"*":{"capabilities":[]}}"#).is_err());
        assert!(validate_document(
            br#"{
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef": {
                    "capabilities": []
                }
            }"#
        )
        .is_err());
    }

    #[test]
    fn invalid_source_publishes_empty_fail_closed_projection() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("durable.json");
        let target = root.path().join("shm/grants.json");
        let ready = root.path().join("run/ready");
        fs::write(&source, br#"{"*":{"capabilities":["admin"]}}"#).unwrap();
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, br#"{"principal":{"capabilities":["admin"]}}"#).unwrap();

        assert!(reconcile(&source, &target, &ready).is_err());
        assert_eq!(fs::read(target).unwrap(), INVALID_DOCUMENT);
        assert!(!ready.exists());
    }
}
