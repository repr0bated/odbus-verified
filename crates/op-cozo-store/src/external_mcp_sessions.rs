//! Durable external human MCP anchors. These are never container or SID1 records.
use super::*;
use base64::Engine as _;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExternalMcpSessionRecord {
    pub session_id: String,
    pub principal_id: String,
    pub peer_key: String,
    pub origin: String,
    pub issuer_key_id: String,
    pub allowed_ip_scope: String,
    pub signed_inner_ip: String,
    pub first_assertion_digest: String,
    pub genesis: String,
    pub created_at: i64,
    pub chain_head: String,
    pub catalog_hash: String,
    pub head_timestamp: i64,
}

impl ExternalMcpSessionRecord {
    pub fn canonical_session_id(principal: &str, key: &str, issuer: &str, scope: &str) -> String {
        let canonical = serde_json::to_vec(&(
            "opdbus/external-mcp-session/v1",
            principal,
            key,
            issuer,
            scope,
        ))
        .expect("strings serialize");
        let digest = blake3::hash(&canonical);
        let mut bytes = [0_u8; 16];
        bytes.copy_from_slice(&digest.as_bytes()[..16]);
        bytes[6] = (bytes[6] & 0x0f) | 0x80;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        let text = hex::encode(bytes);
        format!(
            "{}-{}-{}-{}-{}",
            &text[..8],
            &text[8..12],
            &text[12..16],
            &text[16..20],
            &text[20..]
        )
    }

    pub fn canonical_origin(principal: &str, key: &str, issuer: &str, scope: &str) -> String {
        let material =
            serde_json::to_vec(&[issuer, principal, key, scope]).expect("strings serialize");
        let digest = blake3::derive_key("opdbus wireguard MCP origin v1", &material);
        format!("https://wg-{}.opdbus.invalid", &hex::encode(digest)[..32])
    }

    fn validate(&self) -> Result<(), CozoError> {
        for value in [
            &self.session_id,
            &self.principal_id,
            &self.peer_key,
            &self.origin,
            &self.issuer_key_id,
            &self.allowed_ip_scope,
            &self.genesis,
            &self.chain_head,
            &self.catalog_hash,
        ] {
            if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
                return Err(CozoError::Other("invalid external MCP anchor".into()));
            }
        }
        if self.created_at < 0
            || self.head_timestamp < 0
            || op_identity::session::derive_principal_id(&self.peer_key) != self.principal_id
            || self.session_id == op_identity::session::derive_session_id(&self.peer_key)
        {
            return Err(CozoError::Other(
                "external MCP principal/session binding invalid".into(),
            ));
        }
        let hex_valid = |text: &str, lengths: &[usize]| {
            lengths.contains(&text.len())
                && text
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        };
        if !hex_valid(&self.genesis, &[64])
            || !hex_valid(&self.catalog_hash, &[64])
            || !hex_valid(&self.chain_head, &[32, 64])
            || self.session_id.len() != 36
            || !self.origin.starts_with("https://wg-")
            || !self.origin.ends_with(".opdbus.invalid")
        {
            return Err(CozoError::Other(
                "external MCP anchor provenance invalid".into(),
            ));
        }
        let (ip, prefix) = self
            .allowed_ip_scope
            .rsplit_once('/')
            .ok_or_else(|| CozoError::Other("external MCP scope is not a host route".into()))?;
        let ip: std::net::IpAddr = ip
            .parse()
            .map_err(|_| CozoError::Other("external MCP scope address invalid".into()))?;
        if prefix != if ip.is_ipv4() { "32" } else { "128" }
            || ip.to_string() != self.signed_inner_ip
            || self.allowed_ip_scope != format!("{ip}/{prefix}")
        {
            return Err(CozoError::Other(
                "external MCP scope is not a host route".into(),
            ));
        }
        if self.session_id
            != Self::canonical_session_id(
                &self.principal_id,
                &self.peer_key,
                &self.issuer_key_id,
                &self.allowed_ip_scope,
            )
            || self.origin
                != Self::canonical_origin(
                    &self.principal_id,
                    &self.peer_key,
                    &self.issuer_key_id,
                    &self.allowed_ip_scope,
                )
            || !hex_valid(&self.first_assertion_digest, &[64])
        {
            return Err(CozoError::Other(
                "external MCP canonical scope mismatch".into(),
            ));
        }
        let key = base64::engine::general_purpose::STANDARD
            .decode(&self.peer_key)
            .map_err(|_| CozoError::Other("invalid external MCP peer key".into()))?;
        let key: [u8; 32] = key
            .try_into()
            .map_err(|_| CozoError::Other("invalid external MCP peer key length".into()))?;
        if base64::engine::general_purpose::STANDARD.encode(key) != self.peer_key {
            return Err(CozoError::Other(
                "noncanonical external MCP peer key".into(),
            ));
        }
        let chain = hex::decode(&self.chain_head)
            .map_err(|_| CozoError::Other("invalid chain digest".into()))?;
        let catalog: [u8; 32] = hex::decode(&self.catalog_hash)
            .map_err(|_| CozoError::Other("invalid catalog digest".into()))?
            .try_into()
            .map_err(|_| CozoError::Other("invalid catalog digest length".into()))?;
        let genesis = op_identity::session_genesis::mint_genesis(
            &key,
            &chain,
            self.head_timestamp,
            &catalog,
            self.created_at,
        );
        if hex::encode(genesis) != self.genesis {
            return Err(CozoError::Other(
                "external MCP genesis inputs mismatch".into(),
            ));
        }
        Ok(())
    }

    fn same_binding(&self, other: &Self) -> bool {
        self.session_id == other.session_id
            && self.principal_id == other.principal_id
            && self.peer_key == other.peer_key
            && self.origin == other.origin
            && self.issuer_key_id == other.issuer_key_id
            && self.allowed_ip_scope == other.allowed_ip_scope
            && self.signed_inner_ip == other.signed_inner_ip
    }
}

