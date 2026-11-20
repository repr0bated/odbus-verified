//! Principal-bound capability-grant materializer.
//!
//! The durable document lives under `/etc`; consumers read a root-only atomic
//! projection in tmpfs. Validation happens before every projection so wildcard
//! and legacy footprint authority can never be published accidentally.
//!
//! `generate` unions every sealed-catalog `required_capability` into `$ROOT`.
//! Without `--apply-to-live` it only prints a plan. With `--apply-to-live` it
//! backs up the durable file, writes only if `$ROOT` is missing catalog caps,
//! and leaves projection to the `opdbus-grants` run loop. It never assigns
//! caps to other groups and never writes SHM itself.

use std::collections::{BTreeSet, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
// The materializer uses the shared policy parsers, not the mutation API.
#[allow(dead_code)]
#[path = "../mcp_policy.rs"]
mod mcp_policy;
#[allow(dead_code)]
#[path = "../sealed_schema_reader.rs"]
mod sealed_schema_reader;

use mcp_policy::{parse_audience_policy, parse_toolset_manifest};

const DEFAULT_SOURCE: &str = "/etc/opdbus/capability-grants.json";
const DEFAULT_TARGET: &str = "/dev/shm/opdbus/capability-grants.json";
const DEFAULT_READY: &str = "/run/opdbus/runit-ready/opdbus-grants";
const DEFAULT_CATALOG: &str = "/dev/shm/opdbus/plugin-blobs";
const PERSISTED_CATALOG: &str = "/var/lib/opdbus/plugin-blobs";
const ROOT_GROUP: &str = "$ROOT";
const INVALID_DOCUMENT: &[u8] = b"{}\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReconcileState {
    Fresh,
    Materialized,
}

fn validate_document(bytes: &[u8]) -> Result<()> {
    op_grpc_bridge::grant_document::validate_grant_document(bytes)
        .map_err(|error| anyhow::anyhow!(error))
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct GenerateReport {
    added: usize,
    already: usize,
    catalog: usize,
}

fn backup_stamp() -> String {
    if let Ok(output) = std::process::Command::new("date")
        .arg("+%Y%m%d-%H%M%S")
        .output()
        && output.status.success()
    {
        let stamp = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !stamp.is_empty() {
            return stamp;
        }
    }
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn backup_path(source: &Path) -> PathBuf {
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("capability-grants.json");
    let parent = source.parent().unwrap_or_else(|| Path::new("."));
    let mut path = parent.join(format!("{name}.bak-{}", backup_stamp()));
    let mut n = 1u32;
    while path.exists() {
        path = parent.join(format!("{name}.bak-{}-{n}", backup_stamp()));
        n += 1;
    }
    path
}

fn resolve_catalog(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        if path.is_dir() {
            return Ok(path);
        }
        bail!("catalog directory missing: {}", path.display());
    }
    if let Some(path) = std::env::var_os("OP_CATALOG_DIR").map(PathBuf::from) {
        if path.is_dir() {
            return Ok(path);
        }
        bail!("OP_CATALOG_DIR is not a directory: {}", path.display());
    }
    for candidate in [DEFAULT_CATALOG, PERSISTED_CATALOG] {
        let path = PathBuf::from(candidate);
        if path.join(".manifest.json").is_file() || path.is_dir() {
            // Prefer a directory that actually contains blobs.
            if path
                .read_dir()
                .ok()
                .map(|entries| {
                    entries.filter_map(Result::ok).any(|entry| {
                        entry.path().extension().and_then(|ext| ext.to_str()) == Some("blob")
                    })
                })
                .unwrap_or(false)
            {
                return Ok(path);
            }
        }
    }
    bail!(
        "no sealed catalog found (tried {DEFAULT_CATALOG} and {PERSISTED_CATALOG}; pass --catalog or OP_CATALOG_DIR)"
    )
}

fn catalog_capabilities(dir: &Path) -> Result<BTreeSet<String>> {
    anyhow::ensure!(dir.is_dir(), "catalog directory missing: {}", dir.display());
    let mut caps = BTreeSet::new();
    let mut blobs = 0u32;
    for entry in fs::read_dir(dir)
        .with_context(|| format!("read catalog {}", dir.display()))?
    {
        let path = entry
            .with_context(|| format!("read catalog entry in {}", dir.display()))?
            .path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("blob") {
            continue;
        }
        let bytes = fs::read(&path)
            .with_context(|| format!("read plugin blob {}", path.display()))?;
        let blob = op_blob::blob::BlobRef::new(&bytes)
            .map_err(|error| anyhow::anyhow!("invalid blob {}: {error}", path.display()))?;
        let manifest = blob
            .manifest()
            .map_err(|error| anyhow::anyhow!("blob manifest {}: {error}", path.display()))?;
        blobs += 1;
        for method in manifest.methods {
            if let Some(cap) = method.required_capability {
                if !cap.trim().is_empty() {
                    caps.insert(cap);
                }
            }
        }
    }
    if blobs == 0 {
        bail!("no plugin blobs in {}", dir.display());
    }
    if caps.is_empty() {
        bail!(
            "catalog at {} has no required_capability values",
            dir.display()
        );
    }
    Ok(caps)
}

fn generate_root_grants(
    document: &[u8],
    catalog_caps: &BTreeSet<String>,
) -> Result<(Vec<u8>, GenerateReport)> {
    validate_document(document)?;
    let mut value: serde_json::Value = serde_json::from_slice(document)
        .context("grant document is not JSON")?;
    let root = value
        .get_mut(ROOT_GROUP)
        .and_then(serde_json::Value::as_object_mut)
        .with_context(|| format!("grant document has no {ROOT_GROUP} group"))?;
    let capabilities = root
        .get_mut("capabilities")
        .and_then(serde_json::Value::as_array_mut)
        .with_context(|| format!("{ROOT_GROUP} must own a capabilities array"))?;
    let existing: HashSet<String> = capabilities
        .iter()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_string)
        .collect();
    let mut added = 0usize;
    for cap in catalog_caps {
        if !existing.contains(cap) {
            capabilities.push(serde_json::Value::String(cap.clone()));
            added += 1;
        }
    }
    let already = catalog_caps
        .iter()
        .filter(|cap| existing.contains(*cap))
        .count();
    let report = GenerateReport {
        added,
        already,
        catalog: catalog_caps.len(),
    };
    if added == 0 {
        return Ok((document.to_vec(), report));
    }
    let mut bytes = serde_json::to_vec_pretty(&value).context("serialize generated grants")?;
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    validate_document(&bytes).context("generated grant document is invalid")?;
    Ok((bytes, report))
}

