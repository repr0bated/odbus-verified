//! Canonical Plugin Path and Interface Constants
//!
//! This module defines the SINGLE SOURCE OF TRUTH for all plugin-related
//! paths, interfaces, and naming conventions following FreeDesktop D-Bus
//! standards.
//!
//! ## Canonical Naming Convention
//!
//! ### D-Bus Object Paths
//! ```text
//! /org/opdbus/v1/plugins/{plugin_name}
//! /org/opdbus/v1/plugins/{plugin_name}/{child_object}
//! ```
//!
//! ### D-Bus Interface Names
//! ```text
//! org.opdbus.v1.PluginV1
//! ```
//!
//! ### D-Bus Service Names (Bus Names)
//! ```text
//! org.opdbus.v1.plugins
//! ```
//!
//! ## Forbidden Patterns (DO NOT USE)
//!
//! These patterns are deprecated and MUST NOT be used:
//! - `/opdbus/v1/plugins/` - Missing `org` prefix
//! - `/org/opdbus/plugins/` - Missing `v1` version
//! - `/org/opdbus/v1/` - Missing `plugin` segment
//! - `org.opdbus.plugins.*` - Missing `v1` version
//! - `org.opdbus.v1plugins.*` - Missing dot separator
//!
//! ## Migration Guide
//!
//! Old (deprecated): `/opdbus/v1/plugins/net`
//! New (canonical): `/org/opdbus/v1/plugins/net`
//!
//! Old (deprecated alias): `/org/opdbus/v1/plugin/plugins/net`
//! New (canonical): `/org/opdbus/v1/plugins/net`
//!
//! Old (deprecated): `org.opdbus.NetPlugin`
//! New (canonical): `org.opdbus.v1.PluginV1`

/// Root D-Bus object path for the op-dbus system
pub const DBUS_ROOT_PATH: &str = "/org/opdbus/v1";

/// Canonical base path for all plugins
pub const PLUGIN_BASE_PATH: &str = "/org/opdbus/v1/plugins";

/// Base interface name for plugin-related interfaces.
pub const PLUGIN_BASE_INTERFACE: &str = "org.opdbus.v1.PluginV1";

/// Interface shared by every object on the canonical plugin tree.
pub const PLUGIN_INTERFACE: &str = "org.opdbus.v1.PluginV1";

/// Compatibility alias for the one plugin interface.
pub const PLUGINS_INTERFACE: &str = PLUGIN_INTERFACE;

/// Canonical plugin-host service name (bus name)
pub const BASE_SERVICE_NAME: &str = "org.opdbus.v1.plugins";

/// ObjectManager interface (FreeDesktop standard)
pub const OBJECT_MANAGER_INTERFACE: &str = "org.freedesktop.DBus.ObjectManager";

/// Properties interface (FreeDesktop standard)
pub const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";

/// Introspectable interface (FreeDesktop standard)
pub const INTROSPECTABLE_INTERFACE: &str = "org.freedesktop.DBus.Introspectable";

/// Peer interface (FreeDesktop standard)
pub const PEER_INTERFACE: &str = "org.freedesktop.DBus.Peer";

/// Legacy path prefix - DEPRECATED, for migration only
#[deprecated(
    since = "2.0.0",
    note = "Use PLUGIN_BASE_PATH instead. Legacy paths will be removed in v3.0.0"
)]
pub const LEGACY_PLUGIN_PATH: &str = "/opdbus/v1/plugins";

/// Alternative legacy path - DEPRECATED, for migration only
#[deprecated(
    since = "2.0.0",
    note = "Use PLUGIN_BASE_PATH instead. Legacy paths will be removed in v3.0.0"
)]
pub const LEGACY_ALT_PLUGIN_PATH: &str = "/org/opdbus/v1/plugin/plugins";

/// Build the full D-Bus object path for a plugin
///
/// # Example
/// ```
/// use op_plugins::canonical::plugin_path;
///
/// let path = plugin_path("incus");
/// assert_eq!(path, "/org/opdbus/v1/plugins/incus");
/// ```
pub fn plugin_path(name: &str) -> String {
    format!("{}/{}", PLUGIN_BASE_PATH, sanitize_plugin_name(name))
}

