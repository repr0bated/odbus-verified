use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use chrono::Utc;
use ring::hkdf;
use serde::{Deserialize, Serialize};

use crate::users::PrivacyUser;

const DEFAULT_INGRESS_PORT: &str = "ovsbr0-sock";
const DEFAULT_NEXT_HOP: &str = "gbr_wg";
const ROUTE_HKDF_INFO: &[u8] = b"op-dbus/privacy-route/v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivacyRoutesState {
    #[serde(default)]
    pub routes: Vec<PrivacyRoute>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivacyRoute {
    pub name: String,
    pub route_id: String,
    pub user_id: String,
    pub email: String,
    pub wireguard_public_key: String,
    pub assigned_ip: String,
    pub selector_ip: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_name: Option<String>,
    pub ingress_port: String,
    pub next_hop: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

struct RouteIdKeyLen;

impl hkdf::KeyType for RouteIdKeyLen {
    fn len(&self) -> usize {
        32
    }
}

pub async fn publish_user_privacy_route(
    user: &PrivacyUser,
    container_name: Option<&str>,
) -> Result<String> {
    let route_id = derive_route_id(&user.wg_public_key)?;
    let mut state = crate::state_manager_client::query_plugin_state("privacy_routes")
        .await?
        .unwrap_or(PrivacyRoutesState { routes: Vec::new() });
    upsert_route(
        &mut state,
        PrivacyRoute::from_user(user, route_id.clone(), container_name),
    );
    crate::state_manager_client::apply_plugin_state("privacy_routes", &state).await?;

    Ok(route_id)
}

fn upsert_route(state: &mut PrivacyRoutesState, route: PrivacyRoute) {
    match state
        .routes
        .iter_mut()
        .find(|existing| existing.route_id == route.route_id)
    {
        Some(existing) => {
            let created_at = existing.created_at.clone();
            *existing = route;
            existing.created_at = created_at;
        }
        None => state.routes.push(route),
    }

    state.routes.sort_by(|a, b| a.route_id.cmp(&b.route_id));
}

pub fn derive_route_id(wg_public_key: &str) -> Result<String> {
    let shared_secret = std::env::var("PRIVACY_ROUTE_SHARED_SECRET")
        .context("PRIVACY_ROUTE_SHARED_SECRET is required for privacy route derivation")?;
    if shared_secret.trim().is_empty() {
        bail!("PRIVACY_ROUTE_SHARED_SECRET must not be empty");
    }

    let public_key_bytes = base64::engine::general_purpose::STANDARD
        .decode(wg_public_key.trim())
        .with_context(|| format!("invalid WireGuard public key '{}'", wg_public_key))?;
    if public_key_bytes.len() != 32 {
        bail!(
            "WireGuard public key must decode to 32 bytes, got {}",
            public_key_bytes.len()
        );
    }

    let salt = hkdf::Salt::new(hkdf::HKDF_SHA256, shared_secret.as_bytes());
    let prk = salt.extract(&public_key_bytes);
    let okm = prk
        .expand(&[ROUTE_HKDF_INFO], RouteIdKeyLen)
        .map_err(|_| anyhow!("failed to expand HKDF route ID"))?;
    let mut route_id = [0u8; 32];
    okm.fill(&mut route_id)
        .map_err(|_| anyhow!("failed to derive HKDF route ID bytes"))?;
    Ok(hex::encode(route_id))
}

impl PrivacyRoute {
    fn from_user(user: &PrivacyUser, route_id: String, container_name: Option<&str>) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            name: route_id.clone(),
            route_id,
            user_id: user.id.clone(),
            email: user.email.clone(),
            wireguard_public_key: user.wg_public_key.clone(),
            assigned_ip: user.assigned_ip.clone(),
            selector_ip: selector_ip(&user.assigned_ip),
            container_name: container_name.map(ToOwned::to_owned),
            ingress_port: std::env::var("PRIVACY_ROUTE_INGRESS_PORT")
                .unwrap_or_else(|_| DEFAULT_INGRESS_PORT.to_string()),
            next_hop: std::env::var("PRIVACY_ROUTE_NEXT_HOP")
                .unwrap_or_else(|_| DEFAULT_NEXT_HOP.to_string()),
            enabled: true,
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

fn selector_ip(cidr: &str) -> String {
    cidr.split('/').next().unwrap_or(cidr).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_id_is_deterministic() {
        std::env::set_var("PRIVACY_ROUTE_SHARED_SECRET", "test-shared-secret");
        let key = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        let a = derive_route_id(key).expect("derive a");
        let b = derive_route_id(key).expect("derive b");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn test_selector_ip_strips_prefix() {
        assert_eq!(selector_ip("10.100.0.2/32"), "10.100.0.2");
        assert_eq!(selector_ip("10.100.0.2"), "10.100.0.2");
    }
}
