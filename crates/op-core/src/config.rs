//! Environment Configuration Loader
//!
//! Loads environment variables from the canonical location: `/etc/op-dbus/environment`
//! This ensures all op-dbus components share the same configuration.
//!
//! ## Usage
//!
//! Call `load_environment()` early in main() before accessing any config:
//!
//! ```rust
//! use op_core::config::load_environment;
//!
//! fn main() {
//!     load_environment();
//!     // Now all env vars from /etc/op-dbus/environment are available
//! }
//! ```

use std::fs;
use std::path::Path;
use tracing::{debug, info, warn};

/// Canonical D-Bus well-known bus name for the op-dbus system.
///
/// All components (op-dbus-mirror, projection_server, rovs_proxy, etc.)
/// must use this constant instead of hardcoding the string.
pub const OPDBUS_BUS_NAME: &str = "org.opdbus.v1.plugins";

/// Canonical base object path for the projected plugin tree.
///
/// This is the single stable root anchor; plugin leaves live at
/// `{PLUGIN_BASE_PATH}/<plugin_id>` and must be derived from it (or discovered
/// by introspection), never hardcoded with a different spelling. Consumers that
/// only depend on `op-core` use this; `op-plugins` exposes the richer
/// `canonical::plugin_path()` helper built on the same value.
pub const PLUGIN_BASE_PATH: &str = "/org/opdbus/v1/plugins";

/// D-Bus bus name for the op-dbus-mirror service.
///
/// The mirror publishes OVSDB, ObjectManager, and other non-plugin surfaces.
/// It MUST NOT request `OPDBUS_BUS_NAME` — the projection server is the sole
/// owner of that name. Consumers of the mirror's surfaces (OVSDB JSON-RPC,
/// ObjectManager) dial this name instead.
pub const MIRROR_BUS_NAME: &str = "org.opdbus.v1.mirror";

/// Canonical session bus socket path.  No dash — consistent with `opdbus`
/// used in D-Bus names, object paths, and shm directories.
pub const SESSION_BUS_SOCKET_PATH: &str = "/run/opdbus/session-bus.sock";

/// Canonical session bus address string (for `DBUS_SESSION_BUS_ADDRESS`).
pub const SESSION_BUS_ADDRESS: &str = "unix:path=/run/opdbus/session-bus.sock";

/// Default path for the environment file
pub const DEFAULT_ENV_FILE: &str = "/etc/op-dbus/environment";

/// Alternative paths to check (in order of priority)
pub const ENV_FILE_PATHS: &[&str] = &["/etc/op-dbus/environment", "/etc/op-dbus.env", ".env"];

/// Load environment variables from the canonical configuration file.
///
/// This function:
/// 1. Checks `/etc/op-dbus/environment` first (system-wide)
/// 2. Falls back to `.env` in current directory (development)
/// 3. Does NOT override existing environment variables
///
/// Returns the path that was loaded, or None if no file was found.
pub fn load_environment() -> Option<String> {
    // Check if a custom path is specified
    if let Ok(custom_path) = std::env::var("OP_ENV_FILE") {
        if let Some(path) = try_load_env_file(&custom_path) {
            return Some(path);
        }
    }

    // Try each path in order
    for path in ENV_FILE_PATHS {
        if let Some(loaded_path) = try_load_env_file(path) {
            return Some(loaded_path);
        }
    }

    debug!("No environment file found, using existing environment");
    None
}

/// Try to load an environment file from the given path.
fn try_load_env_file(path: &str) -> Option<String> {
    let path_obj = Path::new(path);

    if !path_obj.exists() {
        return None;
    }

    match fs::read_to_string(path_obj) {
        Ok(content) => {
            let mut loaded_count = 0;
            let mut skipped_count = 0;

            for line in content.lines() {
                let line = line.trim();

                // Skip comments and empty lines
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }

                // Parse KEY=VALUE
                if let Some((key, value)) = parse_env_line(line) {
                    // Don't override existing environment variables
                    if std::env::var(&key).is_err() {
                        std::env::set_var(&key, &value);
                        loaded_count += 1;
                        debug!(
                            "Loaded: {}={}",
                            key,
                            if key.contains("KEY")
                                || key.contains("TOKEN")
                                || key.contains("SECRET")
                            {
                                "***"
                            } else {
                                &value
                            }
                        );
                    } else {
                        skipped_count += 1;
                        debug!("Skipped (already set): {}", key);
                    }
                }
            }

            info!(
                "Loaded {} environment variables from {} ({} skipped - already set)",
                loaded_count, path, skipped_count
            );

            Some(path.to_string())
        }
        Err(e) => {
            warn!("Failed to read environment file {}: {}", path, e);
            None
        }
    }
}

/// Parse a single environment line into key-value pair.
fn parse_env_line(line: &str) -> Option<(String, String)> {
    // Handle: KEY=VALUE, KEY="VALUE", KEY='VALUE'
    let mut parts = line.splitn(2, '=');
    let key = parts.next()?.trim();
    let value = parts.next()?.trim();

    if key.is_empty() {
        return None;
    }

    // Remove surrounding quotes
    let value = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(value);

    Some((key.to_string(), value.to_string()))
}

/// Get a configuration value with a default.
pub fn get_config(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Get an optional configuration value.
pub fn get_config_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// Get a boolean configuration value.
pub fn get_config_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .map(|v| matches!(v.to_lowercase().as_str(), "true" | "1" | "yes" | "on"))
        .unwrap_or(default)
}

/// Get an integer configuration value.
pub fn get_config_int(key: &str, default: i64) -> i64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_env_line_simple() {
        let (k, v) = parse_env_line("FOO=bar").unwrap();
        assert_eq!(k, "FOO");
        assert_eq!(v, "bar");
    }

    #[test]
    fn test_parse_env_line_quoted() {
        let (k, v) = parse_env_line("FOO=\"bar baz\"").unwrap();
        assert_eq!(k, "FOO");
        assert_eq!(v, "bar baz");
    }

    #[test]
    fn test_parse_env_line_single_quoted() {
        let (k, v) = parse_env_line("FOO='bar'").unwrap();
        assert_eq!(k, "FOO");
        assert_eq!(v, "bar");
    }

    #[test]
    fn test_parse_env_line_empty() {
        assert!(parse_env_line("").is_none());
        assert!(parse_env_line("=value").is_none());
    }
}
