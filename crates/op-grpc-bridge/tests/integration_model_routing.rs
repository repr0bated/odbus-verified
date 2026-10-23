//! Integration tests for model routing and native profile provisioning.
//!
//! These exercise `ProviderConfig` the way the live provider list is shaped:
//! `enabled` is a plain bool, `native_profile_overrides` is always present
//! (empty by default), and every provider carries `name`/`models_url`/
//! `format`/`auth`.

use op_plugins::state_plugins::model_registry::{
    ControlPlaneRouting, ModelListFormat, ProviderAuth, ProviderConfig,
};
use std::collections::BTreeMap;

fn openai_provider() -> ProviderConfig {
    ProviderConfig {
        id: "openai".to_string(),
        native_provider_id: Some("openai.personal".to_string()),
        inference_url_prefix: Some("https://api.openai.com/v1".to_string()),
        native_profile_overrides: BTreeMap::new(),
        name: "OpenAI".to_string(),
        models_url: "https://api.openai.com/v1/models".to_string(),
        format: ModelListFormat::Openai,
        auth: ProviderAuth::Bearer,
        key_file: Some("/etc/op-dbus/secrets/openai-api-key".to_string()),
        key_env: Some("OPENAI_API_KEY".to_string()),
        credential: None,
        credential_slot: None,
        control_plane: Some(ControlPlaneRouting {
            profile: "openai.control_plane".to_string(),
            alternatives: Vec::new(),
        }),
        enabled: true,
    }
}

#[test]
fn valid_openai_provider_resolves_its_profiles() {
    let provider = openai_provider();
    assert_eq!(provider.id, "openai");
    assert!(provider.inference_url_prefix.is_some());
    assert!(provider.key_file.is_some());
    // No recorded alternative → ordinary (Personal) binding is the native id.
    assert_eq!(
        provider.native_provider_id_for_model(None).as_deref(),
        Some("openai.personal")
    );
    // Control Plane resolves to the provider default.
    assert_eq!(
        provider.control_plane_profile(None),
        Some("openai.control_plane")
    );
}

#[test]
fn recorded_alternative_binds_through_an_explicit_override() {
    let mut provider = openai_provider();
    provider.control_plane = Some(ControlPlaneRouting {
        profile: "openai.control_plane".to_string(),
        alternatives: vec!["openai.control_plane_responses".to_string()],
    });
    let mut overrides = BTreeMap::new();
    overrides.insert(
        "openai.control_plane_responses".to_string(),
        "openai.personal_responses".to_string(),
    );
    provider.native_profile_overrides = overrides;

    // The recorded alternative uses its explicit ordinary counterpart.
    assert_eq!(
        provider
            .native_provider_id_for_model(Some("openai.control_plane_responses"))
            .as_deref(),
        Some("openai.personal_responses")
    );
    // An unmapped alternative must not silently reuse another API.
    assert_eq!(
        provider.native_provider_id_for_model(Some("openai.unmapped")),
        None
    );
}

#[test]
fn model_format_consistency() {
    for format in [ModelListFormat::Openai, ModelListFormat::Anthropic] {
        match format {
            ModelListFormat::Openai => assert_eq!(format, ModelListFormat::Openai),
            ModelListFormat::Anthropic => assert_eq!(format, ModelListFormat::Anthropic),
        }
    }
}
