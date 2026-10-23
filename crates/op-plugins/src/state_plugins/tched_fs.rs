//! 3tchedFS filesystem registration, typed contract, and shared invocation convention.
//!
//! This describes the existing FUSE implementation, not a second method router.
//! Target declarations and admission remain owned by each sealed plugin. Mount
//! identity and live contents remain in the identity's own filesystem namespace.

use anyhow::{Result, bail, ensure};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
use super::tched_fs_contract;

pub const PLUGIN_NAME: &str = "tched_fs";
const VERSION: &str = "2.0.8";
const READ_PROTOCOL: &str = "cap.storage.3tchedfs.protocol.read@v1";
const READ_CONTRACT: &str = "cap.storage.3tchedfs.contract.read@v1";
const READ_RECEIPT: &str = "cap.storage.3tchedfs.receipt.read@v1";

/// Public discovery metadata; no mount identity, credentials, or live values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContractDescriptor {
    pub schema_version: String,
    pub source_commit: String,
    pub method: String,
}

/// A complete, immutable contract authored by this plugin. Its sealed output
/// schema includes the exact document; callers cannot construct arbitrary JSON
/// as a contract. Dynamic payload definitions remain inside the typed document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct FilesystemContract(serde_json::Value);

impl FilesystemContract {
    pub fn get() -> Self {
        Self(tched_fs_contract::document())
    }

    pub fn document(&self) -> &serde_json::Value {
        &self.0
    }
}

impl schemars::JsonSchema for FilesystemContract {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "FilesystemContract".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // Seal the contract itself as well as its shape. No separate schema
        // directory, runtime file loader, or reusable parsed-document cache.
        // Schema documents have dynamic keys such as `$defs`. An open object
        // transports those keys as protobuf Struct; the exact constraints still
        // rejects every document except this plugin's immutable contract.
        // jsonschema 0.29's object-const equality depends on insertion order
        // with preserve_order. Derive key constraints so transport ordering is
        // irrelevant without weakening the exact output declaration.
        serde_json::json!({
            "type": "object",
            "additionalProperties": true,
            "allOf": [literal_schema(&tched_fs_contract::document())],
        })
        .try_into()
        .expect("filesystem contract schema is an object")
    }
}

