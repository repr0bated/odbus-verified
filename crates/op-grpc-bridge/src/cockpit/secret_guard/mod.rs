//! Secret_Guard and credential registry for base cockpit evidence records.
//! Kept in base 3tched when the chat interface moved to the container repo
//! (/srv/git/3tched.com crates/op-chat-interface carries its own copy).
pub mod credential_registry;
pub mod guard;

pub use guard::{GuardVerdict, SecretGuard};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CredentialClass {
    #[serde(rename = "sid1")]
    Sid1,
    #[serde(rename = "oia1")]
    Oia1,
    #[serde(rename = "provider-credential")]
    ProviderCredential,
    #[serde(rename = "identity-key")]
    IdentityKey,
    #[serde(rename = "w3-key")]
    W3Key,
    #[serde(rename = "declared-secret-field")]
    DeclaredSecretField,
}
