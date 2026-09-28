//! Canonical SID1 forwarding for native generation transports.
//!
//! This module only reads the private identity projection. It never mints,
//! re-encodes, or logs identity credentials.

use anyhow::{Context, anyhow};

/// Return the exact canonical SID1 payload for the verified caller.
///
/// `actor`, `session`, and `genesis` are compared against the decoded sealed
/// claims and the complete projection validation performed by
/// `mcp_sealed_id_header`. A mismatch fails closed before any native request.
pub(crate) fn credential_for(actor: &str, session: &str, genesis: &str) -> anyhow::Result<String> {
    let actor = actor.trim();
    let session = session.trim();
    let genesis = genesis.trim();
    anyhow::ensure!(!actor.is_empty(), "SID1 actor is empty");
    anyhow::ensure!(!session.is_empty(), "SID1 session is empty");
    anyhow::ensure!(!genesis.is_empty(), "SID1 genesis is empty");

    let projection =
        op_identity::session_projection::resolve_identity_credential_session(Some(session))
            .context("canonical identity credential projection is unavailable")?;
    let encoded = projection
        .mcp_sealed_id_header()
        .context("canonical MCP SID1 projection is unavailable")?;
    let claims = op_identity::sealed_id::SealedId::from_inline_ref(&format!(
        "{}{}",
        op_identity::sealed_id::INLINE_PREFIX,
        encoded
    ))
    .map_err(|error| anyhow!("canonical SID1 claims are invalid: {error}"))?;
    anyhow::ensure!(
        claims.principal_id == actor,
        "SID1 actor does not match caller"
    );
    anyhow::ensure!(
        claims.session_id == session,
        "SID1 session does not match caller"
    );
    anyhow::ensure!(
        claims.session_genesis == genesis,
        "SID1 genesis does not match caller"
    );
    Ok(encoded.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_identity_inputs_fail_before_projection_read() {
        assert!(credential_for("", "session", "genesis").is_err());
        assert!(credential_for("actor", "", "genesis").is_err());
        assert!(credential_for("actor", "session", "").is_err());
    }
}
