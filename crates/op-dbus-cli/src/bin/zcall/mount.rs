//! The call path: the identity's mounted 3tchedFS namespace. Every call is a
//! write to `plugins/<plugin>/operations/<method>/call`, admitted, executed and
//! receipted by the bridge. zcall has no D-Bus or backend fallback.

use std::path::{Path, PathBuf};

use _3tched_fs::mounted::{IdentityMode, MountedDiagnostic, MountedNamespace};
use _3tched_fs::namespace::encode_component;
use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde_json::Value;

/// The host view named in AGENTS.md; `--mount` / `ZCALL_MOUNT` select another.
pub const DEFAULT_MOUNT: &str = "/run/mounts/3tchedFS";

/// Pick the mount: explicit choice, then the host view, then the only
/// per-container view this user can see. Never guesses between several.
pub fn select(explicit: Option<&str>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(PathBuf::from(path));
    }
    if Path::new(DEFAULT_MOUNT).join(".3tchedfs.json").exists() {
        return Ok(PathBuf::from(DEFAULT_MOUNT));
    }
    let visible: Vec<PathBuf> = std::fs::read_dir("/run/mounts/by-container")
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path().join("3tchedFS"))
        .filter(|path| path.join(".3tchedfs.json").exists())
        .collect();
    match visible.as_slice() {
        [one] => Ok(one.clone()),
        [] => bail!(
            "no readable 3tchedFS mount: {DEFAULT_MOUNT} is absent and no \
             /run/mounts/by-container/*/3tchedFS is readable by this user; pass --mount"
        ),
        many => bail!(
            "several 3tchedFS mounts are visible; choose one with --mount: {}",
            many.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
        ),
    }
}

/// The identity-mode expectation. The mount's own declaration chooses it
/// unless the operator names one; MountedNamespace then checks consistency.
fn identity_mode(mount: &Path, explicit: Option<&str>) -> Result<IdentityMode> {
    let kind = match explicit {
        Some(kind) => kind.to_owned(),
        None => {
            let text = std::fs::read_to_string(mount.join(".3tchedfs.json"))
                .with_context(|| format!("read {}/.3tchedfs.json", mount.display()))?;
            let declaration: Value = serde_json::from_str(&text)?;
            declaration["mount_identity"]["kind"]
                .as_str()
                .context("mount declares no identity kind")?
                .to_owned()
        }
    };
    IdentityMode::from_str(&kind, true)
        .map_err(|_| anyhow::anyhow!("unsupported mount identity mode {kind}"))
}

pub fn open(mount: &Path, mode: Option<&str>) -> Result<MountedNamespace> {
    let mode = identity_mode(mount, mode)?;
    MountedNamespace::open_for_identity_mode(mount, mode, None)
        .with_context(|| format!("open 3tchedFS namespace {}", mount.display()))
}

/// The mount-relative operation path for one sealed method.
pub fn call_path(plugin: &str, method: &str) -> String {
    format!(
        "plugins/{}/operations/{}/call",
        encode_component(plugin),
        encode_component(method)
    )
}

/// Write one call and return its terminal receipt. A mutation needs
/// `confirm`; a refusal comes back as the mount's original diagnostic.
pub fn call(
    namespace: &MountedNamespace,
    plugin: &str,
    method: &str,
    arguments: Value,
    confirm: bool,
) -> Result<Value> {
    namespace
        .invoke(&call_path(plugin, method), arguments, confirm)
        .map_err(|error| match error.downcast::<MountedDiagnostic>() {
            Ok(MountedDiagnostic(diagnostic)) => anyhow::anyhow!(
                "operation refused or failed (mount diagnostic, not retried):\n{}",
                serde_json::to_string_pretty(&diagnostic).unwrap_or_default()
            ),
            Err(other) => other,
        })
}

pub fn state(namespace: &MountedNamespace, plugin: &str) -> Result<Value> {
    namespace.state(plugin)
}
