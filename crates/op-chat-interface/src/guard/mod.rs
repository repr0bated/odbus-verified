//! Secret_Guard (spec task 3): fails closed on credential material in
//! every outbound channel and at request intake. Property-test tasks
//! 3.2/3.4/3.6/3.8 are deferred — the owner directed no testing or
//! verification until after deployment.

pub mod credential_registry;
pub mod intake;
pub mod records;
pub mod secret_guard;
pub mod stream_guard;

pub use credential_registry::CredentialRegistry;
pub use records::{RecordWriter, RedactingLogger};
pub use secret_guard::{GuardErrorKind, GuardVerdict, OutputChannel, SecretGuard};
pub use stream_guard::StreamGuard;
