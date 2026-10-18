//! Error types for the user-space ingress stack.
//!
//! One error enum spans transport termination, identity extraction and the
//! door handoff so a listener task can report any stage without boxing.

use std::io;

/// Failure from any stage of the user-space ingress path.
#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    /// The listener could not bind its configured socket.
    #[error("bind {addr} failed after {attempts} attempts: {source}")]
    Bind {
        addr: std::net::SocketAddr,
        attempts: u32,
        #[source]
        source: io::Error,
    },

    /// Transport-level framing, handshake or decryption failure.
    #[error("{transport} transport failure: {reason}")]
    Transport {
        transport: &'static str,
        reason: String,
    },

    /// The stream carried no identity credential, or an unusable one.
    ///
    /// Exactly one of SID1 or OIA1 must be present; both is a protocol error.
    #[error("identity rejected: {0}")]
    Identity(String),

    /// The door socket refused or dropped the connection.
    #[error("door handoff to {path} failed: {reason}")]
    Door {
        path: std::path::PathBuf,
        reason: String,
    },

    /// Downstream could not keep up and the bounded queue stayed full.
    ///
    /// Shedding here is deliberate: it bounds memory instead of letting the
    /// ingress buffer grow without limit.
    #[error("ingress backpressure: {0} dropped")]
    Backpressure(&'static str),

    #[error(transparent)]
    Io(#[from] io::Error),
}

impl NetworkError {
    /// Build a transport failure without formatting at every call site.
    pub fn transport(transport: &'static str, reason: impl Into<String>) -> Self {
        Self::Transport {
            transport,
            reason: reason.into(),
        }
    }

    /// Whether the connection manager should try the next configured ingress.
    ///
    /// Transport and door faults are transient by nature. An identity
    /// rejection is a decision, not a fault, so it never triggers fallback.
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::Transport { .. } | Self::Door { .. } | Self::Io(_)
        )
    }
}
