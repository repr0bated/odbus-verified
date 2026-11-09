//! Profile_Loader and Session_Mode resolution (spec task 4).
//! Property-test tasks 4.2/4.3/4.4 are deferred — the owner directed
//! no testing or verification until after deployment.

pub mod profile_loader;

pub use profile_loader::{MAX_PROFILE_BYTES, ProfileError, SessionMode, format, parse};
