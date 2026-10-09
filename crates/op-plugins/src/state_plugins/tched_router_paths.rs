//! The native ZeroClaw service paths shared by tched-router plugin surfaces.
//! Defaults mirror deploy/runit/tched-router/run; service overrides use its
//! TCHED_ROUTER_* environment names when supplied to the bridge.

use std::path::PathBuf;

use anyhow::{Result, ensure};

pub const DEFAULT_BINARY: &str = "/usr/bin/zeroclaw";
pub const DEFAULT_CONFIG_DIR: &str = "/var/lib/tched-router";
pub const DEFAULT_MANIFEST: &str = "/usr/share/op-dbus/native-manifests/tched-router.json";

fn absolute_env_path(name: &str, default: &str) -> Result<PathBuf> {
    let path = std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default));
    ensure!(path.is_absolute(), "{name} must be an absolute path");
    Ok(path)
}

pub fn binary() -> Result<PathBuf> {
    absolute_env_path("TCHED_ROUTER_NATIVE_BIN", DEFAULT_BINARY)
}

pub fn config_dir() -> Result<PathBuf> {
    absolute_env_path("TCHED_ROUTER_CONFIG_DIR", DEFAULT_CONFIG_DIR)
}

pub fn manifest() -> Result<PathBuf> {
    absolute_env_path("TCHED_ROUTER_NATIVE_MANIFEST", DEFAULT_MANIFEST)
}
