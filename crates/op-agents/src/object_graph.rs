//! Pure authored object graph contracts, independent of role/workstack lifecycle.
use std::collections::BTreeMap;
use schemars::JsonSchema;
use serde::{Serialize,Deserialize};
use serde_json::Value;
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all="snake_case")]
pub enum DefinitionStatus { Enabled, Disabled, Retired }
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag="kind", rename_all="snake_case", deny_unknown_fields)]
pub enum ObjectBinding {
    Operation { method: String, required_capability: String },
    ReadProperty { property: String, read_capability: String },
    AgentRecipe { recipe_id: String, recipe_version: u64, method: String, required_capability: String },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TypedPort {
    pub name: String,
    pub schema_pointer: String,
    pub contract_digest: String,
    pub contract: Value,
    pub required: bool,
    pub default: Option<Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ObjectNode {
    pub node_id: String,
    pub plugin_id: String,
    pub object_path: String,
    pub schema_hash: String,
    pub schema_version: String,
    pub policy_revision: u64,
    pub binding: ObjectBinding,
    pub input_ports: Vec<TypedPort>,
    pub output_ports: Vec<TypedPort>,
    pub arguments: BTreeMap<String, Value>,
    pub checkpoint: Option<CheckpointRequirement>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObjectConnection {
    pub from_node: String,
    pub from_port: String,
    pub to_node: String,
    pub to_port: String,
    pub expected_contract_digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObjectWorkflow {
    pub workflow_id: String,
    pub version: u64,
    pub name: String,
    pub description: String,
    pub status: DefinitionStatus,
    pub content_digest: String,
    pub nodes: Vec<ObjectNode>,
    pub connections: Vec<ObjectConnection>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag="kind",rename_all="snake_case",deny_unknown_fields)]
pub enum CheckpointRequirement {
    Approval { approver_capability: String, preview: String },
    Input { approver_capability: String, preview: String, response_schema: Value, input_port: String },
}
