//! Handoff to the door over its local socket.
//!
//! Everything the transports terminate converges here and leaves over one
//! Unix socket. The door's interceptor is the only component that decides
//! whether a credential is good; this module delivers bytes and credential
//! unchanged, and fails closed when it cannot.
//!
//! The socket, not an address, is the boundary. An identity container never
//! binds an IP inside its own namespace: it receives the door socket as a
//! mount and reaches the control plane through it, so the kernel authenticates
//! the peer process and no fabric address has to exist for a local hop.

pub mod proxy;

pub use proxy::{DoorProxy, DoorTarget};

/// The door socket, at the path a container sees after it is mounted in.
///
/// Same path the bridge binds and the in-container 3tchedFS client consumes,
/// so host and guest name one socket rather than two conventions.
pub const DEFAULT_LUNGS_SOCKET: &str = "/run/opdbus/lungs/lungs.sock";

/// Bounded queue depth between a transport and the door.
///
/// When this fills, ingress sheds rather than buffering: a stalled door must
/// not translate into unbounded memory growth in this process.
pub const HANDOFF_QUEUE_DEPTH: usize = 1024;