/// Derive an order-independent exact JSON value schema from the sole authority.
fn literal_schema(value: &serde_json::Value) -> serde_json::Value {
    use serde_json::{Value, json};
    match value {
        Value::Object(fields) => {
            let properties: serde_json::Map<String, Value> = fields
                .iter()
                .map(|(key, value)| (key.clone(), literal_schema(value)))
                .collect();
            json!({
                "type": "object", "properties": properties,
                "required": fields.keys().collect::<Vec<_>>(),
                "additionalProperties": false,
            })
        }
        Value::Array(items) => {
            let mut schema = json!({
                "type": "array", "minItems": items.len(),
                "maxItems": items.len(), "items": false,
            });
            if !items.is_empty() {
                schema["prefixItems"] = items.iter().map(literal_schema).collect();
            }
            schema
        }
        _ => json!({"const": value}),
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetContractOutput {
    pub contract: FilesystemContract,
}

/// One convention for every declared operation file, relative to the mount.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Invocation {
    /// Percent-encoded path components; resolve against the pinned catalog.
    pub path: String,
    /// Open one descriptor without truncating, then retain it through the reply.
    pub open: String,
    pub arguments: String,
    pub commit: String,
    pub result: String,
    /// Target-owned declaration, including input/output schemas and capability.
    pub declaration: String,
    /// A transport error can leave execution uncertain; never replay implicitly.
    pub automatic_retry: bool,
}

/// Static filesystem contract. This never asserts that a mount is ready.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-oscal-subid" = "sch.storage.3tchedfs.schema@v1"))]
#[schemars(extend("x-oscal-category" = "storage"))]
pub struct FilesystemProtocol {
    pub filesystem_type: String,
    /// Mount-local metadata holds the actual identity and pinned catalog.
    pub mount_metadata: String,
    pub invocation: Invocation,
    pub contract: ContractDescriptor,
}

impl Default for FilesystemProtocol {
    fn default() -> Self {
        Self {
            filesystem_type: "fuse.3tchedFS".into(),
            mount_metadata: ".3tchedfs.json".into(),
            invocation: Invocation {
                path: "plugins/{plugin}/operations/{method}/call".into(),
                open: "O_RDWR".into(),
                arguments: "application/json".into(),
                commit: "fsync".into(),
                result: "seek(0), read receipt or diagnostic on the same descriptor".into(),
                declaration: "plugins/{plugin}/operations/{method}/metadata.json".into(),
                automatic_retry: false,
            },
            contract: ContractDescriptor {
                schema_version: tched_fs_contract::schema_version().into(),
                source_commit: tched_fs_contract::source_commit().into(),
                method: "get_contract".into(),
            },
        }
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetProtocolInput {}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetContractInput {}

#[derive(Default)]
pub struct ThreeTchedFsPlugin;

#[async_trait]
impl StatePlugin for ThreeTchedFsPlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }
    fn version(&self) -> &str {
        VERSION
    }
    fn schema(&self) -> Option<PluginSchema> {
        Some(tched_fs_schema())
    }

    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff> {
        ensure!(
            current == desired && *desired == protocol_value()?,
            "3tchedFS protocol is immutable"
        );
        Ok(StateDiff {
            plugin: PLUGIN_NAME.into(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: String::new(),
                desired_hash: String::new(),
            },
        })
    }
    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        ensure!(
            diff.plugin == PLUGIN_NAME && diff.actions.is_empty(),
            "3tchedFS protocol is immutable"
        );
        Ok(ApplyResult {
            success: true,
            changes_applied: vec![],
            errors: vec![],
            checkpoint: None,
        })
    }
    async fn verify_state(&self, desired: &Value) -> Result<bool> {
        Ok(*desired == protocol_value()?)
    }
    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        bail!("3tchedFS protocol has no mutable checkpoints")
    }
    async fn rollback(&self, _: &Checkpoint) -> Result<()> {
        bail!("3tchedFS protocol has no mutable checkpoints")
    }
    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: false,
            supports_verification: true,
            atomic_operations: true,
        }
    }
}

/// Complete canonical state, independent of which descriptor method was read.
pub fn protocol_value() -> Result<Value> {
    Ok(simd_json::serde::to_owned_value(
        FilesystemProtocol::default(),
    )?)
}