/// Return the single legal D-Bus interface for a plugin object.
///
/// # Example
/// ```
/// use op_plugins::canonical::plugin_interface;
///
/// let iface = plugin_interface("incus");
/// assert_eq!(iface, "org.opdbus.v1.PluginV1");
/// ```
pub fn plugin_interface(_name: &str) -> String {
    PLUGIN_INTERFACE.to_string()
}

/// Build a child object path under a plugin
///
/// # Example
/// ```
/// use op_plugins::canonical::plugin_child_path;
///
/// let path = plugin_child_path("incus", "instance-1");
/// assert_eq!(path, "/org/opdbus/v1/plugins/incus/instance-1");
/// ```
pub fn plugin_child_path(plugin_name: &str, child_id: &str) -> String {
    format!(
        "{}/{}/{}",
        PLUGIN_BASE_PATH,
        sanitize_plugin_name(plugin_name),
        sanitize_child_id(child_id)
    )
}

/// Sanitize a plugin name for use in paths and interface names
///
/// Rules:
/// - Convert to lowercase
/// - Replace hyphens with underscores
/// - Remove any leading/trailing slashes
/// - Must start with a letter (a-z)
pub fn sanitize_plugin_name(name: &str) -> String {
    let sanitized = name
        .trim()
        .trim_matches('/')
        .to_lowercase()
        .replace('-', "_");

    // Ensure it starts with a letter
    if sanitized
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        format!("plugin_{}", sanitized)
    } else {
        sanitized
    }
}

/// Capitalize a plugin name for use in interface names
///
/// Converts `plugin_name` to `PluginName`
pub fn capitalize_plugin_name(name: &str) -> String {
    name.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    format!(
                        "{}{}",
                        first.to_uppercase().collect::<String>(),
                        chars.as_str().to_lowercase()
                    )
                }
            }
        })
        .collect()
}

/// Sanitize a child object ID for use in paths
///
/// Rules:
/// - Must not contain path separators
/// - Must be valid UTF-8
/// - Length must be <= 255 characters
fn sanitize_child_id(id: &str) -> String {
    let sanitized: String = id
        .chars()
        .filter(|c| *c != '/' && *c != '\\' && !c.is_control())
        .take(255)
        .collect();

    if sanitized.is_empty() {
        "_".to_string()
    } else {
        sanitized
    }
}

/// Validate a plugin path is in canonical form
///
/// Returns true if the path follows the canonical convention
pub fn is_canonical_plugin_path(path: &str) -> bool {
    path.starts_with(PLUGIN_BASE_PATH)
}

/// Normalize a plugin path to canonical form
///
/// Converts legacy paths to canonical paths
///
/// # Examples
/// - `/opdbus/v1/plugins/net` → `/org/opdbus/v1/plugins/net`
/// - `/org/opdbus/v1/plugin/plugins/net` → `/org/opdbus/v1/plugins/net`
/// - `/org/opdbus/v1/plugins/net` → `/org/opdbus/v1/plugins/net` (already canonical)
pub fn normalize_plugin_path(path: &str) -> Option<String> {
    let trimmed = path.trim().trim_matches('/');

    // Already canonical
    if trimmed.starts_with("org/opdbus/v1/plugins/") {
        return Some(format!("/{}", trimmed));
    }

    // Legacy alias: /org/opdbus/v1/plugin/plugins/{name}
    if let Some(rest) = trimmed.strip_prefix("org/opdbus/v1/plugin/plugins/") {
        let name = rest.split('/').next()?;
        return Some(plugin_path(name));
    }

    // Legacy: /opdbus/v1/plugins/{name}
    if let Some(rest) = trimmed.strip_prefix("opdbus/v1/plugins/") {
        let name = rest.split('/').next()?;
        return Some(plugin_path(name));
    }

    // Just a plugin name
    if !trimmed.contains('/') {
        return Some(plugin_path(trimmed));
    }

    None
}

