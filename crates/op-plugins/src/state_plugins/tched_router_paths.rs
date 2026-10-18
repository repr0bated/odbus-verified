//! The threetched service paths shared by tched-router plugin surfaces.
//! Defaults mirror deploy/runit/tched-router/run; service overrides use its
//! THREETCHED_BIN, THREETCHED_MANIFEST and TCHED_ROUTER_CONFIG_DIR variables.

use std::path::PathBuf;

use anyhow::{Result, ensure};

pub const DEFAULT_BINARY: &str = "/usr/local/libexec/op-dbus/threetched/threetched";
pub const DEFAULT_CONFIG_DIR: &str = "/var/lib/tched-router";
pub const DEFAULT_MANIFEST: &str = "/usr/share/op-dbus/threetched-manifests/tched-router.json";

fn absolute_env_path(name: &str, default: &str) -> Result<PathBuf> {
    let path = std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default));
    ensure!(path.is_absolute(), "{name} must be an absolute path");
    Ok(path)
}

pub fn binary() -> Result<PathBuf> {
    absolute_env_path("THREETCHED_BIN", DEFAULT_BINARY)
}

pub fn config_dir() -> Result<PathBuf> {
    absolute_env_path("TCHED_ROUTER_CONFIG_DIR", DEFAULT_CONFIG_DIR)
}

pub fn manifest() -> Result<PathBuf> {
    absolute_env_path("THREETCHED_MANIFEST", DEFAULT_MANIFEST)
}

/// Select the binary and provenance manifest used only to capture the sealed
/// CLI contract. Runtime dispatch continues to use [`binary`] and [`manifest`].
/// Build/seal invocations may point these selectors at a not-yet-installed
/// candidate, but must supply the pair together so provenance cannot drift.
pub(crate) fn capture_paths() -> Result<(PathBuf, PathBuf)> {
    match (
        std::env::var_os("THREETCHED_CAPTURE_BIN"),
        std::env::var_os("THREETCHED_CAPTURE_MANIFEST"),
    ) {
        (None, None) => Ok((binary()?, manifest()?)),
        (Some(binary), Some(manifest)) => {
            let binary = PathBuf::from(binary);
            let manifest = PathBuf::from(manifest);
            ensure!(binary.is_absolute(), "THREETCHED_CAPTURE_BIN must be an absolute path");
            ensure!(manifest.is_absolute(), "THREETCHED_CAPTURE_MANIFEST must be an absolute path");
            Ok((binary, manifest))
        }
        _ => anyhow::bail!("THREETCHED_CAPTURE_BIN and THREETCHED_CAPTURE_MANIFEST must be supplied together"),
    }
}