pub fn tched_fs_schema() -> PluginSchema {
    let root =
        serde_json::to_value(schemars::schema_for!(FilesystemProtocol)).expect("protocol schema");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        PLUGIN_NAME,
        VERSION,
        "Identity-bound 3tchedFS typed filesystem contract and shared invocation convention",
        &root,
    );
    schema.category = "storage".into();
    schema.display_name = Some("3tchedFS".into());
    schema.tags = vec!["immutable".into(), "filesystem".into()];
    schema.immutable_paths = vec![
        "/filesystem_type".into(),
        "/mount_metadata".into(),
        "/invocation".into(),
        "/contract".into(),
    ];
    // Only the public protocol is global. No session, file content, or mount
    // readiness is fabricated or copied into shared public state.
    schema.public_state.paths = schema.immutable_paths.clone();
    let state = protocol_value().expect("protocol serializes");
    super::schemars_adapter::apply_state_defaults(&mut schema, &state);
    schema.example = Some(state);
    schema.methods.insert(
        "get_receipt".into(),
        method_decl_from_schemars_with_output::<
            op_state_store::method_receipt::GetMethodReceiptInput,
            op_state_store::method_receipt::MethodReceiptRecovery,
        >(
            "get_receipt",
            SideEffect::Read,
            true,
            READ_RECEIPT,
            "obs.storage.3tchedfs.receipt.get@v1",
        ),
    );
    schema.capabilities.insert(
        READ_RECEIPT.into(),
        CapabilityDecl {
            id: READ_RECEIPT.into(),
            description: "Recover an operation proof only for the verified caller and session; returns no private arguments or domain result.".into(),
        },
    );
    schema.methods.insert(
        "get_contract".into(),
        method_decl_from_schemars_with_output::<GetContractInput, GetContractOutput>(
            "get_contract",
            SideEffect::Read,
            true,
            READ_CONTRACT,
            "obs.storage.3tchedfs.contract.get@v1",
        ),
    );
    schema.methods.insert(
        "get_protocol".into(),
        method_decl_from_schemars_with_output::<GetProtocolInput, FilesystemProtocol>(
            "get_protocol",
            SideEffect::Read,
            true,
            READ_PROTOCOL,
            "obs.storage.3tchedfs.protocol.get@v1",
        ),
    );
    schema.capabilities.insert(
        READ_CONTRACT.into(),
        CapabilityDecl {
            id: READ_CONTRACT.into(),
            description: "Read the immutable typed filesystem contract; grants no target operation, mount, or backing-store access.".into(),
        },
    );
    schema.capabilities.insert(
        READ_PROTOCOL.into(),
        CapabilityDecl {
            id: READ_PROTOCOL.into(),
            description:
                "Read the shared 3tchedFS invocation convention; grants no target operation access."
                    .into(),
        },
    );
    schema
}

pub fn dispatch(method: &str, arguments: &serde_json::Value) -> Result<serde_json::Value> {
    match method {
        "get_protocol" => {
            let _: GetProtocolInput = serde_json::from_value(arguments.clone())?;
            Ok(serde_json::to_value(FilesystemProtocol::default())?)
        }
        "get_contract" => {
            let _: GetContractInput = serde_json::from_value(arguments.clone())?;
            Ok(serde_json::to_value(GetContractOutput {
                contract: FilesystemContract::get(),
            })?)
        }
        _ => bail!("unknown 3tchedFS method: {method}"),
    }
}

