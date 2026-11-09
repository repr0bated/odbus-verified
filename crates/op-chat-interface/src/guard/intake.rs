//! Request-intake checking (spec task 3.7): every field of turn,
//! decision and re-verify requests is checked before it reaches the
//! turn loop, the context builder, the executor or the logger. A match
//! rejects the request with no state change and one security event.

use super::secret_guard::{GuardVerdict, SecretGuard};
use crate::{
    errors::{ErrorKind, TypedError},
    types::{CredentialClass, SecurityEvent},
};

pub struct IntakeRefusal {
    pub error: TypedError,
    pub credential_class: Option<CredentialClass>,
}

pub fn check_request(
    guard: &SecretGuard,
    request_name: &str,
    fields: &[&str],
    at: &str,
) -> Result<(), (IntakeRefusal, SecurityEvent)> {
    for field in fields {
        match guard.check_item(field.as_bytes()) {
            GuardVerdict::Clean => {}
            GuardVerdict::Blocked { cls } => {
                let event = SecurityEvent {
                    at: at.to_string(),
                    kind: "credential-refused".to_string(),
                    channel: "request-intake".to_string(),
                    credential_class: Some(cls),
                };
                let refusal = IntakeRefusal {
                    error: TypedError::new(ErrorKind::Denial, request_name)
                        .with_detail(SecretGuard::redaction_marker(cls)),
                    credential_class: Some(cls),
                };
                return Err((refusal, event));
            }
            GuardVerdict::Error { .. } => {
                let event = SecurityEvent {
                    at: at.to_string(),
                    kind: "credential-check-error".to_string(),
                    channel: "request-intake".to_string(),
                    credential_class: None,
                };
                let refusal = IntakeRefusal {
                    error: TypedError::new(ErrorKind::Denial, request_name)
                        .with_detail("[redacted:guard-error]".to_string()),
                    credential_class: None,
                };
                return Err((refusal, event));
            }
        }
    }
    Ok(())
}
