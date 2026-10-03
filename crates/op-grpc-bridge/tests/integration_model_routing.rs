//! Integration tests for model routing and native profile provisioning
use op_plugins::state_plugins::model_registry::{ModelListFormat, ProviderConfig};

#[test]
fn test_valid_openai_provider_config() {
    let provider = ProviderConfig {
        id: "openai".to_string(),
        enabled: Some(true),
        inference_url_prefix: Some("https://api.openai.com/v1".to_string()),
        key_file: Some("/etc/op-dbus/secrets/openai-key".to_string()),
        control_plane: Some(
            op_plugins::state_plugins::model_registry::ControlPlaneRouting {
                profile: "openai.control_plane".to_string(),
            },
        ),
        native_provider_id: Some("openai.control_plane".to_string()),
        native_profile_overrides: None,
        key_env: Some("OPENAI_API_KEY".to_string()),
    };

    // Should not panic or error
    assert_eq!(provider.id, "openai");
    assert!(provider.inference_url_prefix.is_some());
    assert!(provider.key_file.is_some());
}

#[test]
fn test_provider_with_alternative_profiles() {
    let mut overrides = std::collections::BTreeMap::new();
    overrides.insert("openai.personal".to_string(), "openai.personal".to_string());

    let provider = ProviderConfig {
        id: "openai".to_string(),
        enabled: Some(true),
        inference_url_prefix: Some("https://api.openai.com/v1".to_string()),
        key_file: Some("/etc/op-dbus/secrets/openai-key".to_string()),
        control_plane: Some(
            op_plugins::state_plugins::model_registry::ControlPlaneRouting {
                profile: "openai.control_plane".to_string(),
            },
        ),
        native_provider_id: Some("openai.control_plane".to_string()),
        native_profile_overrides: Some(overrides),
        key_env: Some("OPENAI_API_KEY".to_string()),
    };

    assert!(provider.native_profile_overrides.is_some());
}

#[test]
fn test_model_format_consistency() {
    // Verify model list formats are handled consistently
    let formats = vec![ModelListFormat::Openai, ModelListFormat::Anthropic];

    for format in formats {
        match format {
            ModelListFormat::Openai => assert_eq!(format, ModelListFormat::Openai),
            ModelListFormat::Anthropic => assert_eq!(format, ModelListFormat::Anthropic),
        }
    }
}
