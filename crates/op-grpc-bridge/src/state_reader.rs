//! Read committed public state through the pinned plugin policy.
use std::path::Path;

fn redact_identity_credentials(value: &mut simd_json::OwnedValue) {
    match value {
        simd_json::OwnedValue::Object(object) => {
            object.remove("sealed_id");
            for nested in object.values_mut() {
                redact_identity_credentials(nested);
            }
        }
        simd_json::OwnedValue::Array(values) => {
            for nested in values {
                redact_identity_credentials(nested);
            }
        }
        _ => {}
    }
}

pub(crate) fn public_plugin_value(
    plugin_id: &str,
    value: &simd_json::OwnedValue,
) -> simd_json::OwnedValue {
    let mut public = value.clone();
    if plugin_id == "identity_sled" {
        redact_identity_credentials(&mut public);
    }
    public
}

/// Public state read from the committed projection, not from an event payload.
pub(crate) struct PublicStateSnapshot {
    pub value: simd_json::OwnedValue,
    pub generation: u64,
    pub schema_hash: String,
}

/// Read a public projection using one manifest-pinned plugin policy.
/// Caller authorization is separate and must precede outward disclosure.
pub(crate) fn read_public_state_in(
    catalog_dir: &Path,
    state_dir: &str,
    plugin_id: &str,
) -> anyhow::Result<Option<PublicStateSnapshot>> {
    let before = crate::catalog::Catalog::sealed_plugin(catalog_dir, plugin_id)?;
    let schema: op_state_store::PluginSchema = serde_json::from_value(
        before
            .schema
            .clone()
            .ok_or_else(|| anyhow::anyhow!("sealed plugin schema unavailable"))?,
    )?;
    let snapshot =
        op_core::projection_shm::read_projection_snapshot_in(state_dir, &[plugin_id.to_owned()])?;
    let after = crate::catalog::Catalog::sealed_plugin(catalog_dir, plugin_id)?;
    anyhow::ensure!(
        before.schema_hash == after.schema_hash
            && before.sealed_schema_json == after.sealed_schema_json,
        "sealed plugin changed during state read"
    );
    let Some(mut bytes) = snapshot.states.get(plugin_id).cloned() else {
        return Ok(None);
    };
    let private = simd_json::to_owned_value(&mut bytes)?;
    let Some(projected) = schema.project_public_state(&private)? else {
        return Ok(None);
    };
    let public = public_plugin_value(plugin_id, &projected);
    anyhow::ensure!(
        matches!(&public, simd_json::OwnedValue::Object(_)),
        "committed plugin state is not an object"
    );
    Ok(Some(PublicStateSnapshot {
        value: public,
        generation: snapshot.generation,
        schema_hash: before.schema_hash,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_committed_state_through_sealed_public_policy() {
        let root = tempfile::tempdir().unwrap();
        let catalog_dir = root.path().join("catalog");
        let state_dir = root.path().join("state");
        let state_path = state_dir.to_str().unwrap();
        let schema = op_state_store::PluginSchema::builder("fixture")
            .string_field("status", false, "")
            .string_field("secret", false, "")
            .public_state_paths(&["/status"])
            .build();
        let mut catalog = op_blob::BlobStore::open(&catalog_dir).unwrap();
        let blob = op_blob::blobify_plugin_schema("fixture", schema);
        catalog.write(&blob).unwrap();
        op_core::projection_shm::update_projection_in(state_path, "fixture", |_| {
            Ok(Some(br#"{"status":"ready","secret":"private"}"#.to_vec()))
        })
        .unwrap();
        let snapshot = read_public_state_in(&catalog_dir, state_path, "fixture")
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::to_value(snapshot.value).unwrap(),
            serde_json::json!({"status":"ready"})
        );
        assert!(snapshot.generation > 0);
        assert_eq!(snapshot.schema_hash, blob.manifest.schema_hash);

        let untouched = op_state_store::PluginSchema::builder("untouched")
            .string_field("status", false, "")
            .public_state_paths(&["/status"])
            .build();
        catalog
            .write(&op_blob::blobify_plugin_schema("untouched", untouched))
            .unwrap();
        assert!(
            read_public_state_in(&catalog_dir, state_path, "untouched")
                .unwrap()
                .is_none()
        );

        let closed = op_state_store::PluginSchema::builder("closed")
            .string_field("status", false, "")
            .build();
        catalog
            .write(&op_blob::blobify_plugin_schema("closed", closed))
            .unwrap();
        op_core::projection_shm::update_projection_in(state_path, "closed", |_| {
            Ok(Some(br#"{"status":"private"}"#.to_vec()))
        })
        .unwrap();
        assert!(
            read_public_state_in(&catalog_dir, state_path, "closed")
                .unwrap()
                .is_none()
        );
        assert!(read_public_state_in(&catalog_dir, state_path, "missing").is_err());
    }

    #[test]
    fn identity_credentials_are_removed_recursively() {
        let state = simd_json::json!({"sleds": [{"sealed_id":"secret", "nested":{"sealed_id":"also-secret"}}]});
        let public = public_plugin_value("identity_sled", &state);
        assert_eq!(
            serde_json::to_value(public).unwrap(),
            serde_json::json!({"sleds":[{"nested":{}}]})
        );
        assert_eq!(public_plugin_value("other", &state), state);
    }
}