fn report_line(report: &GenerateReport, apply: bool) -> String {
    if apply {
        if report.added == 0 {
            format!(
                "{ROOT_GROUP} already has all {} catalog capabilities ({} already present); durable grants unchanged",
                report.catalog, report.already
            )
        } else {
            format!(
                "Grants generated: added {} capabilities to {ROOT_GROUP} ({} already present, {} catalog caps)",
                report.added, report.already, report.catalog
            )
        }
    } else {
        format!(
            "plan: generate would add {} capabilities to {ROOT_GROUP} ({} already present, {} catalog caps); no write",
            report.added, report.already, report.catalog
        )
    }
}

fn copy_backup(source: &Path) -> Result<PathBuf> {
    let backup = backup_path(source);
    fs::copy(source, &backup)
        .with_context(|| format!("backup {} to {}", source.display(), backup.display()))?;
    fs::set_permissions(&backup, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("set permissions on {}", backup.display()))?;
    Ok(backup)
}

fn restore_backup(backup: &Path, source: &Path) -> Result<()> {
    let bytes = fs::read(backup)
        .with_context(|| format!("read grants backup {}", backup.display()))?;
    atomic_write(source, &bytes, 0o600)
        .with_context(|| format!("restore grants from {}", backup.display()))?;
    Ok(())
}

fn generate(source: &Path, catalog: Option<PathBuf>, apply: bool) -> Result<()> {
    let catalog_dir = resolve_catalog(catalog)?;
    let catalog_caps = catalog_capabilities(&catalog_dir)?;
    let source_bytes = fs::read(source)
        .with_context(|| format!("durable grant source unreadable: {}", source.display()))?;
    let (new_bytes, report) = generate_root_grants(&source_bytes, &catalog_caps)?;
    if !apply {
        println!("{}", report_line(&report, false));
        println!("catalog: {}", catalog_dir.display());
        return Ok(());
    }
    if report.added == 0 {
        println!("{}", report_line(&report, true));
        println!("catalog: {}", catalog_dir.display());
        return Ok(());
    }
    let backup = copy_backup(source)?;
    println!("backup={}", backup.display());
    if let Err(error) = (|| -> Result<()> {
        atomic_write(source, &new_bytes, 0o600)?;
        validate_document(&fs::read(source).with_context(|| source.display().to_string())?)
            .context("durable grants invalid after generate")?;
        Ok(())
    })() {
        if let Err(restore_error) = restore_backup(&backup, source) {
            bail!(
                "generate failed ({error:#}); also failed to restore {} ({restore_error:#})",
                backup.display()
            );
        }
        bail!(
            "generate failed; old grants restored from {}: {error:#}",
            backup.display()
        );
    }
    println!("{}", report_line(&report, true));
    println!("catalog: {}", catalog_dir.display());
    println!("applied={}", source.display());
    Ok(())
}

