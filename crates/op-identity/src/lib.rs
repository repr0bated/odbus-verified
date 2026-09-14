//! Identity crate – WireGuard pubkey as identity + OAuth token cache via
//! org.freedesktop.secrets. Zero passwords; the WireGuard handshake is the login.

#![deny(rustdoc::broken_intra_doc_links)]

pub mod gcloud_auth;
pub mod identity_vault;
pub mod oracle_assertion;
pub mod recovery;
pub mod registration;
pub mod schema_bridge;
pub mod sealed_id;
pub mod session;
pub mod session_genesis;
pub mod session_projection;
#[deprecated(
    note = "This module is deprecated; WG interface discovery is kept for compatibility only and should be moved to explicit identity injection paths."
)]
pub mod wg;
pub mod wireguard;

pub use gcloud_auth::GCloudAuth;
pub use recovery::{
    derive_keypair, generate_seed, load_mnemonic_local, mnemonic_to_seed, provision_local_identity,
    recover_keypair, seed_to_mnemonic, store_mnemonic_local,
};
pub use registration::{generate_magic_link_token, generate_wireguard_keypair, WireGuardKeyPair};
pub use schema_bridge::{
    resolve_verified_session, schema_catalog_hash, schema_catalog_hash_previous,
    verify_session_genesis, SessionGenesisVerifyError, SubidCategory, SubidTaxonomy,
};
pub use session::{Session, SessionManager};
pub use session_projection::{
    configured_identity_session, read_identity_credential_sessions, read_identity_sessions,
    resolve_identity_credential_session, resolve_identity_session, SessionIdentity,
    SessionProjectionError, PRINCIPAL_KIND_HUMAN, PRINCIPAL_KIND_SERVICE, SESSION_SELECTOR_ENV,
};

#[allow(deprecated)]
pub use wg::{get_local_pubkey, get_peer_pubkey};
pub use wireguard::{PeerInfo, WireGuardIdentity};
