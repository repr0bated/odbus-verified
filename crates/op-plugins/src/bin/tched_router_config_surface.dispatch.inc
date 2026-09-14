// ── Native dispatch metadata ───────────────────────────────────────────────
//
// This generated module owns schemas and routing metadata only. Async provider
// I/O and committed-state reads belong to the bridge after canonical admission.

/// The authoritative backing required by one retained config-surface method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigMethodBacking {
    /// Masked `GET /api/config` from the pinned v0.8.4 loopback gateway.
    WholeNativeConfig,
    /// One top-level value selected from the masked native config response.
    NativeSection(&'static str),
    /// A field in the committed protected tched_router state. The native
    /// v0.8.4 config document does not contain these plugin-owned fields.
    CommittedPluginStateSection(&'static str),
    /// Bounded typed property operations sent to native `PATCH /api/config`.
    NativePropertyPatch,
}

/// Resolve a canonical method name to its backing without reading state or
/// consulting the Rust registry for plugin existence.
pub fn config_method_backing(method: &str) -> Option<ConfigMethodBacking> {
    match method {
        "GetConfig" => Some(ConfigMethodBacking::WholeNativeConfig),
        "PatchConfig" => Some(ConfigMethodBacking::NativePropertyPatch),
        other => SECTIONS
            .iter()
            .find(|(name, _, _)| *name == other)
            .map(|(_, key, source)| match source {
                SectionSource::Upstream => ConfigMethodBacking::NativeSection(key),
                SectionSource::Local => ConfigMethodBacking::CommittedPluginStateSection(key),
            }),
    }
}

#[cfg(test)]
mod backing_tests {
    use super::*;

    #[test]
    fn all_declared_config_methods_have_one_explicit_backing() {
        for method in CONFIG_METHODS {
            assert!(
                config_method_backing(method).is_some(),
                "missing backing metadata for {method}"
            );
        }
        assert_eq!(config_method_backing("unknown"), None);
    }

    #[test]
    fn native_and_plugin_owned_sections_are_not_conflated() {
        assert_eq!(
            config_method_backing("GetGatewayConfig"),
            Some(ConfigMethodBacking::NativeSection("gateway"))
        );
        assert_eq!(
            config_method_backing("GetIdentityChainConfig"),
            Some(ConfigMethodBacking::CommittedPluginStateSection(
                "identity_chain"
            ))
        );
        assert_eq!(
            config_method_backing("PatchConfig"),
            Some(ConfigMethodBacking::NativePropertyPatch)
        );
    }
}