inventory::submit! {
    crate::default_registry::PluginReg::new(PLUGIN_NAME, |_ctx| std::sync::Arc::new(ThreeTchedFsPlugin))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_filesystem_contract_preserves_protocol_and_target_authority() {
        let schema = tched_fs_schema();
        assert!(schema.validate_capability_closure().missing.is_empty());
        schema.validate_public_state_policy().unwrap();
        for subid in schema
            .subids
            .values()
            .chain(schema.methods.values().map(|m| &m.subid))
        {
            super::super::common::oscal::validate_subid(subid).unwrap();
        }
        let blob = op_blob::blobify_plugin_schema(PLUGIN_NAME, schema);
        assert_eq!(
            blob.manifest.dbus.object_path,
            crate::canonical::plugin_path(PLUGIN_NAME)
        );
        assert_eq!(
            blob.manifest.dbus.object_path,
            format!("/org/opdbus/v1/plugins/{PLUGIN_NAME}")
        );
        let bytes = blob.seal();
        // The descriptor is public source metadata, never mount/session data.
        // Keep the sealed object comfortably within the door's message budget.
        assert!(bytes.len() < 4 * 1024 * 1024, "blob: {} bytes", bytes.len());
        let pinned =
            op_blob::BlobRef::from_manifest_pinned(&bytes, &blob.manifest.schema_hash).unwrap();
        let sealed = pinned.state_store_schema().unwrap();
        let protocol = protocol_value().unwrap();
        assert!(sealed.validate(&protocol).valid);
        assert!(sealed.capability_grants.is_empty());
        assert!(sealed.field_writes.is_empty());
        let method = &sealed.methods["get_protocol"];
        let output_schema = serde_json::to_value(method.returns.as_ref().unwrap()).unwrap();
        let validator = jsonschema::validator_for(&output_schema).unwrap();
        assert!(validator.is_valid(&dispatch("get_protocol", &serde_json::json!({})).unwrap()));
        let method = &sealed.methods["get_contract"];
        assert_eq!(method.required_capability.as_deref(), Some(READ_CONTRACT));
        assert_eq!(method.side_effect, SideEffect::Read);
        assert!(method.idempotent);
        let output_schema = serde_json::to_value(method.returns.as_ref().unwrap()).unwrap();
        let contract = dispatch("get_contract", &serde_json::json!({})).unwrap();
        let validator = jsonschema::validator_for(&output_schema).unwrap();
        if let Err(error) = validator.validate(&contract) {
            panic!(
                "contract rejected at instance {} / schema {}",
                error.instance_path, error.schema_path
            );
        }
        let mut changed = contract.clone();
        changed["contract"]["methods"][0]["name"] = "invented_method".into();
        assert!(!validator.is_valid(&changed));
        assert!(!validator.is_valid(&serde_json::json!({})));
        let mut extra = contract.clone();
        extra["contract"]["invented_metadata"] = true.into();
        assert!(!validator.is_valid(&extra));
        let mut missing = contract.clone();
        missing["contract"].as_object_mut().unwrap().remove("types");
        assert!(!validator.is_valid(&missing));
        assert_eq!(
            contract["contract"],
            FilesystemContract::get().document().clone()
        );
        assert_eq!(
            sealed.project_public_state(&protocol).unwrap().unwrap(),
            protocol
        );
        // The sealer canonicalizes object keys, including nested JSON maps.
        let repeated = op_blob::blobify_plugin_schema(PLUGIN_NAME, tched_fs_schema());
        let repeated_bytes = repeated.seal();
        assert_eq!(
            bytes, repeated_bytes,
            "repeated sealing must be deterministic"
        );
        let repeated_schema =
            op_blob::BlobRef::from_manifest_pinned(&repeated_bytes, &repeated.manifest.schema_hash)
                .unwrap()
                .state_store_schema()
                .unwrap();
        assert_eq!(
            serde_json::to_value(&sealed).unwrap(),
            serde_json::to_value(&repeated_schema).unwrap()
        );
    }

    #[test]
    fn registered_contract_has_an_executable_descriptor_and_no_target_proxy() {
        assert!(
            crate::default_registry::DefaultPluginRegistry::available_plugins()
                .iter()
                .any(|id| id == PLUGIN_NAME)
        );
        let schema = tched_fs_schema();
        assert_eq!(schema.methods.len(), 3);
        let output = dispatch("get_protocol", &serde_json::json!({})).unwrap();
        assert_eq!(
            output,
            serde_json::to_value(schema.example.unwrap()).unwrap()
        );
        assert!(dispatch("invoke", &serde_json::json!({})).is_err());
        assert!(dispatch("get_protocol", &serde_json::json!({"session": "another"})).is_err());
        assert!(dispatch("get_contract", &serde_json::json!({"session": "another"})).is_err());
        assert!(dispatch("get_contract", &serde_json::json!(null)).is_err());
        // Recovery requires canonical verified actor/session context, which
        // this immutable descriptor dispatcher deliberately does not possess.
        assert!(dispatch("get_receipt", &serde_json::json!({})).is_err());
        let recovery = &schema.methods["get_receipt"];
        assert_eq!(recovery.required_capability.as_deref(), Some(READ_RECEIPT));
        let input = serde_json::to_value(&recovery.args).unwrap();
        let validator = jsonschema::validator_for(&input).unwrap();
        assert!(validator.is_valid(&serde_json::json!({
            "operation_id":"00000000-0000-0000-0000-000000000001"
        })));
        assert!(!validator.is_valid(&serde_json::json!({
            "operation_id":"00000000-0000-0000-0000-000000000001",
            "actor_id":"someone-else"
        })));
    }

    #[test]
    fn contract_reads_keep_the_complete_immutable_protocol_state() {
        let before = protocol_value().unwrap();
        let output = dispatch("get_contract", &serde_json::json!({})).unwrap();
        let after = protocol_value().unwrap();
        assert_eq!(before, after);
        let state = serde_json::to_value(after).unwrap();
        assert_eq!(state["contract"]["method"], "get_contract");
        assert_eq!(
            state["contract"]["schema_version"],
            output["contract"]["versioning"]["schema_version"]
        );
        assert_eq!(
            state["contract"]["source_commit"],
            output["contract"]["versioning"]["source_commit"]
        );
        assert!(state.get("invocation").is_some());
        assert!(state.get("types").is_none());
        assert!(output["contract"].get("types").is_some());
    }

    #[test]
    fn contract_survives_generated_protobuf_without_losing_schema_metadata() {
        use prost::Message;
        use prost_reflect::{DescriptorPool, DynamicMessage};

        let schema = tched_fs_schema();
        let returns =
            serde_json::to_value(schema.methods["get_contract"].returns.clone().unwrap()).unwrap();
        let blob = op_blob::blobify_plugin_schema(PLUGIN_NAME, schema);
        let pool = DescriptorPool::decode(blob.descriptor_set.as_slice()).unwrap();
        let descriptor = pool
            .get_message_by_name("operation.method.tched_fs.get_contract.GetContractOutput")
            .unwrap();
        let output = dispatch("get_contract", &serde_json::json!({})).unwrap();
        let message = DynamicMessage::deserialize(descriptor.clone(), &output).unwrap();
        let encoded = message.encode_to_vec();
        assert!(encoded.len() < 4 * 1024 * 1024);
        let decoded = DynamicMessage::decode(descriptor, encoded.as_slice()).unwrap();
        let transported = serde_json::to_value(decoded).unwrap();
        // JSON numbers may become protobuf doubles; numeric constraints retain
        // semantic equality while exact object validation ignores key order.
        if let Err(error) = jsonschema::validator_for(&returns)
            .unwrap()
            .validate(&transported)
        {
            panic!(
                "transported contract rejected at instance {} / schema {}",
                error.instance_path, error.schema_path
            );
        }
        let contract = &transported["contract"];
        assert_eq!(contract["$schema"], output["contract"]["$schema"]);
        assert_eq!(
            contract["$defs"].as_object().unwrap().len(),
            output["contract"]["$defs"].as_object().unwrap().len()
        );
        for inventory in ["nodes", "methods"] {
            assert_eq!(
                contract[inventory].as_array().unwrap().len(),
                output["contract"][inventory].as_array().unwrap().len()
            );
        }
        assert!(
            !contract["bindings"]["classes"]
                .as_object()
                .unwrap()
                .contains_key("Outcome")
        );
    }

    #[test]
    fn exact_output_schema_ignores_object_order_but_rejects_value_changes() {
        let value: serde_json::Value =
            serde_json::from_str(r#"{"a":1,"b":{"c":2,"d":3}}"#).unwrap();
        let reordered: serde_json::Value =
            serde_json::from_str(r#"{"b":{"d":3.0,"c":2.0},"a":1.0}"#).unwrap();
        let schema = literal_schema(&value);
        let validator = jsonschema::validator_for(&schema).unwrap();
        assert!(validator.is_valid(&reordered));
        assert!(!validator.is_valid(&serde_json::json!({"a": 1, "b": {"c": 2}})));
        assert!(!validator.is_valid(&serde_json::json!({"a": 1, "b": {"c": 2, "d": 4}})));
    }

    #[tokio::test]
    async fn rejects_protocol_mutation_and_false_checkpoint_support() {
        let plugin = ThreeTchedFsPlugin;
        let state = protocol_value().unwrap();
        assert!(plugin.verify_state(&state).await.unwrap());
        assert!(
            plugin
                .calculate_diff(&state, &simd_json::json!({}))
                .await
                .is_err()
        );
        assert!(plugin.create_checkpoint().await.is_err());
    }
}
