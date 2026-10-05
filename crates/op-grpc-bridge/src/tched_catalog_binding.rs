//! Narrow ordinary-profile binding selection. The owner catalog supplies the
//! independent endpoint trust; native observations never manufacture that trust.

use anyhow::{Context, Result, ensure};
use op_plugins::state_plugins::tched_router::ResolveCatalogModelBindingOutput;
use std::collections::BTreeSet;

pub(crate) const MAX_BINDING_PROFILES: usize = 32;
pub(crate) const MAX_PROFILE_MODELS: usize = 5000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BindingTarget {
    pub provider_id: String,
    pub model_id: String,
    pub inference_url_prefix: String,
    pub catalog_revision: u64,
    pub ordinary_reference: Option<String>,
    pub excluded_references: BTreeSet<String>,
    /// The provider's native serve family (`custom`, `openai`, `anthropic`,
    /// …): the concrete "provider type" every binding candidate must share,
    /// so one live model listed by two unrelated familes can never produce
    /// an ambiguous binding.
    pub family: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProfileCandidate {
    pub reference: String,
    pub uri: String,
    pub models: Vec<String>,
    pub live: bool,
    /// `requires_openai_auth` profile: the native itself owns the credential
    /// and the backend origin (subscription auth), so the provider entry's
    /// HTTP `inference_url_prefix` is not the serve origin. Trust provenance
    /// is the profile's own declaration observed from the pinned native
    /// runtime, not an endpoint match against the provider file.
    pub oauth: bool,
}

pub(crate) fn valid_reference(reference: &str) -> bool {
    fn segment(part: &str) -> bool {
        (1..=64).contains(&part.len())
            && part.as_bytes()[0].is_ascii_lowercase()
            && part
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
    }
    reference
        .split_once('.')
        .is_some_and(|(family, alias)| segment(family) && segment(alias))
}

/// The serve family of a valid `family.alias` reference.
pub(crate) fn reference_family(reference: &str) -> Option<&str> {
    reference.split_once('.').map(|(family, _)| family)
}

fn endpoint(value: &str) -> Result<reqwest::Url> {
    ensure!(
        !value.is_empty()
            && value.len() <= 2048
            && value.trim() == value
            && !value.chars().any(char::is_control)
            && !value.contains(['?', '#', '\\']),
        "catalog binding endpoint is invalid"
    );
    let url = reqwest::Url::parse(value)
        .map_err(|_| anyhow::anyhow!("catalog binding endpoint is invalid"))?;
    let path = url.path().to_ascii_lowercase();
    ensure!(
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && !path.contains("%2f")
            && !path.contains("%5c"),
        "catalog binding endpoint must be credential-free HTTPS"
    );
    Ok(url)
}

fn endpoint_matches(uri: &str, prefix: &str) -> Result<bool> {
    let actual = endpoint(uri)?;
    let trusted = endpoint(prefix)?;
    let path = trusted.path().trim_end_matches('/');
    Ok(actual.origin() == trusted.origin()
        && (path.is_empty()
            || actual.path() == path
            || actual.path().starts_with(&format!("{path}/"))))
}

pub(crate) fn select(
    target: &BindingTarget,
    candidates: &[ProfileCandidate],
) -> Result<ResolveCatalogModelBindingOutput> {
    // Validate independent trust even for an empty or offline inventory.
    endpoint(&target.inference_url_prefix)?;
    ensure!(
        candidates.len() <= MAX_BINDING_PROFILES,
        "catalog binding profile inventory exceeds bound"
    );
    if let Some(reference) = &target.ordinary_reference {
        ensure!(
            valid_reference(reference),
            "catalog binding declared reference is invalid"
        );
    }
    let mut seen = BTreeSet::new();
    let mut matches = Vec::new();
    for candidate in candidates {
        ensure!(
            valid_reference(&candidate.reference) && seen.insert(&candidate.reference),
            "catalog binding profile inventory is invalid"
        );
        let mut models = BTreeSet::new();
        ensure!(
            candidate.models.len() <= MAX_PROFILE_MODELS
                && candidate.models.iter().all(|model| {
                    !model.is_empty()
                        && model.len() <= 2048
                        && model.trim() == model
                        && !model.chars().any(char::is_control)
                        && models.insert(model)
                }),
            "catalog binding model inventory is invalid"
        );
        if target.excluded_references.contains(&candidate.reference)
            || target
                .ordinary_reference
                .as_ref()
                .is_some_and(|reference| reference != &candidate.reference)
        {
            continue;
        }
        let match_family = target
            .family
            .as_str();
        ensure!(
            candidate
                .reference
                .split_once('.')
                .is_some_and(|(family, _)| family == match_family),
            "catalog binding candidate {} is outside the provider family {match_family}",
            candidate.reference
        );
        if candidate.live
            && models.contains(&target.model_id)
            // Subscription-native (OAuth) profiles carry no HTTP endpoint of
            // their own; their serve origin is managed by the native auth
            // store, so the provider entry's prefix match does not apply.
            && (candidate.oauth
                || endpoint_matches(&candidate.uri, &target.inference_url_prefix)?)
        {
            matches.push(candidate);
        }
    }
    ensure!(
        matches.len() == 1,
        "catalog model has no unique live ordinary native binding"
    );
    let candidate = matches
        .first()
        .context("catalog model binding is unavailable")?;
    Ok(ResolveCatalogModelBindingOutput {
        provider_id: target.provider_id.clone(),
        model_id: target.model_id.clone(),
        reference: candidate.reference.clone(),
        model: target.model_id.clone(),
        uri: candidate.uri.clone(),
        catalog_revision: target.catalog_revision,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target() -> BindingTarget {
        BindingTarget {
            provider_id: "go".into(),
            model_id: "same-model".into(),
            inference_url_prefix: "https://inference.example/zen/go/v1".into(),
            catalog_revision: 7,
            ordinary_reference: None,
            excluded_references: BTreeSet::new(),
            family: "openai".into(),
        }
    }
    fn candidate(reference: &str, uri: &str) -> ProfileCandidate {
        ProfileCandidate {
            reference: reference.into(),
            uri: uri.into(),
            models: vec!["same-model".into()],
            live: true,
            oauth: false,
        }
    }
    #[test]
    fn catalog_binding_distinguishes_platforms_and_path_boundaries() {
        let inventory = vec![
            candidate("custom.zen", "https://inference.example/zen/v1"),
            candidate("openai.go", "https://inference.example/zen/go/v1"),
        ];
        assert_eq!(
            select(&target(), &inventory).unwrap().reference,
            "openai.go"
        );
        assert!(
            select(
                &target(),
                &[candidate(
                    "openai.fake",
                    "https://inference.example/zen/go/v10"
                )]
            )
            .is_err()
        );
    }
    #[test]
    fn catalog_binding_ambiguity_and_private_routes_fail_closed() {
        let inventory = vec![
            candidate("openai.first", "https://inference.example/zen/go/v1"),
            candidate("openai.second", "https://inference.example/zen/go/v1"),
        ];
        assert!(select(&target(), &inventory).is_err());
        let mut declared = target();
        declared.ordinary_reference = Some("openai.first".into());
        assert_eq!(
            select(&declared, &inventory).unwrap().reference,
            "openai.first"
        );
        declared.excluded_references.insert("openai.first".into());
        assert!(select(&declared, &inventory).is_err());
    }
    #[test]
    fn catalog_binding_oauth_profile_binds_without_endpoint_match() {
        // A subscription-native profile has no HTTP endpoint of its own and
        // would fail the provider's prefix match; the oauth flag replaces it.
        let mut oauth = candidate("openai.catalog", "");
        oauth.oauth = true;
        assert_eq!(
            select(&target(), &[oauth.clone()]).unwrap().reference,
            "openai.catalog"
        );
        // Without the oauth declaration the same empty endpoint still fails
        // closed (an unexplained endpoint is never trusted).
        let mut keyed = oauth.clone();
        keyed.oauth = false;
        assert!(select(&target(), &[keyed]).is_err());
        // OAuth does not relax liveness or exact-model requirements.
        let mut dead = oauth.clone();
        dead.live = false;
        assert!(select(&target(), &[dead]).is_err());
        let mut wrong = oauth;
        wrong.models = vec!["different-model".into()];
        assert!(select(&target(), &[wrong]).is_err());
    }
    #[test]
    fn catalog_binding_requires_live_exact_model_and_bounds() {
        let mut profile = candidate("openai.go", "https://inference.example/zen/go/v1");
        profile.live = false;
        assert!(select(&target(), &[profile]).is_err());
        let mut profile = candidate("openai.go", "https://inference.example/zen/go/v1");
        profile.models = vec!["different-model".into()];
        assert!(select(&target(), &[profile]).is_err());
        let mut profile = candidate("openai.go", "https://inference.example/zen/go/v1");
        profile.models = vec!["same-model".into(); MAX_PROFILE_MODELS + 1];
        assert!(select(&target(), &[profile]).is_err());
    }
    #[test]
    fn catalog_binding_unsafe_urls_never_echo_secrets() {
        for uri in [
            "https://user:SECRET@inference.example/zen/go/v1",
            "https://inference.example/zen/go/v1?SECRET",
            "https://inference.example/zen/go/v1#SECRET",
            "https://inference.example/zen/go/v1%2fSECRET",
            "http://inference.example/zen/go/v1",
            "https://inference.example\\SECRET",
        ] {
            let error = select(&target(), &[candidate("openai.go", uri)])
                .unwrap_err()
                .to_string();
            assert!(!error.contains("SECRET"));
        }
    }
}
