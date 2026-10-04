//! Validate model-providers.toml configuration at startup to catch missing
//! fields before runtime routing failures.
//!
//! `parse_providers_toml` already enforces the structural invariants (profile
//! grammar, unique ownership, no credential-bearing URLs). This adds the
//! stricter *routing-readiness* gate the native profile pipeline needs: an
//! enabled provider must declare the `inference_url_prefix` the route checks
//! compare against, and must name the key file its auth requires.

use anyhow::{Result, ensure};
use op_plugins::state_plugins::model_registry::{ProviderAuth, ProviderConfig};

/// Validate that all enabled providers carry the fields native profile
/// provisioning and route binding depend on. Disabled providers are skipped
/// so a parked entry never blocks startup.
pub fn validate_providers_config(providers: &[ProviderConfig]) -> Result<()> {
    for provider in providers {
        if !provider.enabled {
            continue;
        }

        ensure!(!provider.id.is_empty(), "provider has empty id");

        let prefix = provider
            .inference_url_prefix
            .as_deref()
            .with_context_prefix(
                &provider.id,
                "missing inference_url_prefix (required for native profile provisioning)",
            )?;
        ensure!(
            !prefix.is_empty(),
            "provider {} has empty inference_url_prefix",
            provider.id
        );

        // `auth = "none"` providers legitimately carry no key_file; any other
        // auth needs one. parse_providers_toml already enforces this pairing,
        // but we restate the readiness expectation explicitly here.
        if provider.auth != ProviderAuth::None {
            let key_file = provider.key_file.as_deref().with_context_prefix(
                &provider.id,
                "missing key_file (required for API authentication)",
            )?;
            ensure!(
                !key_file.is_empty(),
                "provider {} has empty key_file",
                provider.id
            );
        }

        if let Some(control_plane) = &provider.control_plane {
            ensure!(
                !control_plane.profile.is_empty(),
                "provider {} control_plane profile is empty",
                provider.id
            );
        }

        for (profile, override_id) in &provider.native_profile_overrides {
            ensure!(
                !profile.is_empty() && !override_id.is_empty(),
                "provider {} has invalid override: '{}' -> '{}'",
                provider.id,
                profile,
                override_id
            );
        }
    }
    Ok(())
}

/// Small helper so a missing optional field reports with the provider id.
trait WithContextPrefix<T> {
    fn with_context_prefix(self, provider_id: &str, message: &str) -> Result<T>;
}

impl<T> WithContextPrefix<T> for Option<T> {
    fn with_context_prefix(self, provider_id: &str, message: &str) -> Result<T> {
        self.ok_or_else(|| anyhow::anyhow!("provider {provider_id} {message}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_plugins::state_plugins::model_registry::ModelListFormat;
    use std::collections::BTreeMap;

    fn provider(id: &str) -> ProviderConfig {
        ProviderConfig {
            id: id.to_string(),
            native_provider_id: None,
            inference_url_prefix: Some("https://api.openai.com/v1".to_string()),
            native_profile_overrides: BTreeMap::new(),
            name: "Test".to_string(),
            models_url: "https://api.openai.com/v1/models".to_string(),
            format: ModelListFormat::Openai,
            auth: ProviderAuth::Bearer,
            key_file: Some("/etc/op-dbus/secrets/openai-api-key".to_string()),
            key_env: None,
            control_plane: None,
            enabled: true,
        }
    }

    #[test]
    fn valid_provider_config_passes() {
        assert!(validate_providers_config(&[provider("openai")]).is_ok());
    }

    #[test]
    fn missing_inference_url_is_rejected() {
        let mut p = provider("openai");
        p.inference_url_prefix = None;
        assert!(validate_providers_config(&[p]).is_err());
    }

    #[test]
    fn missing_key_file_for_authed_provider_is_rejected() {
        let mut p = provider("openai");
        p.key_file = None;
        assert!(validate_providers_config(&[p]).is_err());
    }

    #[test]
    fn none_auth_provider_without_key_file_is_ok() {
        let mut p = provider("public");
        p.auth = ProviderAuth::None;
        p.key_file = None;
        assert!(validate_providers_config(&[p]).is_ok());
    }

    #[test]
    fn disabled_provider_is_skipped() {
        let mut p = provider("openai");
        p.enabled = false;
        p.inference_url_prefix = None;
        p.key_file = None;
        assert!(validate_providers_config(&[p]).is_ok());
    }
}
