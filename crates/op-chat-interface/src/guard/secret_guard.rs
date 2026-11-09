//! `checkItem` detection (spec tasks 3.1, 3.5, 3.7): held values,
//! SID1/OIA1 envelopes (interim D-18 grammar; real grammar is task 19.7)
//! and declared-secret JSON fields. Fail-closed: detection, internal
//! error, or exceeding the 2 000 ms check limit withholds the item.

use super::credential_registry::CredentialRegistry;
use crate::types::CredentialClass;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub const CHECK_LIMIT: Duration = Duration::from_millis(2000);
const SEALED_ID_SUBID: &str = "src.service.identity-sled.sealed-id@v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputChannel {
    ModelContext,
    UiStream,
    Log,
    Telemetry,
    PersistedServer,
    PersistedClient,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardVerdict {
    Clean,
    Blocked { cls: CredentialClass },
    Error { kind: GuardErrorKind },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardErrorKind {
    Timeout,
    Internal,
}

impl GuardVerdict {
    pub fn is_clean(&self) -> bool {
        matches!(self, Self::Clean)
    }
}

pub struct SecretGuard {
    registry: CredentialRegistry,
}

impl Default for SecretGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretGuard {
    pub fn new() -> Self {
        Self {
            registry: CredentialRegistry::new(),
        }
    }

    pub fn register_held(&mut self, cls: CredentialClass, value: &[u8]) {
        self.registry.register_held(cls, value);
    }

    pub fn check_item(&self, item: &[u8]) -> GuardVerdict {
        let started = Instant::now();
        let verdict = self.check(item);
        if started.elapsed() > CHECK_LIMIT {
            return GuardVerdict::Error {
                kind: GuardErrorKind::Timeout,
            };
        }
        verdict
    }

    fn check(&self, item: &[u8]) -> GuardVerdict {
        for (cls, needle) in self.registry.needles() {
            if contains(item, needle) {
                return GuardVerdict::Blocked { cls: *cls };
            }
        }
        if let Some(cls) = envelope_hit(item) {
            return GuardVerdict::Blocked { cls };
        }
        if declared_secret_hit(item) {
            return GuardVerdict::Blocked {
                cls: CredentialClass::DeclaredSecretField,
            };
        }
        GuardVerdict::Clean
    }

    /// Fail-closed output gate (task 3.5): the whole item is withheld on
    /// detection or guard error; no partial item is emitted.
    pub fn gate(&self, _channel: OutputChannel, item: &[u8]) -> Result<Vec<u8>, GuardVerdict> {
        match self.check_item(item) {
            GuardVerdict::Clean => Ok(item.to_vec()),
            verdict => Err(verdict),
        }
    }

    /// Class-only redaction marker; never includes any part of the value.
    pub fn redaction_marker(cls: CredentialClass) -> String {
        let name = serde_json::to_value(cls)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "credential".to_string());
        format!("[redacted:{name}]")
    }
}

pub(crate) fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && needle.len() <= haystack.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn envelope_hit(item: &[u8]) -> Option<CredentialClass> {
    let text = String::from_utf8_lossy(item);
    for (prefix, cls) in [
        ("SID1", CredentialClass::Sid1),
        ("OIA1", CredentialClass::Oia1),
    ] {
        let mut search = text.as_ref();
        while let Some(at) = search.find(prefix) {
            let rest = &search[at + prefix.len()..];
            let count = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || "+/=_.:-".contains(*c))
                .count();
            if count >= 16 {
                return Some(cls);
            }
            search = &search[at + prefix.len()..];
        }
    }
    None
}

fn declared_secret_hit(item: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(item) else {
        return false;
    };
    fn walk(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(map) => {
                for (key, member) in map {
                    if key == "sealed_id" {
                        return true;
                    }
                    if key == "x-oscal-subid"
                        && member.as_str() == Some(SEALED_ID_SUBID)
                    {
                        return true;
                    }
                    if walk(member) {
                        return true;
                    }
                }
                false
            }
            serde_json::Value::Array(items) => items.iter().any(walk),
            _ => false,
        }
    }
    walk(&value)
}
