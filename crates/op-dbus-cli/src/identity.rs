//! Display the canonical configured session, using the same fail-closed
//! human/service selection policy as other identity-aware clients.

use tracing::debug;

/// Canonical location written by `write_projection()` (post projection-removal).
const STATE_PATH: &str = "/dev/shm/opdbus/state/identity_sled.json";

/// Identity context for authenticated calls.
#[derive(Debug, Clone)]
pub struct CliIdentity {
    /// Hex-encoded Blake3 hashed footprint
    pub footprint: String,
    /// Hex-encoded trace ID
    pub trace_id: String,
    /// Session ID (container name / derived identity)
    pub session_id: String,
    /// WireGuard public key (base64)
    pub wireguard_pubkey: String,
    /// Schema mutation index
    pub mutation_index: u64,
}

impl CliIdentity {
    /// Resolve the configured session, or the unambiguous current human.
    /// Never adopt the chatbot merely because it is the first projected row.
    pub fn read() -> Option<Self> {
        let session = match op_identity::session_projection::configured_identity_session() {
            Ok(session) => session,
            Err(error) => {
                debug!(%error, "No unambiguous current CLI identity");
                return None;
            }
        };
        Some(Self {
            footprint: session.genesis?,
            trace_id: session.trace_id,
            session_id: session.session_id,
            wireguard_pubkey: session.wireguard_pubkey,
            mutation_index: session.mutation_index,
        })
    }

    /// Print identity info for `dbus-plugin-cli identity` command.
    pub fn display(&self) {
        println!("Identity (from {}):", STATE_PATH);
        println!("  Session ID:      {}", self.session_id);
        println!("  Footprint:       {}", self.footprint);
        println!("  Trace ID:        {}", self.trace_id);
        println!("  WireGuard Key:   {}", self.wireguard_pubkey);
        println!("  Mutation Index:  {}", self.mutation_index);
    }
}