fn parse_generate_args(
    args: &mut impl Iterator<Item = std::ffi::OsString>,
) -> Result<(bool, Option<PathBuf>)> {
    let mut apply = false;
    let mut catalog = None;
    while let Some(raw) = args.next() {
        let arg = raw
            .into_string()
            .map_err(|_| anyhow::anyhow!("generate arguments must be UTF-8"))?;
        match arg.as_str() {
            "--apply-to-live" => apply = true,
            "--catalog" => {
                catalog = Some(PathBuf::from(
                    args.next()
                        .context("generate --catalog requires a path")?,
                ));
            }
            other if other.starts_with('-') => {
                bail!("unknown generate flag '{other}'");
            }
            other => bail!("unexpected generate argument '{other}'"),
        }
    }
    Ok((apply, catalog))
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
        "generate" => {
            let (apply, catalog) = parse_generate_args(&mut args)?;
            generate(&source, catalog, apply)?;
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
            "unknown command '{command}'; expected run, check, once, generate [--apply-to-live] [--catalog <dir>], validate [path], validate-audience <path>, or validate-toolsets <path>"
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
                    "capabilities": ["cognitive.read", "cognitive.invoke"]
                }
            }"#,
        )
        .unwrap();
    }

    #[test]
    fn accepts_group_membership() {
        validate_document(
            br#"{
                "$GUEST": {"capabilities": ["cap.data.cozo.system-graph.read@v1"]},
                "937d6d2b-ecae-ed53-f3a2-d7bd09f544ff": {
                    "groups": ["$GUEST"],
                    "capabilities": ["cognitive.read"]
                }
            }"#,
        )
        .unwrap();
    }

    #[test]
    fn rejects_unknown_group() {
        assert!(validate_document(br#"{"$NOPE":{"capabilities":[]}}"#).is_err());
    }

    #[test]
    fn rejects_wildcard_and_legacy_footprint_keys() {
        assert!(validate_document(br#"{"*":{"capabilities":[]}}"#).is_err());
        assert!(
            validate_document(
                br#"{
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef": {
                    "capabilities": []
                }
            }"#
            )
            .is_err()
        );
    }

    fn sample_document(root_caps: &[&str]) -> Vec<u8> {
        let caps: Vec<String> = root_caps.iter().map(|cap| format!("\"{cap}\"")).collect();
        format!(
            "{{\n  \"$ROOT\": {{\"capabilities\":[{}]}},\n  \"$GUEST\": {{\"capabilities\":[\"cap.data.cozo.system-graph.read@v1\"]}},\n  \"principal\": {{\"groups\":[\"$GUEST\"],\"capabilities\":[]}}\n}}\n",
            caps.join(", ")
        )
        .into_bytes()
    }

    #[test]
    fn generate_adds_only_missing_root_caps_and_keeps_other_entries() {
        let document = sample_document(&["already.held"]);
        let catalog = ["already.held", "cap.lxd.auth.get_auth_group@v1", "cap.storage.fuse.fs.chmod@v1"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let (bytes, report) = generate_root_grants(&document, &catalog).unwrap();
        assert_eq!(report.added, 2);
        assert_eq!(report.already, 1);
        assert_eq!(report.catalog, 3);
        validate_document(&bytes).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let root: Vec<&str> = value["$ROOT"]["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();
        assert_eq!(
            root,
            [
                "already.held",
                "cap.lxd.auth.get_auth_group@v1",
                "cap.storage.fuse.fs.chmod@v1"
            ]
        );
        assert_eq!(value["$GUEST"]["capabilities"][0], "cap.data.cozo.system-graph.read@v1");
        assert_eq!(value["principal"]["groups"][0], "$GUEST");
    }

    #[test]
    fn generate_is_a_no_write_when_root_already_has_the_catalog() {
        let document = sample_document(&["one", "two"]);
        let catalog = ["one", "two"].into_iter().map(str::to_string).collect();
        let (bytes, report) = generate_root_grants(&document, &catalog).unwrap();
        assert_eq!(report.added, 0);
        assert_eq!(bytes, document);
    }

    #[test]
    fn generate_refuses_a_document_without_root() {
        let document = br#"{"$GUEST":{"capabilities":["cap.data.cozo.system-graph.read@v1"]}}"#;
        let catalog = ["cap.lxd.one@v1"].into_iter().map(str::to_string).collect();
        assert!(generate_root_grants(document, &catalog).is_err());
    }

    #[test]
    fn apply_to_live_leaves_grants_untouched_when_the_catalog_is_empty() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("capability-grants.json");
        fs::write(&source, sample_document(&["held"])).unwrap();
        // A catalog directory with no blobs must fail closed and leave the source alone.
        let catalog = root.path().join("catalog");
        fs::create_dir(&catalog).unwrap();
        let before = fs::read(&source).unwrap();
        assert!(generate(&source, Some(catalog), true).is_err());
        assert_eq!(fs::read(&source).unwrap(), before);
        let backups: Vec<_> = fs::read_dir(root.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("capability-grants.json.bak-"))
            })
            .collect();
        // Failure happens before backup when the catalog is empty.
        assert!(backups.is_empty());
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
