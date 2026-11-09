//! Confirmation_Gate (spec task 8.1). Property tests 8.2/8.3 deferred
//! (owner: no testing until after deployment).
pub mod confirmation_gate;
pub use confirmation_gate::{CONFIRM_TTL_SECS, ConfirmationGate, FailedCheck, digest};
