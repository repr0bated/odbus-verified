//! A.N.N.A. Scribe: the arrival gate that binds a key to its credential.
//!
//! The decoy proves a WireGuard peer and the sealed credential proves its own
//! integrity, but neither statement says the two describe the same caller.
//! Admission accepted that gap: the receiver hashed whichever credential
//! arrived and wrote that hash in as the expected value, so the later
//! comparison in `revalidate_bound_oracle` could only match itself. The one
//! check that did compare the keys was a `debug_assert_eq!`, absent from
//! release builds.
//!
//! This module performs the checks that make the binding load-bearing, at the
//! point a session is admitted. It compares values the validator already
//! produced; it never mints, re-seals or rewrites a credential, and it holds
//! no credential bytes.
//!
//! Scope: this gate answers "do these statements describe one caller". Seal
//! integrity and signature verification stay with `op-identity`; the
//! authorization decision stays with the interceptor.

/// Service persona recorded against an AnnaScribe verdict.
///
/// Taken from the plugin so the audit actor and the sealed schema stay one
/// value; a second spelling here would detach the verdict from the identity
/// that issued it.
pub(crate) use op_plugins::state_plugins::wg_opdbus::ANNA_SCRIBE_PRINCIPAL_ID;

/// Why an arrival was refused.
///
/// Each variant is a disagreement between two statements that must match, so
/// the message names the pair rather than reporting a generic failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ScribeRejection {
    #[error("assertion peer key does not match the admitted session key")]
    PeerKeyMismatch,
    #[error("credential names a different principal than the assertion binding")]
    PrincipalMismatch,
    #[error("credential names a different session than the assertion binding")]
    SessionMismatch,
    #[error("credential carries no sealed identity to commit to")]
    MissingCredential,
    #[error("credential digest does not match the admitted commitment")]
    DigestMismatch,
}

/// The statements AnnaScribe compares at arrival.
///
/// Borrowed rather than owned: every field already lives in the proof, the
/// consumed admission or the validated claims, and copying them would create
/// a second place where an identity can drift.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ArrivalFacts<'a> {
    /// Peer key the decoy signed in its assertion binding.
    pub assertion_peer_key: &'a str,
    /// Principal the decoy's binding names.
    pub assertion_principal_id: &'a str,
    /// Session the decoy's binding names.
    pub assertion_session_id: &'a str,
    /// Peer key the receiver consumed for this pending admission.
    pub consumed_peer_key: &'a str,
    /// WireGuard key carried inside the verified credential.
    pub credential_peer_key: &'a str,
    /// Principal carried inside the verified credential.
    pub credential_principal_id: &'a str,
    /// Session carried inside the verified credential.
    pub credential_session_id: &'a str,
    /// Digest of the verified credential, as derived during validation.
    ///
    /// Taken rather than recomputed so the gate commits to the same value the
    /// validator produced; hashing a second copy here would let the two
    /// disagree without either being wrong on its own.
    pub credential_digest: &'a str,
}

/// An admitted arrival, carrying the digest the session commits to.
///
/// Only [`verify_arrival`] constructs one, so holding a verdict is evidence
/// the checks ran. The digest is returned rather than stored elsewhere to
/// keep this module free of session state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScribeVerdict {
    credential_digest: String,
}

impl ScribeVerdict {
    /// Digest of the credential this session is bound to.
    pub(crate) fn credential_digest(&self) -> &str {
        &self.credential_digest
    }

    /// Audit record naming the persona that produced the verdict.
    pub(crate) fn audit_value(&self) -> serde_json::Value {
        serde_json::json!({
            "kind": "anna-scribe",
            "verifier_principal_id": ANNA_SCRIBE_PRINCIPAL_ID,
            "credential_digest": self.credential_digest,
        })
    }
}

/// Gate one arrival: every statement must name the same caller.
///
/// Three keys have to agree — the key the decoy signed, the key the receiver
/// consumed, and the key sealed inside the credential. Any two agreeing is
/// not enough: the pair without the third is what lets a credential for one
/// peer ride an assertion for another.
///
/// Returns the digest this session commits to, so the caller binds the proof
/// to a value the gate accepted rather than one the request supplied.
pub(crate) fn verify_arrival(facts: ArrivalFacts<'_>) -> Result<ScribeVerdict, ScribeRejection> {
    if facts.assertion_peer_key != facts.consumed_peer_key
        || facts.assertion_peer_key != facts.credential_peer_key
    {
        return Err(ScribeRejection::PeerKeyMismatch);
    }
    if facts.assertion_principal_id != facts.credential_principal_id {
        return Err(ScribeRejection::PrincipalMismatch);
    }
    if facts.assertion_session_id != facts.credential_session_id {
        return Err(ScribeRejection::SessionMismatch);
    }
    if facts.credential_digest.is_empty() {
        return Err(ScribeRejection::MissingCredential);
    }
    Ok(ScribeVerdict {
        credential_digest: facts.credential_digest.to_string(),
    })
}

