//! Shared identity-to-container and human binding checks.
use op_plugins::state_plugins::identity_sled::ContainerIdentitySled;
use op_plugins::state_plugins::incus::IncusInstance;

pub(crate) const CONTROL_PLANE_CHATBOT_CONTAINER: &str = "3tchedChatbot";
pub(crate) const RESERVED_CHATBOT_CONTAINER: &str = "3tchedCtlChatbot";

pub fn configured_chatbot_key() -> Option<String> {
    std::env::var("OP_CONTROL_PLANE_CHATBOT_WIREGUARD_PUBKEY")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn chatbot_binding_matches(
    sled: &ContainerIdentitySled,
    instance: &IncusInstance,
    configured_session: Option<&str>,
    configured_key: Option<&str>,
) -> bool {
    instance.name == CONTROL_PLANE_CHATBOT_CONTAINER
        && instance.instance_type == "container"
        && sled.principal_kind.as_deref() == Some("service")
        && configured_session == Some(sled.session_id.as_str())
        && configured_key == Some(sled.wireguard_pubkey.as_str())
        && op_identity::session::derive_session_id(&sled.wireguard_pubkey) == sled.session_id
        && instance.config.as_ref().is_some_and(|config| {
            config.get("user.opdbus.session_id") == Some(&sled.session_id)
                && config.get("user.opdbus.wireguard_public_key") == Some(&sled.wireguard_pubkey)
        })
}

pub fn identity_instance_matches(sled: &ContainerIdentitySled, instance: &IncusInstance) -> bool {
    // The native name is a label. Ownership remains the stored UUID, session
    // and key binding, not the label's number or its creation receipt.
    let named = (op_cozo_store::is_numbered_native_name(&instance.name)
        || (instance.name == RESERVED_CHATBOT_CONTAINER
            && sled.principal_kind.as_deref() == Some("service")
            && control_plane_chatbot_session_id().as_deref() == Some(sled.session_id.as_str())
            && configured_chatbot_key().as_deref() == Some(sled.wireguard_pubkey.as_str())
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
    named
        || (instance.name == sled.session_id && instance.instance_type == "container")
        || chatbot_binding_matches(
            sled,
            instance,
            control_plane_chatbot_session_id().as_deref(),
            configured_chatbot_key().as_deref(),
        )
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
}