impl CozoGraphShuttle {
    /// Atomically retain the first genesis for this exact origin. Cloned handles
    /// share the owner's lock, and RocksDB excludes other process owners.
    pub fn get_or_create_external_mcp_session(
        &self,
        proposed: &ExternalMcpSessionRecord,
    ) -> Result<ExternalMcpSessionRecord, CozoError> {
        proposed.validate()?;
        let _guard = self
            .identity_transaction
            .lock()
            .map_err(|_| CozoError::Other("external MCP session owner poisoned".into()))?;
        // The ingress rechecks principal revocation after persistence too:
        // principal revocation uses Cozo's own transaction, not this lock.
        let principal = self
            .get_human_principal_by_pubkey(&proposed.peer_key)?
            .ok_or_else(|| CozoError::Other("external MCP principal is not registered".into()))?;
        if principal.principal_id != proposed.principal_id || principal.revoked_at != 0 {
            return Err(CozoError::Other(
                "external MCP principal is revoked or mismatched".into(),
            ));
        }
        if let Some(current) = self.get_external_mcp_session(&proposed.session_id)? {
            if !current.same_binding(proposed) {
                return Err(CozoError::Other(
                    "external MCP origin binding changed".into(),
                ));
            }
            return Ok(current);
        }
        self.run_query(
            "?[session_id, principal_id, document] <- [[$session, $principal, $document]] :put external_mcp_sessions {session_id => principal_id, document}",
            Some(serde_json::json!({"session": proposed.session_id, "principal": proposed.principal_id,
                "document": serde_json::to_string(proposed)?})),
        )?;
        Ok(proposed.clone())
    }

