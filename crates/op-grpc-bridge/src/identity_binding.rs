//! Shared identity-to-container and human binding checks.
use op_plugins::state_plugins::identity_sled::ContainerIdentitySled;
use op_plugins::state_plugins::incus::IncusInstance;

/// Historical adoption label retained only to validate original provenance.
/// New preparation and in-place rename use the one current native label.
/// Neither label is identity authority without the exact session/key/UUID.
pub(crate) const CONTROL_PLANE_CHATBOT_CONTAINER: &str = "3tchedChatbot";
pub(crate) const RESERVED_CHATBOT_CONTAINER: &str = op_cozo_store::CONTROL_PLANE_NATIVE_NAME;

pub fn configured_chatbot_key() -> Option<String> {
    std::env::var("OP_CONTROL_PLANE_CHATBOT_WIREGUARD_PUBKEY")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// True when `instance` is the configured control-plane chatbot container,
/// whatever its native name (e.g. a cockpit-named container sprouted from the
/// seed rootfs). This is the adoption-time check: it must not require the
/// sled's stored instance binding, because adoption is what creates it. The
/// instance must carry the exact configured session/key in its own config,
/// and the key must derive the session.
pub fn configured_chatbot_instance_matches(
    sled: &ContainerIdentitySled,
    instance: &IncusInstance,
) -> bool {
    sled.principal_kind.as_deref() == Some("service")
        && control_plane_chatbot_session_id().as_deref() == Some(sled.session_id.as_str())
        && configured_chatbot_key().as_deref() == Some(sled.wireguard_pubkey.as_str())
        && op_identity::session::derive_session_id(&sled.wireguard_pubkey) == sled.session_id
        && instance.instance_type == "container"
        && instance.config.as_ref().is_some_and(|config| {
            config.get("user.opdbus.session_id") == Some(&sled.session_id)
                && config.get("user.opdbus.wireguard_public_key") == Some(&sled.wireguard_pubkey)
                && config
                    .get("volatile.uuid")
                    .is_some_and(|value| uuid::Uuid::parse_str(value).is_ok())
        })
}

pub fn identity_instance_matches(sled: &ContainerIdentitySled, instance: &IncusInstance) -> bool {
    // The native name is a label. Ownership remains the stored UUID, session
    // and key binding, not the label's number or its creation receipt.
    // A sled validates from its own record: the instance config must carry
    // the sled's exact session and key, and the key must derive the session.
    // Requiring the *currently configured* control-plane session here would
    // make every other service sled invalid whenever the configured session
    // rotates, and startup recovery could never publish. The stricter
    // configured-session check lives in configured_chatbot_instance_matches.
    let named = (op_cozo_store::is_numbered_native_name(&instance.name)
        || (sled.principal_kind.as_deref() == Some("service")
            && op_identity::session::derive_session_id(&sled.wireguard_pubkey) == sled.session_id))
        && instance.instance_type == "container"
        && instance.config.as_ref().is_some_and(|c| {
            c.get("user.opdbus.session_id") == Some(&sled.session_id)
                && c.get("user.opdbus.wireguard_public_key") == Some(&sled.wireguard_pubkey)
                && c.get("volatile.uuid")
                    .is_some_and(|v| uuid::Uuid::parse_str(v).is_ok())
        })
        && sled.instance.as_ref().is_some_and(|stored| {
            stored.name == instance.name
                && stored
                    .config
                    .as_ref()
                    .zip(instance.config.as_ref())
                    .is_some_and(|(a, b)| a.get("volatile.uuid") == b.get("volatile.uuid"))
        });
    named || (instance.name == sled.session_id && instance.instance_type == "container")
}

pub fn identity_session_binding_matches(sled: &ContainerIdentitySled) -> bool {
    match sled.instance.as_ref() {
        Some(instance) => identity_instance_matches(sled, instance),
        // The identity owner explicitly initializes humans without per-account
        // containers. Actor class comes from the committed record, not SID1 or
        // request metadata; any attached instance must still match above.
        None => sled.principal_kind.as_deref() == Some("human"),
    }
}

pub fn validate_sled(sled: &ContainerIdentitySled) -> anyhow::Result<()> {
    anyhow::ensure!(!sled.session_id.is_empty(), "identity session id is empty");
    anyhow::ensure!(
        sled.mutation_index <= i64::MAX as u64,
        "mutation index overflow"
    );
    if let Some(instance) = &sled.instance {
        anyhow::ensure!(
            identity_instance_matches(sled, instance),
            "identity instance does not match session container"
        );
    }
    if sled.is_anchored() {
        anyhow::ensure!(
            sled.sealed_id
                .as_deref()
                .is_some_and(|value| !value.is_empty()),
            "anchored identity lacks authored SID1"
        );
    }
    Ok(())
}

/// Session id of the control-plane chatbot, from
/// `OP_CONTROL_PLANE_CHATBOT_SESSION_ID`.
pub fn control_plane_chatbot_session_id() -> Option<String> {
    std::env::var("OP_CONTROL_PLANE_CHATBOT_SESSION_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_and_container_bindings_are_checked_from_the_record() {
        let mut sled: ContainerIdentitySled = serde_json::from_value(serde_json::json!({
            "session_id": "session", "wireguard_pubkey": "key", "principal_kind": "human"
        }))
        .unwrap();
        assert!(identity_session_binding_matches(&sled));
        sled.principal_kind = Some("service".into());
        assert!(!identity_session_binding_matches(&sled));
        sled.instance = Some(
            serde_json::from_value(serde_json::json!({
                "name": "session", "status": "Stopped", "type": "container"
            }))
            .unwrap(),
        );
        assert!(identity_session_binding_matches(&sled));
        sled.instance.as_mut().unwrap().name = "other".into();
        assert!(!identity_session_binding_matches(&sled));
    }

    #[test]
    fn cockpit_named_container_matches_configured_chatbot_without_stored_instance() {
        use base64::Engine as _;
        let key = base64::engine::general_purpose::STANDARD.encode([7u8; 32]);
        let session = op_identity::session::derive_session_id(&key);
        // SAFETY: this test is the only one in this module that reads these
        // variables, and it sets both before any read.
        unsafe {
            std::env::set_var("OP_CONTROL_PLANE_CHATBOT_SESSION_ID", &session);
            std::env::set_var("OP_CONTROL_PLANE_CHATBOT_WIREGUARD_PUBKEY", &key);
        }
        let sled: ContainerIdentitySled = serde_json::from_value(serde_json::json!({
            "session_id": session, "wireguard_pubkey": key, "principal_kind": "service"
        }))
        .unwrap();
        assert!(sled.instance.is_none());
        let instance: IncusInstance = serde_json::from_value(serde_json::json!({
            "name": "3tchedCockpitBot", "status": "Running", "type": "container",
            "config": {
                "user.opdbus.session_id": session,
                "user.opdbus.wireguard_public_key": key,
                "volatile.uuid": "00000000-0000-4000-8000-000000000001"
            }
        }))
        .unwrap();
        assert!(configured_chatbot_instance_matches(&sled, &instance));
        // A name alone, or a mismatched key in the instance config, never matches.
        let mut wrong = instance.clone();
        wrong.config.as_mut().unwrap().insert(
            "user.opdbus.wireguard_public_key".into(),
            base64::engine::general_purpose::STANDARD.encode([9u8; 32]),
        );
        assert!(!configured_chatbot_instance_matches(&sled, &wrong));
        // SAFETY: same as above; do not leak the fixture identity to other tests.
        unsafe {
            std::env::remove_var("OP_CONTROL_PLANE_CHATBOT_SESSION_ID");
            std::env::remove_var("OP_CONTROL_PLANE_CHATBOT_WIREGUARD_PUBKEY");
        }
    }
}
