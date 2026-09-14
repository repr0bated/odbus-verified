//! Per-user privacy container publication.
//!
//! Registration/verification flows publish desired Incus state through
//! `org.opdbus.StateManager` instead of shelling out to `incus` directly.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info};

use crate::privacy_routes::derive_route_id;
use crate::users::PrivacyUser;

const DEFAULT_CONTAINER_IMAGE: &str = "images:alpine/3.19";
const DEFAULT_NAME_PREFIX: &str = "privacy-user-";
const DEFAULT_DEVICE_NAME: &str = "privacy0";

#[derive(Debug, Clone)]
pub struct PrivacyContainerConfig {
    pub image: String,
    pub name_prefix: String,
    pub device_name: String,
    pub storage_pool: Option<String>,
    pub attach_bridged_nic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct IncusState {
    #[serde(default)]
    instances: Vec<IncusInstance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct IncusInstance {
    name: String,
    status: String,
    #[serde(rename = "type")]
    instance_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    storage_pool: Option<String>,
    #[serde(default)]
    profiles: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    config: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    devices: Option<HashMap<String, HashMap<String, String>>>,
}

impl PrivacyContainerConfig {
    pub fn from_env() -> Self {
        Self {
            image: std::env::var("PRIVACY_CONTAINER_IMAGE")
                .unwrap_or_else(|_| DEFAULT_CONTAINER_IMAGE.to_string()),
            name_prefix: std::env::var("PRIVACY_CONTAINER_PREFIX")
                .unwrap_or_else(|_| DEFAULT_NAME_PREFIX.to_string()),
            device_name: std::env::var("PRIVACY_CONTAINER_DEVICE")
                .unwrap_or_else(|_| DEFAULT_DEVICE_NAME.to_string()),
            storage_pool: std::env::var("PRIVACY_CONTAINER_STORAGE_POOL")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            attach_bridged_nic: std::env::var("PRIVACY_CONTAINER_ATTACH_BRIDGED_NIC")
                .ok()
                .map(|value| {
                    let value = value.trim().to_ascii_lowercase();
                    value == "1" || value == "true" || value == "yes"
                })
                .unwrap_or(true),
        }
    }
}

/// Ensure an Incus container is represented in desired state and running.
///
/// Idempotent: existing instances are updated in place.
pub async fn ensure_user_container(user: &PrivacyUser) -> Result<String> {
    let cfg = PrivacyContainerConfig::from_env();
    ensure_user_container_with_config(user, &cfg).await
}

async fn ensure_user_container_with_config(
    user: &PrivacyUser,
    cfg: &PrivacyContainerConfig,
) -> Result<String> {
    if !cfg.attach_bridged_nic && cfg.storage_pool.is_none() {
        bail!(
            "PRIVACY_CONTAINER_STORAGE_POOL is required when provisioning Incus containers without a bridged NIC"
        );
    }

    let container_name = container_name_for_user(&user.id, &cfg.name_prefix);
    let route_id = derive_route_id(&user.wg_public_key)?;
    debug!(
        "Publishing privacy container '{}' for user {} via StateManager",
        container_name, user.id
    );

    let mut state = crate::state_manager_client::query_plugin_state("incus")
        .await?
        .unwrap_or(IncusState {
            instances: Vec::new(),
        });
    upsert_instance(
        &mut state,
        desired_instance(user, &container_name, &route_id, cfg),
    );
    crate::state_manager_client::apply_plugin_state("incus", &state).await?;

    info!(
        "Privacy container '{}' is published and running for user {}",
        container_name, user.id
    );
    Ok(container_name)
}

fn desired_instance(
    user: &PrivacyUser,
    container_name: &str,
    route_id: &str,
    cfg: &PrivacyContainerConfig,
) -> IncusInstance {
    let devices = if cfg.attach_bridged_nic {
        let bridge =
            std::env::var("PRIVACY_CONTAINER_BRIDGE").unwrap_or_else(|_| "ovsbr0".to_string());
        Some(HashMap::from([(
            cfg.device_name.clone(),
            HashMap::from([
                ("type".to_string(), "nic".to_string()),
                ("nictype".to_string(), "bridged".to_string()),
                ("parent".to_string(), bridge),
            ]),
        )]))
    } else {
        None
    };

    IncusInstance {
        name: container_name.to_string(),
        status: "Running".to_string(),
        instance_type: "container".to_string(),
        image: Some(cfg.image.clone()),
        storage_pool: cfg.storage_pool.clone(),
        profiles: Vec::new(),
        config: Some(HashMap::from([
            ("user.opdbus.user_id".to_string(), user.id.clone()),
            ("user.opdbus.email".to_string(), user.email.clone()),
            (
                "user.opdbus.assigned_ip".to_string(),
                user.assigned_ip.clone(),
            ),
            (
                "user.opdbus.wireguard_public_key".to_string(),
                user.wg_public_key.clone(),
            ),
            ("user.opdbus.route_id".to_string(), route_id.to_string()),
        ])),
        devices,
    }
}

fn upsert_instance(state: &mut IncusState, instance: IncusInstance) {
    match state
        .instances
        .iter_mut()
        .find(|existing| existing.name == instance.name)
    {
        Some(existing) => *existing = instance,
        None => state.instances.push(instance),
    }
    state.instances.sort_by(|a, b| a.name.cmp(&b.name));
}

fn container_name_for_user(user_id: &str, name_prefix: &str) -> String {
    let mut prefix = normalize_prefix(name_prefix);
    if !prefix.ends_with('-') {
        prefix.push('-');
    }

    let suffix = user_suffix(user_id);
    let mut full_name = format!("{}{}", prefix, suffix);
    if full_name.len() > 63 {
        full_name.truncate(63);
        while full_name.ends_with('-') {
            full_name.pop();
        }
    }
    full_name
}

fn normalize_prefix(prefix: &str) -> String {
    let mut out: String = prefix
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .map(|c| c.to_ascii_lowercase())
        .collect();

    if out.is_empty() {
        out = DEFAULT_NAME_PREFIX.trim_end_matches('-').to_string();
    }

    out
}

fn user_suffix(user_id: &str) -> String {
    let suffix: String = user_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .take(12)
        .collect();

    if suffix.is_empty() {
        "user".to_string()
    } else {
        suffix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_container_name_from_uuid() {
        let name = container_name_for_user("f7f95f64-5f65-4c2d-a47d-4f9c1122af10", "privacy-user-");
        assert_eq!(name, "privacy-user-f7f95f645f65");
    }

    #[test]
    fn sanitizes_non_alnum_characters() {
        let name = container_name_for_user("A B:C", "My Prefix!");
        assert_eq!(name, "myprefix-abc");
    }

    #[test]
    fn falls_back_to_default_prefix_and_suffix() {
        let name = container_name_for_user("----", "");
        assert_eq!(name, "privacy-user-user");
    }

    fn test_user() -> PrivacyUser {
        PrivacyUser {
            id: "user-1".to_string(),
            email: "user@example.com".to_string(),
            email_verified: true,
            created_at: chrono::Utc::now(),
            wg_public_key: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string(),
            wg_private_key_encrypted: "secret".to_string(),
            assigned_ip: "10.100.0.2/32".to_string(),
            privacy_quota_bytes: 1,
            privacy_quota_used_bytes: 0,
            privacy_container_name: None,
            privacy_route_id: None,
            privacy_network_connected: false,
            privacy_network_connected_at: None,
            google_id: None,
            google_email: None,
            api_credentials: None,
        }
    }

    /// Built explicitly rather than via `from_env`: the process environment is
    /// shared across the whole test binary, so reading it here would make these
    /// assertions depend on ambient `PRIVACY_CONTAINER_*` vars.
    fn test_config(attach_bridged_nic: bool) -> PrivacyContainerConfig {
        PrivacyContainerConfig {
            image: DEFAULT_CONTAINER_IMAGE.to_string(),
            name_prefix: DEFAULT_NAME_PREFIX.to_string(),
            device_name: DEFAULT_DEVICE_NAME.to_string(),
            storage_pool: None,
            attach_bridged_nic,
        }
    }

    #[test]
    fn desired_instance_publishes_route_id_in_config() {
        let instance = desired_instance(
            &test_user(),
            "privacy-user-user1",
            "route-a",
            &test_config(false),
        );

        let config = instance.config.as_ref().unwrap();
        assert_eq!(config["user.opdbus.route_id"], "route-a");
        assert_eq!(config["user.opdbus.user_id"], "user-1");
        assert_eq!(config["user.opdbus.assigned_ip"], "10.100.0.2/32");
        assert_eq!(instance.name, "privacy-user-user1");
        assert!(instance.profiles.is_empty());
    }

    #[test]
    fn desired_instance_omits_devices_when_bridged_nic_disabled() {
        let instance = desired_instance(
            &test_user(),
            "privacy-user-user1",
            "route-a",
            &test_config(false),
        );

        assert!(instance.devices.is_none());
    }

    #[test]
    fn desired_instance_attaches_bridged_nic_when_enabled() {
        let cfg = test_config(true);
        let instance = desired_instance(&test_user(), "privacy-user-user1", "route-a", &cfg);

        let devices = instance.devices.as_ref().unwrap();
        let nic = &devices[&cfg.device_name];
        assert_eq!(nic["type"], "nic");
        assert_eq!(nic["nictype"], "bridged");
        // `parent` comes from PRIVACY_CONTAINER_BRIDGE, so only assert it is set.
        assert!(nic.contains_key("parent"));
    }
}