    pub fn get_external_mcp_session(
        &self,
        session_id: &str,
    ) -> Result<Option<ExternalMcpSessionRecord>, CozoError> {
        let result = self.run_query(
            "?[document] := *external_mcp_sessions{session_id, document}, session_id = $session",
            Some(serde_json::json!({"session": session_id})),
        )?;
        let rows = result
            .as_array()
            .ok_or_else(|| CozoError::Other("invalid external MCP rows".into()))?;
        rows.first()
            .map(|row| {
                let text = row["document"]
                    .as_str()
                    .ok_or_else(|| CozoError::Other("invalid external MCP document".into()))?;
                let record: ExternalMcpSessionRecord = serde_json::from_str(text)?;
                record.validate()?;
                if record.session_id != session_id {
                    return Err(CozoError::Other(
                        "external MCP session attribution mismatch".into(),
                    ));
                }
                Ok(record)
            })
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> ExternalMcpSessionRecord {
        let key = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        let principal = op_identity::session::derive_principal_id(key);
        let scope = "10.0.0.5/32";
        let issuer = "decoy-test";
        let chain = [0xcd; 16];
        let catalog = [0xef; 32];
        ExternalMcpSessionRecord {
            session_id: ExternalMcpSessionRecord::canonical_session_id(
                &principal, key, issuer, scope,
            ),
            origin: ExternalMcpSessionRecord::canonical_origin(&principal, key, issuer, scope),
            principal_id: principal,
            peer_key: key.into(),
            issuer_key_id: issuer.into(),
            allowed_ip_scope: scope.into(),
            signed_inner_ip: "10.0.0.5".into(),
            first_assertion_digest: "ab".repeat(32),
            genesis: hex::encode(op_identity::session_genesis::mint_genesis(
                &[0; 32], &chain, 90, &catalog, 100,
            )),
            created_at: 100,
            chain_head: hex::encode(chain),
            catalog_hash: hex::encode(catalog),
            head_timestamp: 90,
        }
    }

    fn register(store: &CozoGraphShuttle, row: &ExternalMcpSessionRecord) {
        store
            .put_human_principal(&HumanPrincipalRecord {
                principal_id: row.principal_id.clone(),
                human_pubkey: row.peer_key.clone(),
                display_alias: String::new(),
                registered_at: 1,
                revoked_at: 0,
            })
            .unwrap();
    }
    #[test]
    fn external_anchor_survives_restart_and_preserves_first_genesis() {
        let dir = tempfile::tempdir().unwrap();
        let row = fixture();
        {
            let store = CozoGraphShuttle::new_persistent(dir.path().join("cozo")).unwrap();
            register(&store, &row);
            assert_eq!(store.get_or_create_external_mcp_session(&row).unwrap(), row);
        }
        let store = CozoGraphShuttle::new_persistent(dir.path().join("cozo")).unwrap();
        let mut next = row.clone();
        next.created_at += 1;
        next.genesis = hex::encode(op_identity::session_genesis::mint_genesis(
            &[0; 32],
            &[0xcd; 16],
            90,
            &[0xef; 32],
            next.created_at,
        ));
        assert_eq!(
            store.get_or_create_external_mcp_session(&next).unwrap(),
            row
        );
        next.origin = "https://wg-other.opdbus.invalid".into();
        assert!(store.get_or_create_external_mcp_session(&next).is_err());
    }
    #[test]
    fn concurrent_external_anchor_creation_has_one_genesis() {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        let row = fixture();
        register(&store, &row);
        let workers: Vec<_> = (0..8)
            .map(|n| {
                let store = store.clone();
                let mut proposed = row.clone();
                proposed.created_at += n;
                proposed.genesis = hex::encode(op_identity::session_genesis::mint_genesis(
                    &[0; 32],
                    &[0xcd; 16],
                    90,
                    &[0xef; 32],
                    proposed.created_at,
                ));
                std::thread::spawn(move || {
                    store.get_or_create_external_mcp_session(&proposed).unwrap()
                })
            })
            .collect();
        let rows: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
        assert!(rows.iter().all(|r| r == &rows[0]));
    }
    #[test]
    fn unregistered_revoked_and_container_session_bindings_are_denied() {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        let mut row = fixture();
        assert!(store.get_or_create_external_mcp_session(&row).is_err());
        register(&store, &row);
        store.revoke_human_principal(&row.principal_id, 5).unwrap();
        assert!(store.get_or_create_external_mcp_session(&row).is_err());
        row.session_id = op_identity::session::derive_session_id(&row.peer_key);
        assert!(store.get_or_create_external_mcp_session(&row).is_err());
    }
    #[test]
    fn poisoned_external_origin_session_and_genesis_are_rejected() {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        let row = fixture();
        register(&store, &row);
        store.get_or_create_external_mcp_session(&row).unwrap();
        for field in [
            "session_id",
            "origin",
            "genesis",
            "signed_inner_ip",
            "catalog_hash",
        ] {
            let mut document = serde_json::to_value(&row).unwrap();
            document[field] = match field {
                "session_id" => "22222222-2222-8222-8222-222222222222".into(),
                "origin" => "https://wg-poison.opdbus.invalid".into(),
                "signed_inner_ip" => "10.0.0.6".into(),
                _ => "01".repeat(32).into(),
            };
            store.run_query(
                "?[session_id, principal_id, document] <- [[$session, $principal, $document]] :put external_mcp_sessions {session_id => principal_id, document}",
                Some(serde_json::json!({"session": row.session_id, "principal": row.principal_id,
                    "document": serde_json::to_string(&document).unwrap()})),
            ).unwrap();
            assert!(
                store.get_external_mcp_session(&row.session_id).is_err(),
                "{field}"
            );
        }
    }
}
