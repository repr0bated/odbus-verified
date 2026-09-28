//! Operator-owned NextDNS assignments, captured once before identity minting.
//!
//! These claims describe an assigned profile/device, not observed DNS traffic.
//! Request arguments and headers never configure them.
use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use base64::Engine as _;
use op_identity::sealed_id::NextDnsContext;
use serde::Deserialize;

const ENV: &str = "OP_IDENTITY_NEXTDNS_BINDINGS";
const MAX_CONFIG_BYTES: usize = 65_536;

#[derive(Default)]
pub(crate) struct NextDnsBindings(BTreeMap<String, NextDnsContext>);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Assignment {
    wireguard_pubkey: String,
    profile_id: String,
    device_name: String,
}

impl NextDnsBindings {
    pub(crate) fn from_env() -> Result<Self> {
        match std::env::var(ENV) {
            Ok(value) => Self::parse(&value).context("invalid NextDNS identity assignments"),
            Err(std::env::VarError::NotPresent) => Ok(Self::default()),
            Err(_) => anyhow::bail!("NextDNS identity assignments are not UTF-8"),
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self> {
        ensure!(
            value.len() <= MAX_CONFIG_BYTES,
            "NextDNS assignment configuration is too large"
        );
        let entries: Vec<Assignment> = serde_json::from_str(value)?;
        ensure!(
            entries.len() <= 256,
            "too many NextDNS identity assignments"
        );
        let mut bindings = BTreeMap::new();
        for entry in entries {
            let key = base64::engine::general_purpose::STANDARD.decode(&entry.wireguard_pubkey)?;
            ensure!(
                key.len() == 32
                    && base64::engine::general_purpose::STANDARD.encode(&key)
                        == entry.wireguard_pubkey,
                "NextDNS assignment requires a canonical WireGuard public key"
            );
            let context = NextDnsContext {
                profile_id: entry.profile_id,
                device_name: entry.device_name,
            };
            context.validate()?;
            ensure!(
                bindings.insert(entry.wireguard_pubkey, context).is_none(),
                "duplicate NextDNS WireGuard assignment"
            );
        }
        Ok(Self(bindings))
    }

    pub(crate) fn get(&self, key: &str) -> Option<&NextDnsContext> {
        self.0.get(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: u8) -> serde_json::Value {
        serde_json::json!({
            "wireguard_pubkey": base64::engine::general_purpose::STANDARD.encode([key; 32]),
            "profile_id": "689ec7", "device_name": "Ghostbridge-Incus"
        })
    }

    #[test]
    fn assignment_is_per_key_without_global_fallback() {
        let entry = entry(1);
        let bindings = NextDnsBindings::parse(&serde_json::json!([entry]).to_string()).unwrap();
        assert_eq!(
            bindings
                .get(entry["wireguard_pubkey"].as_str().unwrap())
                .unwrap()
                .profile_id,
            "689ec7"
        );
        assert!(
            bindings
                .get(&base64::engine::general_purpose::STANDARD.encode([2; 32]))
                .is_none()
        );
        assert!(NextDnsBindings::parse("[]").unwrap().0.is_empty());
    }

    #[test]
    fn rejects_ambiguous_or_malformed_assignments() {
        let entry = entry(1);
        assert!(NextDnsBindings::parse(&serde_json::json!([entry, entry]).to_string()).is_err());
        for (field, value) in [
            ("wireguard_pubkey", "not-a-key"),
            ("profile_id", "invalid"),
            ("device_name", ""),
            ("unexpected", "value"),
        ] {
            let mut invalid = entry.clone();
            invalid[field] = value.into();
            assert!(NextDnsBindings::parse(&serde_json::json!([invalid]).to_string()).is_err());
        }
        assert!(NextDnsBindings::parse(&" ".repeat(MAX_CONFIG_BYTES + 1)).is_err());
    }
}
