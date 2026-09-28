//! Admit a compiled handler only when its contract matches the present seal.
//!
//! The generated artifact records the binary's version and complete method
//! declarations and state fields. It grants no authority and establishes no plugin existence:
//! the trusted sealer-owned, manifest-pinned catalog is always read first.
//! No consumer re-hashing, schema regeneration, or registry lookup occurs.
//!
//! This is separate from identity, caller grants, and argument validation.
//! Existing admission owns those checks. It must pass the exact capability
//! it exercised; this helper compares it to the same pinned declaration used
//! for compatibility. Semantic changes still require a schema version bump.

use std::collections::BTreeMap;
#[cfg(test)]
use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Context as _, Result};
use op_state_store::MethodDecl;
use serde::Deserialize;

use crate::sealed_schema_reader::{
    ManifestPinnedSchemaDocument, blob_catalog_dir_from_env, read_manifest_pinned_schema_document,
};

#[cfg(test)]
#[path = "method_contract_admission_tests.rs"]
mod tests;

#[derive(Debug, Deserialize)]
struct CompiledPluginContract {
    version: String,
    fields: std::collections::HashMap<String, op_state_store::FieldSchema>,
    methods: BTreeMap<String, MethodDecl>,
}

type CompiledContracts = BTreeMap<String, CompiledPluginContract>;

fn compiled_contracts() -> Result<&'static CompiledContracts> {
    static CONTRACTS: OnceLock<Result<CompiledContracts, String>> = OnceLock::new();
    CONTRACTS
        .get_or_init(|| {
            serde_json::from_str(include_str!(concat!(
                env!("OUT_DIR"),
                "/compiled_method_contracts.json"
            )))
            .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| anyhow::anyhow!("invalid compiled method compatibility evidence: {error}"))
}

/// Admit a typed state reader or producer against an already pinned seal.
///
/// This compares build-time compatibility evidence, never runtime registry
/// membership. Callers must pin the authoritative catalog first, and producers
/// must call this before any durable callback or pending-publication marker.
/// The complete nested field declarations and plugin version must agree. The
/// public disclosure policy remains the exact sealed policy; it is not inferred
/// from compiled fields. Method admission and caller authorization are separate.
/// This does not establish an atomic catalog/state transaction.
pub(crate) fn admit_compiled_state_contract(
    pinned: &ManifestPinnedSchemaDocument,
    plugin_id: &str,
) -> Result<()> {
    anyhow::ensure!(
        pinned.schema.name == plugin_id && pinned.blob.manifest.plugin_id == plugin_id,
        "pinned state contract is not for expected plugin {plugin_id}"
    );
    let compiled = compiled_contracts()?
        .get(plugin_id)
        .with_context(|| format!("no compiled state compatibility evidence for {plugin_id}"))?;
    anyhow::ensure!(
        pinned.schema.version == compiled.version,
        "compiled/sealed state version mismatch for {plugin_id}: compiled {}, sealed {}; align the binary and sealed catalog before accessing typed state",
        compiled.version,
        pinned.schema.version
    );
    anyhow::ensure!(
        pinned.schema.fields == compiled.fields,
        "compiled/sealed state field contract mismatch for {plugin_id}; align the binary and sealed catalog before accessing typed state"
    );
    Ok(())
}

/// Re-read the pinned catalog and reject contract drift before backend effects.
///
/// Use the canonical schema method key, not a transport alias. The returned
/// declaration comes from the exact pinned snapshot checked here; callers can
/// use its arguments for separately normalized input validation. This function
/// does not admit identity, grant capability, or claim a cross-call catalog lock.
pub(crate) fn admit_compiled_method(
    plugin_id: &str,
    canonical_method: &str,
    exercised_capability: Option<&str>,
) -> Result<MethodDecl> {
    Ok(admit_compiled_invocation(plugin_id, canonical_method, exercised_capability)?.0)
}

