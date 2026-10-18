//! Canonical sealed declarations for the built-in agent catalog.
use std::collections::BTreeMap;
use std::sync::LazyLock;

use op_agents::{builtin_agent_descriptors, unified::AgentCategory};
use op_state_store::{MethodDecl, PluginSchema, SideEffect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use simd_json::prelude::*;

use super::cognitive::SpecialistOutput;
use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CatalogPersonaInput {
    #[schemars(length(min = 1))]
    pub question: String,
    pub path: Option<String>,
    pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CatalogExecutionOutput {
    pub agent_type: String,
    pub operation: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RustPathInput {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RustBuildRunInput {
    pub path: String,
    #[serde(default)]
    pub release: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PythonRunInput {
    #[schemars(length(min = 1))]
    pub code: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PythonPathArgsInput {
    pub path: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PythonLintInput {
    pub path: String,
    #[serde(default)]
    pub fix: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PythonPathInput {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PythonInstallInput {
    #[schemars(length(min = 1))]
    pub packages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JavaScriptRunInput {
    pub path: String,
    #[schemars(length(min = 1))]
    pub script: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoRunInput {
    pub path: String,
    pub file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShellInput {
    #[schemars(length(min = 1))]
    pub command: String,
    pub cwd: String,
    #[schemars(range(min = 1, max = 3600))]
    pub timeout: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentWorkflowStep {
    pub agent_type: String,
    pub operation: String,
    pub arguments: Value,
    pub condition: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CatalogWorkflowInput {
    pub steps: Option<Vec<AgentWorkflowStep>>,
    pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CatalogWorkflowRunOutput {
    pub ok: bool,
    pub workflow_agent: String,
    pub steps_completed: usize,
    pub results: Vec<Value>,
    pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CatalogWorkflowListOutput {
    pub workflow_agent: String,
    pub steps: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CatalogWorkflowValidateOutput {
    pub valid: bool,
    pub workflow_agent: String,
    pub errors: Vec<Value>,
    pub steps: Vec<Value>,
}

/// Compatibility aliases for bridge code that refers to the wire contract by
/// its shorter workflow names.
pub type CatalogAgentWorkflowStep = AgentWorkflowStep;
pub type CatalogWorkflowOutput = CatalogWorkflowRunOutput;

#[derive(Debug, Clone)]
pub struct CatalogAgentMethod {
    pub agent_type: String,
    pub operation: String,
    pub category: String,
    pub declaration: MethodDecl,
}

fn sanitized(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

pub fn agent_method_name(agent_type: &str, operation: &str) -> String {
    format!("agent_{}_{}", sanitized(agent_type), sanitized(operation))
}

fn hyphenated(value: &str) -> String {
    sanitized(value).replace('_', "-")
}

fn described(mut declaration: MethodDecl, agent: &str, operation: &str) -> MethodDecl {
    if let Some(object) = declaration.args.as_object_mut() {
        object.insert(
            "description".into(),
            simd_json::json!(format!("{} agent: {} operation", agent, operation)),
        );
    }
    declaration
}

fn declaration<Input: JsonSchema, Output: JsonSchema>(
    agent: &str,
    operation: &str,
    side: SideEffect,
    idempotent: bool,
) -> MethodDecl {
    let capability = format!("cognitive.agent.{}", sanitized(agent));
    let subid = format!(
        "{}.service.cognitive.agent.{}.{}@v1",
        if side == SideEffect::Read {
            "obs"
        } else {
            "mut"
        },
        hyphenated(agent),
        hyphenated(operation)
    );
    described(
        method_decl_from_schemars_with_output::<Input, Output>(
            &agent_method_name(agent, operation),
            side,
            idempotent,
            &capability,
            &subid,
        ),
        agent,
        operation,
    )
}

fn execution_decl(agent: &str, operation: &str) -> MethodDecl {
    let backend = op_agents::unified::GLOBAL_REGISTRY
        .get_normalized(agent)
        .expect("catalog executor is registered");
    match backend.id() {
        "rust-executor" => match operation {
            "build" | "run" => declaration::<RustBuildRunInput, CatalogExecutionOutput>(
                agent,
                operation,
                SideEffect::Mutation,
                false,
            ),
            _ => declaration::<RustPathInput, CatalogExecutionOutput>(
                agent,
                operation,
                SideEffect::Mutation,
                false,
            ),
        },
        "python-executor" => match operation {
            "run" => declaration::<PythonRunInput, CatalogExecutionOutput>(
                agent,
                operation,
                SideEffect::Mutation,
                false,
            ),
            "test" => declaration::<PythonPathArgsInput, CatalogExecutionOutput>(
                agent,
                operation,
                SideEffect::Mutation,
                false,
            ),
            "lint" => declaration::<PythonLintInput, CatalogExecutionOutput>(
                agent,
                operation,
                SideEffect::Mutation,
                false,
            ),
            "install" => declaration::<PythonInstallInput, CatalogExecutionOutput>(
                agent,
                operation,
                SideEffect::Mutation,
                false,
            ),
            _ => declaration::<PythonPathInput, CatalogExecutionOutput>(
                agent,
                operation,
                SideEffect::Mutation,
                false,
            ),
        },
        "javascript-executor" => match operation {
            "run" => declaration::<JavaScriptRunInput, CatalogExecutionOutput>(
                agent,
                operation,
                SideEffect::Mutation,
                false,
            ),
            _ => declaration::<PythonPathInput, CatalogExecutionOutput>(
                agent,
                operation,
                SideEffect::Mutation,
                false,
            ),
        },
        "go-executor" => match operation {
            "run" => declaration::<GoRunInput, CatalogExecutionOutput>(
                agent,
                operation,
                SideEffect::Mutation,
                false,
            ),
            _ => declaration::<RustPathInput, CatalogExecutionOutput>(
                agent,
                operation,
                SideEffect::Mutation,
                false,
            ),
        },
        "shell-executor" => declaration::<ShellInput, CatalogExecutionOutput>(
            agent,
            operation,
            SideEffect::Mutation,
            false,
        ),
        _ => panic!("unsupported execution agent in canonical catalog: {agent}"),
    }
}

fn orchestration_decl(agent: &str, operation: &str) -> MethodDecl {
    match operation {
        "run" | "run_workflow" => declaration::<CatalogWorkflowInput, CatalogWorkflowRunOutput>(
            agent,
            operation,
            SideEffect::Mutation,
            false,
        ),
        "list_steps" | "list" => declaration::<CatalogWorkflowInput, CatalogWorkflowListOutput>(
            agent,
            operation,
            SideEffect::Read,
            true,
        ),
        "validate" => declaration::<CatalogWorkflowInput, CatalogWorkflowValidateOutput>(
            agent,
            operation,
            SideEffect::Read,
            true,
        ),
        _ => panic!("unsupported orchestration operation in canonical catalog: {operation}"),
    }
}

fn build_catalog() -> BTreeMap<String, CatalogAgentMethod> {
    let mut catalog = BTreeMap::new();
    for descriptor in builtin_agent_descriptors() {
        let category = match descriptor.category {
            AgentCategory::Execution => "execution",
            AgentCategory::Persona => "persona",
            AgentCategory::Orchestration => "orchestration",
        };
        for operation in descriptor.operations {
            let mut declaration = match descriptor.category {
                AgentCategory::Persona => declaration::<CatalogPersonaInput, SpecialistOutput>(
                    &descriptor.agent_type,
                    &operation,
                    SideEffect::Read,
                    false,
                ),
                AgentCategory::Execution => execution_decl(&descriptor.agent_type, &operation),
                AgentCategory::Orchestration => {
                    orchestration_decl(&descriptor.agent_type, &operation)
                }
            };
            if let Some(arguments) = declaration.args.as_object_mut() {
                arguments.insert(
                    "description".into(),
                    simd_json::json!(format!(
                        "{} — {}: {}",
                        descriptor.name, operation, descriptor.description
                    )),
                );
            }
            let name = agent_method_name(&descriptor.agent_type, &operation);
            assert!(
                catalog
                    .insert(
                        name,
                        CatalogAgentMethod {
                            agent_type: descriptor.agent_type.clone(),
                            operation,
                            category: category.into(),
                            declaration
                        }
                    )
                    .is_none(),
                "duplicate canonical agent method"
            );
        }
    }
    catalog
}

static CATALOG: LazyLock<BTreeMap<String, CatalogAgentMethod>> = LazyLock::new(build_catalog);

pub fn catalog_agent_method(
    agent_type: &str,
    operation: &str,
) -> Option<&'static CatalogAgentMethod> {
    CATALOG.get(&agent_method_name(agent_type, operation))
}
pub fn method_by_name(name: &str) -> Option<&'static CatalogAgentMethod> {
    CATALOG.get(name)
}
pub fn catalog_agent_methods() -> impl Iterator<Item = &'static CatalogAgentMethod> {
    CATALOG.values()
}

pub fn append_catalog_methods(schema: &mut PluginSchema) {
    for method in catalog_agent_methods() {
        let capability = method
            .declaration
            .required_capability
            .clone()
            .expect("catalog capability");
        schema
            .capabilities
            .entry(capability.clone())
            .or_insert_with(|| op_state_store::CapabilityDecl {
                id: capability.clone(),
                description: format!("Sealed {} agent catalog operations", method.agent_type),
            });
        schema.methods.insert(
            agent_method_name(&method.agent_type, &method.operation),
            method.declaration.clone(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_plugins::common::oscal::validate_subid;
    #[test]
    fn catalog_is_unique_and_stable() {
        let methods: Vec<_> = catalog_agent_methods().collect();
        assert_eq!(
            methods.len(),
            methods
                .iter()
                .map(|m| agent_method_name(&m.agent_type, &m.operation))
                .collect::<std::collections::BTreeSet<_>>()
                .len()
        );
        for method in methods {
            validate_subid(&method.declaration.subid).unwrap();
            assert_eq!(
                method_by_name(&method.declaration.name).map(|m| m.agent_type.as_str()),
                Some(method.agent_type.as_str())
            );
        }
    }
    #[test]
    fn catalog_matches_builtin_descriptor_operations() {
        let expected: usize = builtin_agent_descriptors()
            .iter()
            .map(|d| d.operations.len())
            .sum();
        assert_eq!(catalog_agent_methods().count(), expected);
    }
}