/// Extract plugin name from a path (canonical or legacy)
///
/// # Examples
/// - `/org/opdbus/v1/plugins/net` → `Some("net")`
/// - `/opdbus/v1/plugins/net` → `Some("net")` (legacy)
/// - `/other/path` → `None`
pub fn extract_plugin_name(path: &str) -> Option<String> {
    let normalized = normalize_plugin_path(path)?;
    normalized
        .strip_prefix(PLUGIN_BASE_PATH)?
        .trim_start_matches('/')
        .split('/')
        .next()
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_path() {
        assert_eq!(plugin_path("incus"), "/org/opdbus/v1/plugins/incus");
        assert_eq!(plugin_path("my-plugin"), "/org/opdbus/v1/plugins/my_plugin");
        assert_eq!(
            plugin_path("123invalid"),
            "/org/opdbus/v1/plugins/plugin_123invalid"
        );
    }

    #[test]
    fn test_plugin_interface() {
        assert_eq!(plugin_interface("incus"), "org.opdbus.v1.PluginV1");
        assert_eq!(plugin_interface("my-plugin"), "org.opdbus.v1.PluginV1");
    }

    #[test]
    fn test_plugin_child_path() {
        assert_eq!(
            plugin_child_path("incus", "instance-1"),
            "/org/opdbus/v1/plugins/incus/instance-1"
        );
    }

    #[test]
    fn test_sanitize_plugin_name() {
        assert_eq!(sanitize_plugin_name("incus"), "incus");
        assert_eq!(sanitize_plugin_name("My-Plugin"), "my_plugin");
        assert_eq!(sanitize_plugin_name("/incus/"), "incus");
        assert_eq!(sanitize_plugin_name("123test"), "plugin_123test");
    }

    #[test]
    fn test_normalize_plugin_path() {
        // Canonical (no change needed)
        assert_eq!(
            normalize_plugin_path("/org/opdbus/v1/plugins/net"),
            Some("/org/opdbus/v1/plugins/net".to_string())
        );

        // Legacy paths
        assert_eq!(
            normalize_plugin_path("/opdbus/v1/plugins/net"),
            Some("/org/opdbus/v1/plugins/net".to_string())
        );
        assert_eq!(
            normalize_plugin_path("/org/opdbus/v1/plugin/plugins/net"),
            Some("/org/opdbus/v1/plugins/net".to_string())
        );

        // Just plugin name
        assert_eq!(
            normalize_plugin_path("net"),
            Some("/org/opdbus/v1/plugins/net".to_string())
        );

        // Invalid
        assert_eq!(normalize_plugin_path("/other/path"), None);
    }

    #[test]
    fn test_extract_plugin_name() {
        assert_eq!(
            extract_plugin_name("/org/opdbus/v1/plugins/net"),
            Some("net".to_string())
        );
        assert_eq!(
            extract_plugin_name("/opdbus/v1/plugins/net"),
            Some("net".to_string())
        );
        assert_eq!(extract_plugin_name("/other/path"), None);
    }

    #[test]
    fn test_is_canonical_plugin_path() {
        assert!(is_canonical_plugin_path("/org/opdbus/v1/plugins/net"));
        assert!(!is_canonical_plugin_path("/opdbus/v1/plugins/net"));
        assert!(!is_canonical_plugin_path(
            "/org/opdbus/v1/plugin/plugins/net"
        ));
    }

    #[test]
    fn test_constants() {
        assert_eq!(DBUS_ROOT_PATH, "/org/opdbus/v1");
        assert_eq!(PLUGIN_BASE_PATH, "/org/opdbus/v1/plugins");
        assert_eq!(PLUGIN_BASE_INTERFACE, "org.opdbus.v1.PluginV1");
        assert_eq!(PLUGINS_INTERFACE, "org.opdbus.v1.PluginV1");
        assert_eq!(BASE_SERVICE_NAME, "org.opdbus.v1.plugins");
    }
}
