//! Cockpit-specific evidence and integration for the preserved control-plane
//! identity. No listener, transport fallback, grant writer or lifecycle repair.
//! Spec: `.kiro/specs/cockpit-container/`; implementation begins with task 1.1.
pub mod records;
pub mod discovery;
pub mod safety;
pub mod secret_guard;
pub mod writer;