/// Re-check a verdict against a credential seen later in the session.
///
/// Separate from [`verify_arrival`] because revalidation compares against the
/// digest admitted earlier, while arrival establishes it.
pub(crate) fn verify_continuity(
    verdict: &ScribeVerdict,
    credential_digest: &str,
) -> Result<(), ScribeRejection> {
    if credential_digest.is_empty() {
        return Err(ScribeRejection::MissingCredential);
    }
    if credential_digest != verdict.credential_digest {
        return Err(ScribeRejection::DigestMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "0I5FNCuHAu62VctaNgOlJ7RgfejJpYe9Qd0VdPGesX8=";
    const OTHER_KEY: &str = "Env5zPMmM1Zhypu5/DY/yU4PRYW/4AOEq2KZ4OSc+18=";
    const PRINCIPAL: &str = "c9bdf32e-811d-2495-b688-277c9e5c4d36";
    const SESSION: &str = "414fbc56-03fb-20ab-758a-bcc7a7cbdc0e";
    const DIGEST: &str = "9f2c4a1b8d3e5f607182930a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e";

    fn facts() -> ArrivalFacts<'static> {
        ArrivalFacts {
            assertion_peer_key: KEY,
            assertion_principal_id: PRINCIPAL,
            assertion_session_id: SESSION,
            consumed_peer_key: KEY,
            credential_peer_key: KEY,
            credential_principal_id: PRINCIPAL,
            credential_session_id: SESSION,
            credential_digest: DIGEST,
        }
    }

    #[test]
    fn an_agreeing_arrival_commits_to_the_credential_digest() {
        let verdict = verify_arrival(facts()).expect("agreeing arrival admitted");
        assert_eq!(verdict.credential_digest(), DIGEST);
        assert_eq!(
            verdict.audit_value()["verifier_principal_id"],
            serde_json::json!(ANNA_SCRIBE_PRINCIPAL_ID)
        );
    }

    #[test]
    fn a_credential_for_another_peer_cannot_ride_this_assertion() {
        // The decoy and the receiver agree; only the sealed credential names a
        // different key. This is the case the release build stopped catching
        // when the equality check was left as a `debug_assert_eq!`.
        let mut facts = facts();
        facts.credential_peer_key = OTHER_KEY;
        assert_eq!(
            verify_arrival(facts).expect_err("mismatched credential key refused"),
            ScribeRejection::PeerKeyMismatch
        );
    }

    #[test]
    fn a_substituted_consumed_key_is_refused() {
        let mut facts = facts();
        facts.consumed_peer_key = OTHER_KEY;
        assert_eq!(
            verify_arrival(facts).expect_err("mismatched consumed key refused"),
            ScribeRejection::PeerKeyMismatch
        );
    }

    #[test]
    fn principal_and_session_must_agree_with_the_binding() {
        let mut swapped_principal = facts();
        swapped_principal.credential_principal_id = "did:op:service:other";
        assert_eq!(
            verify_arrival(swapped_principal).expect_err("principal drift refused"),
            ScribeRejection::PrincipalMismatch
        );

        let mut swapped_session = facts();
        swapped_session.credential_session_id = "00000000-0000-0000-0000-000000000000";
        assert_eq!(
            verify_arrival(swapped_session).expect_err("session drift refused"),
            ScribeRejection::SessionMismatch
        );
    }

    #[test]
    fn an_arrival_without_a_credential_is_refused() {
        let mut facts = facts();
        facts.credential_digest = "";
        assert_eq!(
            verify_arrival(facts).expect_err("absent credential refused"),
            ScribeRejection::MissingCredential
        );
    }

    #[test]
    fn continuity_rejects_a_credential_swapped_after_admission() {
        let verdict = verify_arrival(facts()).expect("arrival admitted");
        verify_continuity(&verdict, DIGEST).expect("same credential stays bound");
        assert_eq!(
            verify_continuity(&verdict, "0000000000000000000000000000000000000000000000000000000000000000")
                .expect_err("swapped credential refused"),
            ScribeRejection::DigestMismatch
        );
        assert_eq!(
            verify_continuity(&verdict, "").expect_err("dropped credential refused"),
            ScribeRejection::MissingCredential
        );
    }
}
