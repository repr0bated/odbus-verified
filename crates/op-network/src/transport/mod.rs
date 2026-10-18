//! Encrypted user-space ingress transports.
//!
//! Every external client reaches this host through one of three encrypted
//! transports terminated here, in user space: MASQUE (HTTP/3 QUIC), WARP
//! (WireGuard) and reverse SSH. No kernel tunnel device is created, so the
//! whole stack can be stopped and recreated without leaving routing or
//! interface state behind — that is the property the GRE pair lacked.
//!
//! A listener yields a byte stream together with the credential the decoy
//! minted for it. Identity extraction is the transport's job because only the
//! transport knows where its peer put the bytes; everything downstream sees
//! the same [`SealedIdentityBlob`] regardless of which transport it came from.

pub mod masque;
pub mod ssh_tunnel;
pub mod warp;

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::error::NetworkError;
use crate::identity::sealed_id::SealedIdentityBlob;

/// Outer frame budget for the UDP transports.
///
/// Both tunnels ride a 1500-byte path. QUIC and WireGuard each add their own
/// header, and gRPC streams were truncated in production when the outer frame
/// was left at the interface default, so the clamp is applied at the listener
/// rather than trusted from the peer.
pub const TUNNEL_MTU: u16 = 1420;

/// How a listener was configured, for logs and fallback ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportKind {
    Masque,
    Warp,
    SshTunnel,
}

impl TransportKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Masque => "masque",
            Self::Warp => "warp",
            Self::SshTunnel => "ssh",
        }
    }
}

/// Accepts encrypted streams from the decoy and strips the outer protocol.
///
/// Implementors own their socket for the lifetime of the listener. `accept`
/// is cancel-safe: dropping the future loses at most the stream being
/// negotiated, never one already handed to a caller.
#[async_trait]
pub trait EncryptedTransportListener: Send + Sync {
    /// Decrypted bidirectional stream carrying the inner gRPC bytes.
    type Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static;

    /// Which transport this listener terminates.
    fn kind(&self) -> TransportKind;

    /// Accept one decrypted stream and the credential its peer presented.
    ///
    /// Returns [`NetworkError::Identity`] when the peer presented no
    /// credential, or presented both SID1 and OIA1 — the door accepts exactly
    /// one, so the ambiguity is rejected here instead of downstream.
    async fn accept(&self) -> Result<(Self::Stream, SealedIdentityBlob), NetworkError>;
}
