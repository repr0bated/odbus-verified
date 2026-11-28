//! Forward the configured service's authored MQTT credential; never mint identity.

use anyhow::{Result, ensure};
use op_identity::session_projection::{SessionIdentity, resolve_identity_credential_session};
use sha2::{Digest, Sha256};

pub(crate) struct MqttAuthentication {
    pub owner_key: String,
    pub password: String,
}

pub(crate) struct ResponderIdentity {
    pub principal_id: String,
    pub session_id: String,
    pub wireguard_pubkey: String,
    genesis: String,
    credential_digest: [u8; 32],
}

impl ResponderIdentity {
    pub fn load(principal_id: String, session_id: String, wireguard_pubkey: String)
        -> Result<(Self, MqttAuthentication)>
    {
        let session = authored_session(&principal_id, &session_id, &wireguard_pubkey)?;
        let genesis = session.genesis().map_err(|_| anyhow::anyhow!("responder genesis unavailable"))?.to_owned();
        let password = session.inline_sealed_id()
            .map_err(|_| anyhow::anyhow!("authored responder credential unavailable"))?.to_owned();
        let owner_key = mqtt_owner_key(&principal_id, &session_id, &genesis)?;
        let credential_digest = Sha256::digest(password.as_bytes()).into();
        Ok((Self { principal_id, session_id, wireguard_pubkey, genesis, credential_digest },
            MqttAuthentication { owner_key, password }))
    }

    pub fn revalidate(&self) -> Result<()> {
        let session = authored_session(&self.principal_id, &self.session_id, &self.wireguard_pubkey)?;
        let credential = session.inline_sealed_id()
            .map_err(|_| anyhow::anyhow!("authored responder credential unavailable"))?;
        let digest: [u8; 32] = Sha256::digest(credential.as_bytes()).into();
        ensure!(session.genesis().ok() == Some(self.genesis.as_str())
            && digest == self.credential_digest,
            "responder identity changed; stop without replay");
        Ok(())
    }
}

fn authored_session(principal: &str, session: &str, key: &str) -> Result<SessionIdentity> {
    ensure!(uuid::Uuid::parse_str(session)?.to_string() == session
        && uuid::Uuid::parse_str(principal)?.to_string() == principal,
        "configured responder identifiers are not canonical");
    ensure!(op_identity::session::derive_session_id(key) == session
        && op_identity::session::derive_principal_id(key) == principal,
        "configured responder identifiers differ from its WireGuard binding");
    let identity = resolve_identity_credential_session(Some(session))
        .map_err(|_| anyhow::anyhow!("configured responder authored identity unavailable"))?;
    ensure!(identity.is_service_principal() && identity.is_current()
        && identity.session_id == session && identity.wireguard_pubkey == key,
        "configured responder is not the current bound service identity");
    // Validates the exact authored envelope against its projection and transport.
    // The broker independently verifies its signature, registry and current grants.
    identity.sealed_id_header_for_transport("mqtt")
        .map_err(|_| anyhow::anyhow!("authored responder credential does not admit MQTT"))?;
    Ok(identity)
}

/// Must match runtime_mqtt_auth::owner_key_for_claims, not sha256(principal).
fn mqtt_owner_key(principal: &str, session: &str, genesis: &str) -> Result<String> {
    ensure!(!principal.is_empty() && !session.is_empty() && !genesis.is_empty(),
        "responder MQTT scope is incomplete");
    let scope = serde_json::to_vec(&[principal, session, genesis])?;
    Ok(format!("{:x}", Sha256::digest(scope)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mqtt_owner_key_binds_the_exact_identity_triple() {
        let key = mqtt_owner_key("principal", "session", "genesis").unwrap();
        assert_eq!(key, format!("{:x}", Sha256::digest(br#"["principal","session","genesis"]"#)));
        assert_ne!(key, mqtt_owner_key("principal", "another-session", "genesis").unwrap());
        assert_ne!(key, mqtt_owner_key("principal", "session", "another-genesis").unwrap());
        assert!(mqtt_owner_key("principal", "session", "").is_err());
    }
}