/// Retain the complete admitted plugin for methods which consume its catalog
/// metadata. A second catalog read could observe a different seal.
pub(crate) fn admit_compiled_invocation(
    plugin_id: &str,
    canonical_method: &str,
    exercised_capability: Option<&str>,
) -> Result<(MethodDecl, op_state_store::PluginSchema)> {
    // Even absent/broken evidence must never act as a fallback existence index.
    let pinned = read_manifest_pinned_schema_document(&blob_catalog_dir_from_env(), plugin_id)?;
    let declaration = admit_pinned_method(
        &pinned,
        plugin_id,
        canonical_method,
        exercised_capability,
        compiled_contracts()?,
    )?;
    Ok((declaration, pinned.schema))
}

/// Validate against the same declaration admitted immediately before dispatch.
/// Outer transports may have admitted an earlier seal. Do not echo rejected
/// argument values: inputs can contain provider credentials or private data.
pub(crate) fn validate_admitted_arguments(
    declaration: &MethodDecl,
    arguments: &serde_json::Value,
) -> Result<()> {
    let contract = serde_json::to_value(&declaration.args)?;
    let validator =
        jsonschema::Validator::new(&contract).context("invalid admitted method input contract")?;
    validator.validate(arguments).map_err(|error| {
        anyhow::anyhow!(
            "arguments do not conform to admitted method at {}",
            error.instance_path
        )
    })
}

/// Check the result without including private result values in an error.
pub(crate) fn validate_admitted_output(
    declaration: &MethodDecl,
    output: &serde_json::Value,
) -> Result<()> {
    let returns = declaration
        .returns
        .as_ref()
        .context("admitted method has no output contract")?;
    let contract = serde_json::to_value(returns)?;
    let validator =
        jsonschema::Validator::new(&contract).context("invalid admitted method output contract")?;
    validator.validate(output).map_err(|error| {
        anyhow::anyhow!(
            "result does not conform to admitted method at {}",
            error.instance_path
        )
    })
}

fn admit_pinned_method(
    pinned: &ManifestPinnedSchemaDocument,
    plugin_id: &str,
    canonical_method: &str,
    exercised_capability: Option<&str>,
    compiled: &CompiledContracts,
) -> Result<MethodDecl> {
    let declaration = pinned
        .schema
        .methods
        .get(canonical_method)
        .with_context(|| {
            format!("method {plugin_id}.{canonical_method} is absent from pinned seal")
        })?;
    let compiled_plugin = compiled.get(plugin_id).with_context(|| {
        format!("no compiled method compatibility evidence for sealed plugin {plugin_id}")
    })?;
    anyhow::ensure!(
        pinned.schema.version == compiled_plugin.version,
        "compiled/sealed plugin version mismatch for {plugin_id}: compiled {}, sealed {}; align the binary and sealed catalog before dispatch",
        compiled_plugin.version,
        pinned.schema.version
    );
    let compiled_method = compiled_plugin
        .methods
        .get(canonical_method)
        .with_context(|| {
            format!("no compiled method compatibility evidence for {plugin_id}.{canonical_method}")
        })?;
    anyhow::ensure!(
        declaration == compiled_method,
        "compiled/sealed method contract mismatch for {plugin_id}.{canonical_method}; align the binary and sealed catalog before dispatch"
    );
    anyhow::ensure!(
        declaration.required_capability.as_deref() == exercised_capability,
        "exercised capability differs from pinned method {plugin_id}.{canonical_method}"
    );
    Ok(declaration.clone())
}

/// Admit a method from a catalog snapshot already pinned by a typed private
/// consumer. This avoids a second environment-selected catalog read while
/// retaining the same generated compatibility evidence and exact capability.
pub(crate) fn admit_pinned_compiled_method(
    pinned: &ManifestPinnedSchemaDocument,
    plugin_id: &str,
    canonical_method: &str,
    exercised_capability: Option<&str>,
) -> Result<MethodDecl> {
    admit_pinned_method(
        pinned,
        plugin_id,
        canonical_method,
        exercised_capability,
        compiled_contracts()?,
    )
}

#[cfg(test)]
fn admit_fixture_method(
    catalog_dir: &Path,
    plugin_id: &str,
    canonical_method: &str,
    exercised_capability: Option<&str>,
    compiled: &CompiledContracts,
) -> Result<MethodDecl> {
    let pinned = read_manifest_pinned_schema_document(catalog_dir, plugin_id)?;
    admit_pinned_method(
        &pinned,
        plugin_id,
        canonical_method,
        exercised_capability,
        compiled,
    )
}
