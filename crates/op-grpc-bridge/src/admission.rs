//! Exact method grant checks for already verified SID1 identities.
//! The caller must check compiled handler compatibility and revalidate before
//! dispatch; this gate does not execute the method or validate arguments.

use crate::identity_receiver::VerifiedSid1;
use op_state_store::{MethodDecl, PluginSchema};
use std::path::{Path, PathBuf};

/// Read the materializer's protected authority document, never a caller path.
/// The held descriptor prevents a pathname swap after the ownership check.
pub(crate) fn read_grant_document() -> anyhow::Result<serde_json::Value> {
    use std::io::Read as _;
    use std::os::unix::{fs::{MetadataExt as _, OpenOptionsExt as _}, fs::PermissionsExt as _};
    let path = std::env::var_os("OP_GRANTS_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/dev/shm/opdbus/capability-grants.json".into());
    let file = std::fs::OpenOptions::new().read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK).open(path)?;
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file() && metadata.uid() == 0 && metadata.permissions().mode() & 0o022 == 0,
        "grant projection is not a protected root-owned file"
    );
    const MAX_BYTES: u64 = 8 * 1024 * 1024;
    anyhow::ensure!(metadata.len() <= MAX_BYTES, "grant projection exceeds its byte bound");
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() as u64 <= MAX_BYTES, "grant projection grew beyond its byte bound");
    crate::grant_document::validate_grant_document(&bytes).map_err(anyhow::Error::msg)?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// Require the sealed method's exact capability and the principal's exact grant.
/// The grant document must come from the protected grant projection, never a caller.
pub(crate) fn require_method_grant(
    identity: &VerifiedSid1,
    catalog_dir: &Path,
    plugin_id: &str,
    canonical_method: &str,
    requested_capability: Option<&str>,
    grants: &serde_json::Value,
) -> anyhow::Result<(MethodDecl, String)> {
    let pinned = crate::catalog::Catalog::sealed_plugin(catalog_dir, plugin_id)?;
    let schema: PluginSchema = serde_json::from_value(
        pinned
            .schema
            .ok_or_else(|| anyhow::anyhow!("sealed plugin schema is unavailable"))?,
    )?;
    let declaration = schema
        .methods
        .get(canonical_method)
        .ok_or_else(|| anyhow::anyhow!("sealed method is unavailable"))?;
    let required = declaration
        .required_capability
        .as_deref()
        .map(str::trim)
        .filter(|capability| !capability.is_empty())
        .ok_or_else(|| anyhow::anyhow!("sealed method capability is unavailable"))?;
    let claims = identity.claims();
    anyhow::ensure!(
        !claims.principal_id.is_empty() && !claims.session_id.is_empty(),
        "verified identity is incomplete"
    );
    anyhow::ensure!(
        claims.expires_at == 0 || claims.expires_at > chrono::Utc::now().timestamp(),
        "verified identity has expired"
    );
    anyhow::ensure!(
        requested_capability == Some(required),
        "requested capability does not match the sealed method"
    );
    anyhow::ensure!(
        crate::grant_document::capabilities_for_principal(grants, &claims.principal_id)
            .contains(required),
        "principal lacks the required capability"
    );
    Ok((declaration.clone(), pinned.schema_hash))
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(crate) fn check_sid1_method_in(
    catalog_dir: &Path,
    state_dir: &str,
    grants_path: &Path,
    wire: &[u8],
    plugin_id: &str,
    canonical_method: &str,
    requested_capability: Option<&str>,
    now: i64,
    record: Option<op_cozo_store::HumanPrincipalRecord>,
) -> anyhow::Result<(VerifiedSid1, MethodDecl, String)> {
    let bound = crate::identity_receiver::verify_in(catalog_dir, state_dir, wire, "grpc", now)?;
    let identity = crate::identity_receiver::verify_registered_for_test(bound, record)?;
    let grants: serde_json::Value = serde_json::from_slice(&std::fs::read(grants_path)?)?;
    let (method, hash) = require_method_grant(
        &identity,
        catalog_dir,
        plugin_id,
        canonical_method,
        requested_capability,
        &grants,
    )?;
    Ok((identity, method, hash))
}

/// Verify SID1 and check the protected grant projection against the pinned method.
/// The caller must compare the returned method against its compiled handler,
/// validate arguments, and revalidate authorization at the backend effect boundary.
pub async fn check_sid1_method_grant(
    wire: &[u8],
    plugin_id: &str,
    canonical_method: &str,
    requested_capability: Option<&str>,
) -> anyhow::Result<(VerifiedSid1, MethodDecl, String)> {
    let identity = crate::identity_receiver::verify_sid1(wire, "grpc").await?;
    let catalog_dir = std::env::var_os("OP_BLOB_CATALOG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(op_blob::catalog::DEFAULT_SHM_DIR));
    let plugin_id = plugin_id.to_owned();
    let canonical_method = canonical_method.to_owned();
    let requested_capability = requested_capability.map(str::to_owned);
    tokio::task::spawn_blocking(move || {
        let grants = read_grant_document()?;
        let (method, hash) = require_method_grant(
            &identity,
            &catalog_dir,
            &plugin_id,
            &canonical_method,
            requested_capability.as_deref(),
            &grants,
        )?;
        Ok((identity, method, hash))
    })
    .await?
}
