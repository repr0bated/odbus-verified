//! Native-observation and durable role-assignment methods 
//! chat_router surface. This module owns no provider/config/schema authority.

use super::runtime_documents::{RuntimeDocument, RuntimeDocuments};
use super::chat_router::{ObservedChatRouterState, ResolveRouteInput};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc};

const ASSIGNMENTS_DOCUMENT_ID: &str = "chat_router.model_assignments";
const MAX_MODEL_ID_BYTES: usize = 256;
const MAX_HINT_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelRole {
    OvsRouting,
    Obfuscation,
    Vectorization,
    QdrantRetrieval,
    CozoRetrieval,
}

impl ModelRole {
    pub fn method(self) -> &'static str {
        match self {
            Self::OvsRouting => "SetOvsRoutingModel",
            Self::Obfuscation => "SetObfuscationModel",
            Self::Vectorization => "SetVectorizationModel",
            Self::QdrantRetrieval => "SetQdrantRetrievalModel",
            Self::CozoRetrieval => "SetCozoRetrievalModel",
        }
    }

    fn field(self) -> &'static str {
        match self {
            Self::OvsRouting => "ovs_routing",
            Self::Obfuscation => "obfuscation",
            Self::Vectorization => "vectorization",
            Self::QdrantRetrieval => "qdrant_retrieval",
            Self::CozoRetrieval => "cozo_retrieval",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct DurableModelAssignments {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ovs_routing: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub obfuscation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vectorization: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qdrant_retrieval: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cozo_retrieval: Option<String>,
}

impl DurableModelAssignments {
    fn set(&mut self, role: ModelRole, model: String) -> bool {
        let slot = match role {
            ModelRole::OvsRouting => &mut self.ovs_routing,
            ModelRole::Obfuscation => &mut self.obfuscation,
            ModelRole::Vectorization => &mut self.vectorization,
            ModelRole::QdrantRetrieval => &mut self.qdrant_retrieval,
            ModelRole::CozoRetrieval => &mut self.cozo_retrieval,
        };
        if slot.as_deref() == Some(model.as_str()) {
            false
        } else {
            *slot = Some(model);
            true
        }
    }

    fn get(&self, role: ModelRole) -> Option<&str> {
        match role {
            ModelRole::OvsRouting => self.ovs_routing.as_deref(),
            ModelRole::Obfuscation => self.obfuscation.as_deref(),
            ModelRole::Vectorization => self.vectorization.as_deref(),
            ModelRole::QdrantRetrieval => self.qdrant_retrieval.as_deref(),
            ModelRole::CozoRetrieval => self.cozo_retrieval.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RoleAssignmentReceipt {
    pub role: &'static str,
    pub model_id: String,
    pub model_assignments: DurableModelAssignments,
    pub document_revision: u64,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LegacyObservation {
    pub output: Value,
    /// Complete set of fields this adapter can prove in one observation.
    pub represented_state: Value,
}

pub struct ChatRouterLegacyMethods {
    documents: Arc<RuntimeDocuments>,
}

impl ChatRouterLegacyMethods {
    pub fn new(documents: Arc<RuntimeDocuments>) -> Self {
        Self { documents }
    }

    pub async fn assignments(&self) -> Result<(u64, DurableModelAssignments)> {
        match self.documents.read_durable(ASSIGNMENTS_DOCUMENT_ID).await? {
            None => Ok((0, DurableModelAssignments::default())),
            Some(row) => parse_document(row),
        }
    }

    pub async fn observe(
        &self,
        method: &str,
        args: &Value,
        observed: &ObservedChatRouterState,
    ) -> Result<LegacyObservation> {
        validate_observation(observed)?;
        let (_, assignments) = self.assignments().await?;
        let represented_state = represented_state(observed, &assignments)?;
        let output = match method {
            "GetModelRoutes" => json!({"model_routes": observed.model_routes.clone()}),
            "ResolveRoute" => {
                let input: ResolveRouteInput =
                    serde_json::from_value(args.clone()).context("invalid ResolveRoute input")?;
                let hint = bounded_nonempty("hint", &input.hint, MAX_HINT_BYTES)?;
                let matches = observed
                    .model_routes
                    .iter()
                    .filter(|route| route.hint == hint || route.model == hint)
                    .collect::<Vec<_>>();
                anyhow::ensure!(
                    !matches.is_empty(),
                    "route hint is not declared by the native runtime"
                );
                anyhow::ensure!(
                    matches.len() == 1,
                    "route hint is ambiguous in the native runtime catalog"
                );
                json!({"route": matches[0]})
            }
            _ => anyhow::bail!("unsupported retained runtime read: {method}"),
        };
        Ok(LegacyObservation {
            output,
            represented_state,
        })
    }

    /// Commit one assignment with RuntimeDocuments CAS. The caller must hold
    /// the process-wide chat runtime selection guard across native catalog
    /// observation, this commit, readback and SHM publication.
    pub async fn assign(
        &self,
        role: ModelRole,
        model_id: &str,
        observed: &ObservedChatRouterState,
    ) -> Result<(RoleAssignmentReceipt, Value)> {
        validate_observation(observed)?;
        let model_id = bounded_nonempty("model_id", model_id, MAX_MODEL_ID_BYTES)?.to_owned();
        let bindings = observed
            .model_routes
            .iter()
            .filter(|route| route.model == model_id)
            .map(|route| (route.provider.as_str(), route.model.as_str()))
            .collect::<BTreeSet<_>>();
        anyhow::ensure!(
            !bindings.is_empty(),
            "model_id is absent from the native runtime catalog"
        );
        anyhow::ensure!(
            bindings.len() == 1,
            "model_id is ambiguous across native providers"
        );

        let role_copy = role;
        let model_for_commit = model_id.clone();
        let (row, changed) = self
            .documents
            .update_with_result(ASSIGNMENTS_DOCUMENT_ID, move |old| {
                let mut assignments = match old {
                    None => DurableModelAssignments::default(),
                    Some(value) => serde_json::from_value(value)
                        .context("stored chat_router model assignments are malformed")?,
                };
                let changed = assignments.set(role_copy, model_for_commit);
                Ok((serde_json::to_value(assignments)?, changed))
            })
            .await?;
        let (revision, assignments) = parse_document(row)?;
        let committed_model = assignments
            .get(role)
            .context("committed assignment row omitted the requested role")?
            .to_owned();
        let receipt = RoleAssignmentReceipt {
            role: role.field(),
            model_id: committed_model,
            model_assignments: assignments.clone(),
            document_revision: revision,
            changed,
        };
        Ok((receipt, represented_state(observed, &assignments)?))
    }
}

fn parse_document(row: RuntimeDocument) -> Result<(u64, DurableModelAssignments)> {
    let assignments = serde_json::from_value(row.document)
        .context("stored chat_router model assignments are malformed")?;
    Ok((row.revision, assignments))
}

fn bounded_nonempty<'a>(field: &str, value: &'a str, max: usize) -> Result<&'a str> {
    let value = value.trim();
    anyhow::ensure!(!value.is_empty(), "{field} must not be empty");
    anyhow::ensure!(value.len() <= max, "{field} exceeds {max} bytes");
    anyhow::ensure!(
        !value.chars().any(char::is_control),
        "{field} contains control characters"
    );
    Ok(value)
}

fn validate_observation(observed: &ObservedChatRouterState) -> Result<()> {
    anyhow::ensure!(
        !observed.providers.is_empty(),
        "native provider catalog is empty"
    );
    let mut provider_ids = BTreeSet::new();
    for provider in &observed.providers {
        anyhow::ensure!(
            !provider.id.trim().is_empty() && provider_ids.insert(provider.id.as_str()),
            "native provider catalog contains an empty or duplicate provider id"
        );
    }
    let mut routes = BTreeSet::new();
    for route in &observed.model_routes {
        anyhow::ensure!(
            !route.hint.trim().is_empty()
                && !route.model.trim().is_empty()
                && provider_ids.contains(route.provider.as_str())
                && routes.insert((
                    route.hint.as_str(),
                    route.provider.as_str(),
                    route.model.as_str()
                )),
            "native model route is empty, duplicated, or references an unknown provider"
        );
    }
    anyhow::ensure!(
        provider_ids.contains(observed.selected_provider.as_str()),
        "selected provider is absent from native catalog"
    );
    anyhow::ensure!(
        observed
            .model_routes
            .iter()
            .any(|route| route.provider == observed.selected_provider
                && route.model == observed.selected_model),
        "selected model binding is absent from native catalog"
    );
    Ok(())
}

pub fn represented_state(
    observed: &ObservedChatRouterState,
    assignments: &DurableModelAssignments,
) -> Result<Value> {
    Ok(json!({
        "status": observed.status.clone(),
        "selected_provider": observed.selected_provider.clone(),
        "selected_model": observed.selected_model.clone(),
        "providers": observed.providers.clone(),
        "model_routes": observed.model_routes.clone(),
        "model_assignments": assignments,
    }))
}

#[cfg(test)]
#[path = "chat_router_legacy_methods_tests.rs"]
mod tests;
