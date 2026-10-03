//! Validate model-providers.toml configuration at startup to catch missing fields
//! before runtime routing failures.

use anyhow::{ensure, Context, Result};
use op_plugins::state_plugins::model_registry::ProviderConfig;

/// Validate all enabled providers have required fields for native profile provisioning
pub fn validate_providers_config(providers: &[ProviderConfig]) -> Result<()> {
    for provider in providers {
        if !provider.enabled.unwrap_or(true) {
            continue;
        }

        ensure!(!provider.id.is_empty(), "provider has empty id");

        ensure!(
            provider.inference_url_prefix.is_some(),
            "provider {} missing inference_url_prefix (required for native profile provisioning)",
            provider.id
        );

        ensure!(
            !provider.inference_url_prefix.as_ref().unwrap().is_empty(),
            "provider {} has empty inference_url_prefix",
            provider.id
        );

        ensure!(
            provider.key_file.is_some(),
            "provider {} missing key_file (required for API authentication)",
            provider.id
        );

        ensure!(
            !provider.key_file.as_ref().unwrap().is_empty(),
            "provider {} has empty key_file",
            provider.id
        );

        if let Some(control_plane) = &provider.control_plane {
            ensure!(
                !control_plane.profile.is_empty(),
                "provider {} control_plane profile is empty",
                provider.id
            );
        }

        if let Some(overrides) = &provider.native_profile_overrides {
            for (profile, override_id) in overrides {
                ensure!(
                    !profile.is_empty() && !override_id.is_empty(),
                    "provider {} has invalid override: '{}' -> '{}'",
                    provider.id,
                    profile,
                    override_id
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_provider_config() {
        let provider = ProviderConfig {
            id: "openai".to_string(),
            enabled: Some(true),
            inference_url_prefix: Some("https://api.openai.com".to_string()),
            key_file: Some("/etc/op-dbus/secrets/openai-key".to_string()),
            control_plane: None,
            native_provider_id: Some("openai.control_plane".to_string()),
            native_profile_overrides: None,
            key_env: None,
        };
        assert!(validate_providers_config(&[provider]).is_ok());
    }

    #[test]
    fn test_missing_inference_url() {
        let provider = ProviderConfig {
            id: "openai".to_string(),
            enabled: Some(true),
            inference_url_prefix: None,
            key_file: Some("/etc/op-dbus/secrets/openai-key".to_string()),
            control_plane: None,
            native_provider_id: None,
            native_profile_overrides: None,
            key_env: None,
        };
        assert!(validate_providers_config(&[provider]).is_err());
    }

    #[test]
    fn test_missing_key_file() {
        let provider = ProviderConfig {
            id: "openai".to_string(),
            enabled: Some(true),
            inference_url_prefix: Some("https://api.openai.com".to_string()),
            key_file: None,
            control_plane: None,
            native_provider_id: None,
            native_profile_overrides: None,
            key_env: None,
        };
        assert!(validate_providers_config(&[provider]).is_err());
    }

    #[test]
    fn test_disabled_provider_skipped() {
        let provider = ProviderConfig {
            id: "openai".to_string(),
            enabled: Some(false),
            inference_url_prefix: None, // Missing but should be ok
            key_file: None,             // Missing but should be ok
            control_plane: None,
            native_provider_id: None,
            native_profile_overrides: None,
            key_env: None,
        };
        assert!(validate_providers_config(&[provider]).is_ok());
    }
}
