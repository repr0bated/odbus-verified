//! Canonical method DTOs owned by gemma_brain's private UI workspace.
use super::super::super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
use super::*;
use op_state_store::{PluginSchema, SideEffect};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiEmptyInput {}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiSpecInput {
    pub spec_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiJobInput {
    pub job_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiListSpecsInput {
    #[serde(default)]
    pub after: Option<String>,
    #[serde(default)]
    pub limit: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiCreateSpecInput {
    pub spec: Value,
    #[serde(default)]
    pub tags: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiUpdateSpecInput {
    pub spec_id: String,
    pub expected_revision: u64,
    pub element_id: String,
    pub properties: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiDeleteSpecInput {
    pub spec_id: String,
    pub expected_revision: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiDeleteJobInput {
    pub job_id: String,
    pub expected_revision: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiRebindInput {
    pub expected_document_revision: u64,
    pub expected_previous_catalog_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiWorkspaceOutput {
    pub workspace: Option<UiWorkspaceRecovery>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiSpecOutput {
    pub spec: Option<UiSpecRecord>,
    /// Retained stale artifacts are manageable but not render-admitted.
    pub render_admitted: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiJobOutput {
    pub job: Option<UiGenerationJob>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiListedSpec {
    pub record: UiSpecRecord,
    pub render_admitted: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiListSpecsOutput {
    pub specs: Vec<UiListedSpec>,
    pub next_cursor: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiSpecMutationOutput {
    pub document_revision: u64,
    pub spec: UiSpecRecord,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiDeleteSpecOutput {
    pub document_revision: u64,
    pub spec_id: String,
    pub deleted_revision: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiDeleteJobOutput {
    pub document_revision: u64,
    pub job_id: String,
    pub deleted_revision: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiRebindOutput {
    pub document_revision: u64,
    pub previous_catalog_hash: String,
    pub catalog_hash: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiHandleEventInput {
    pub spec_id: String,
    pub expected_revision: u64,
    pub element_id: String,
    pub event: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiHandleEventOutput {
    pub document_revision: u64,
    pub spec: UiSpecRecord,
    pub event: String,
    pub action: String,
}

/// Input contracts have no owner ID, session ID, provider or storage path.
pub(crate) fn declare_methods(schema: &mut PluginSchema) {
    macro_rules! method {
        ($name:literal, $input:ty, $output:ty, $effect:ident, $idem:literal, $cap:literal, $subid:literal) => {
            schema.methods.insert(
                $name.into(),
                method_decl_from_schemars_with_output::<$input, $output>(
                    $name,
                    SideEffect::$effect,
                    $idem,
                    $cap,
                    $subid,
                ),
            );
        };
    }
    method!(
        "get_ui_workspace",
        UiEmptyInput,
        UiWorkspaceOutput,
        Read,
        true,
        "gemma.read",
        "obs.service.gemma.ui-workspace.get@v2"
    );
    method!(
        "get_ui_spec",
        UiSpecInput,
        UiSpecOutput,
        Read,
        true,
        "gemma.read",
        "obs.service.gemma.ui-spec.get@v2"
    );
    method!(
        "get_ui_job",
        UiJobInput,
        UiJobOutput,
        Read,
        true,
        "gemma.read",
        "obs.service.gemma.ui-job.get@v2"
    );
    method!(
        "list_ui_specs",
        UiListSpecsInput,
        UiListSpecsOutput,
        Read,
        true,
        "gemma.read",
        "obs.service.gemma.ui-spec.list@v2"
    );
    method!(
        "create_ui_spec",
        UiCreateSpecInput,
        UiSpecMutationOutput,
        Mutation,
        false,
        "gemma.invoke",
        "mut.service.gemma.ui-spec.create@v2"
    );
    method!(
        "update_ui_spec",
        UiUpdateSpecInput,
        UiSpecMutationOutput,
        Mutation,
        false,
        "gemma.invoke",
        "mut.service.gemma.ui-spec.update@v2"
    );
    method!(
        "handle_ui_event",
        UiHandleEventInput,
        UiHandleEventOutput,
        Mutation,
        false,
        "gemma.invoke",
        "mut.service.gemma.ui-event.handle@v1"
    );
    method!(
        "delete_ui_spec",
        UiDeleteSpecInput,
        UiDeleteSpecOutput,
        Mutation,
        false,
        "gemma.invoke",
        "mut.service.gemma.ui-spec.delete@v2"
    );
    method!(
        "delete_ui_job",
        UiDeleteJobInput,
        UiDeleteJobOutput,
        Mutation,
        false,
        "gemma.invoke",
        "mut.service.gemma.ui-job.delete@v2"
    );
    method!(
        "rebind_ui_renderer",
        UiRebindInput,
        UiRebindOutput,
        Mutation,
        false,
        "gemma.invoke",
        "mut.service.gemma.ui-renderer.rebind@v2"
    );
    method!(
        "recover_ui_workspace",
        UiEmptyInput,
        UiWorkspaceOutput,
        Mutation,
        true,
        "gemma.invoke",
        "mut.service.gemma.ui-workspace.recover@v2"
    );
}

impl UiWorkspaceMethods {
    /// Trusted engine adapter: call only after canonical identity/capability/
    /// contract admission. The returned full projection is engine-private;
    /// publish it before returning `UiWorkspaceCommit::method_result`.
    pub async fn dispatch_mutation(
        &self,
        access: &UiAccessContext,
        method: &str,
        args: &Value,
        engine_minted_spec_id: Option<&str>,
    ) -> Result<UiWorkspaceCommit> {
        reject_precommit(check_json(
            args,
            JsonLimits {
                max_depth: 68,
                max_nodes: 70_000,
                max_bytes: MAX_SPEC_BYTES + 8192,
            },
        ))?;
        fn decode<T: serde::de::DeserializeOwned>(args: &Value) -> Result<T> {
            reject_precommit(
                serde_json::from_value(args.clone())
                    .map_err(|_| anyhow::anyhow!("invalid UI workspace arguments")),
            )
        }
        match method {
            "create_ui_spec" => {
                let input: UiCreateSpecInput = decode(args)?;
                self.create_spec(
                    access,
                    reject_precommit(
                        engine_minted_spec_id.context("engine-minted spec ID is required"),
                    )?,
                    input.spec,
                    input.tags,
                )
                .await
            }
            "update_ui_spec" => {
                let input: UiUpdateSpecInput = decode(args)?;
                self.update_spec_properties(
                    access,
                    &input.spec_id,
                    input.expected_revision,
                    &input.element_id,
                    input.properties,
                )
                .await
            }
            "handle_ui_event" => {
                let input: UiHandleEventInput = decode(args)?;
                self.handle_event(
                    access,
                    &input.spec_id,
                    input.expected_revision,
                    &input.element_id,
                    &input.event,
                )
                .await
            }
            "delete_ui_spec" => {
                let input: UiDeleteSpecInput = decode(args)?;
                self.delete_spec(access, &input.spec_id, input.expected_revision)
                    .await
            }
            "delete_ui_job" => {
                let input: UiDeleteJobInput = decode(args)?;
                self.delete_job(access, &input.job_id, input.expected_revision)
                    .await
            }
            "rebind_ui_renderer" => {
                let input: UiRebindInput = decode(args)?;
                self.rebind_renderer(
                    access,
                    input.expected_document_revision,
                    &input.expected_previous_catalog_hash,
                )
                .await
            }
            "register_tag" => {
                let input: super::super::methods::RegisterTagInput = decode(args)?;
                let owner = reject_precommit(Owner::new(access))?;
                self.update(owner, move |mut workspace| {
                    let output = taxonomy_methods::register_tag(&mut workspace.taxonomy, input)?;
                    Ok((workspace, UiMutationReceipt::TagRegistered { output }))
                })
                .await
            }
            _ => reject_precommit(Err(anyhow::anyhow!(
                "method is not a UI workspace mutation"
            ))),
        }
    }
}

impl UiWorkspaceCommit {
    /// No unrelated private siblings leak through a mutation's output DTO.
    pub fn method_result(&self) -> Result<Value> {
        match &self.receipt {
            UiMutationReceipt::SpecCreated { spec } | UiMutationReceipt::SpecUpdated { spec } => {
                Ok(serde_json::to_value(UiSpecMutationOutput {
                    document_revision: self.document_revision,
                    spec: spec.clone(),
                })?)
            }
            UiMutationReceipt::SpecDeleted {
                spec_id,
                deleted_revision,
            } => Ok(serde_json::to_value(UiDeleteSpecOutput {
                document_revision: self.document_revision,
                spec_id: spec_id.clone(),
                deleted_revision: *deleted_revision,
            })?),
            UiMutationReceipt::JobDeleted {
                job_id,
                deleted_revision,
            } => Ok(serde_json::to_value(UiDeleteJobOutput {
                document_revision: self.document_revision,
                job_id: job_id.clone(),
                deleted_revision: *deleted_revision,
            })?),
            UiMutationReceipt::RendererRebound {
                previous_catalog_hash,
                catalog_hash,
            } => Ok(serde_json::to_value(UiRebindOutput {
                document_revision: self.document_revision,
                previous_catalog_hash: previous_catalog_hash.clone(),
                catalog_hash: catalog_hash.clone(),
            })?),
            UiMutationReceipt::TagRegistered { output } => Ok(serde_json::to_value(output)?),
            UiMutationReceipt::EventHandled {
                spec,
                event,
                action,
            } => Ok(serde_json::to_value(UiHandleEventOutput {
                document_revision: self.document_revision,
                spec: spec.clone(),
                event: event.clone(),
                action: action.clone(),
            })?),
            _ => anyhow::bail!("internal job transition has no public mutation result"),
        }
    }
}

pub(crate) fn page_specs(
    snapshot: Option<&UiWorkspaceProjection>,
    after: Option<&str>,
    limit: usize,
    current_catalog_hash: &str,
) -> UiListSpecsOutput {
    let mut records = snapshot
        .into_iter()
        .flat_map(|snapshot| snapshot.specs.values())
        .filter(|record| after.is_none_or(|cursor| record.id.as_str() > cursor));
    let specs: Vec<_> = records
        .by_ref()
        .take(limit)
        .map(|record| UiListedSpec {
            record: record.clone(),
            render_admitted: record.catalog_hash == current_catalog_hash,
        })
        .collect();
    let next_cursor = records
        .next()
        .and_then(|_| specs.last().map(|spec| spec.record.id.clone()));
    UiListSpecsOutput { specs, next_cursor }
}

impl UiWorkspaceRecovery {
    /// The canonical gemma_brain private root is flat: document_revision,
    /// catalog_hash, jobs and specs. This domain receipt's projection envelope
    /// is not a second state shape and must not be written directly to SHM.
    pub fn plugin_state(&self) -> Result<Value> {
        ensure!(self.document_revision > 0, "invalid UI document revision");
        validate_workspace_content(
            &self.projection.catalog_hash,
            &self.projection.jobs,
            &self.projection.specs,
        )?;
        taxonomy_methods::validate_taxonomy(&GemmaTaxonomy {
            revision: self.projection.taxonomy_revision,
            tags: self.projection.tags.clone(),
        })?;
        let mut state = serde_json::to_value(&self.projection)?;
        state
            .as_object_mut()
            .context("UI projection must be an object")?
            .insert(
                "document_revision".into(),
                Value::from(self.document_revision),
            );
        validate_document_bound(&state)?;
        Ok(state)
    }

    /// Decode only an already committed, authorized private-SHM plugin root.
    /// The bridge supplies its principal/session scope; no caller owner field,
    /// Cozo read, defaults, or stale process cache can provide that authority.
    pub fn from_plugin_state(state: &Value) -> Result<Self> {
        validate_document_bound(state)?;
        let mut object = state
            .as_object()
            .context("invalid committed UI root")?
            .clone();
        let document_revision = object
            .remove("document_revision")
            .and_then(|v| v.as_u64())
            .context("invalid committed UI revision")?;
        ensure!(document_revision > 0, "invalid committed UI revision");
        let projection: UiWorkspaceProjection = serde_json::from_value(Value::Object(object))
            .map_err(|_| anyhow::anyhow!("invalid committed UI projection"))?;
        validate_workspace_content(
            &projection.catalog_hash,
            &projection.jobs,
            &projection.specs,
        )?;
        taxonomy_methods::validate_taxonomy(&GemmaTaxonomy {
            revision: projection.taxonomy_revision,
            tags: projection.tags.clone(),
        })?;
        Ok(Self {
            document_revision,
            projection,
        })
    }
}
impl UiWorkspaceCommit {
    pub fn plugin_state(&self) -> Result<Value> {
        UiWorkspaceRecovery {
            document_revision: self.document_revision,
            projection: self.projection.clone(),
        }
        .plugin_state()
    }
}
impl UiWorkspaceMethods {
    /// Pure read over a committed private-SHM receipt, after bridge authorization.
    /// Never reads Cozo or chooses a principal, directory, cache or catalog.
    pub fn dispatch_committed_read(
        &self,
        method: &str,
        args: &Value,
        snapshot: Option<UiWorkspaceRecovery>,
    ) -> Result<Value> {
        check_json(
            args,
            JsonLimits {
                max_depth: 8,
                max_nodes: 64,
                max_bytes: 72 * 1024,
            },
        )?;
        fn decode<T: serde::de::DeserializeOwned>(args: &Value) -> Result<T> {
            serde_json::from_value(args.clone())
                .map_err(|_| anyhow::anyhow!("invalid UI workspace arguments"))
        }
        if let Some(snapshot) = &snapshot {
            ensure!(
                snapshot.document_revision > 0,
                "invalid committed UI revision"
            );
            validate_workspace_content(
                &snapshot.projection.catalog_hash,
                &snapshot.projection.jobs,
                &snapshot.projection.specs,
            )?;
            taxonomy_methods::validate_taxonomy(&GemmaTaxonomy {
                revision: snapshot.projection.taxonomy_revision,
                tags: snapshot.projection.tags.clone(),
            })?;
        }
        match method {
            "get_ui_workspace" => {
                let _: UiEmptyInput = decode(args)?;
                Ok(serde_json::to_value(UiWorkspaceOutput {
                    workspace: snapshot,
                })?)
            }
            "get_ui_spec" => {
                let input: UiSpecInput = decode(args)?;
                validate_id("spec_id", &input.spec_id)?;
                let record = snapshot
                    .and_then(|snapshot| snapshot.projection.specs.get(&input.spec_id).cloned());
                Ok(serde_json::to_value(self.spec_view(record))?)
            }
            "get_ui_job" => {
                let input: UiJobInput = decode(args)?;
                validate_id("job_id", &input.job_id)?;
                let job = snapshot
                    .and_then(|snapshot| snapshot.projection.jobs.get(&input.job_id).cloned());
                Ok(serde_json::to_value(UiJobOutput { job })?)
            }
            "list_ui_specs" => {
                let input: UiListSpecsInput = decode(args)?;
                let limit = if input.limit == 0 {
                    50
                } else {
                    input.limit as usize
                };
                ensure!(limit <= MAX_SPECS, "UI list limit exceeds capacity");
                if let Some(cursor) = &input.after {
                    validate_id("cursor", cursor)?;
                }
                let mut page = page_specs(
                    snapshot.as_ref().map(|snapshot| &snapshot.projection),
                    input.after.as_deref(),
                    limit,
                    &self.catalog_hash,
                );
                for item in &mut page.specs {
                    item.render_admitted = self.record_is_render_admitted(&item.record);
                }
                Ok(serde_json::to_value(page)?)
            }
            "list_tags" => {
                let _: super::super::methods::ListTaxonomyInput = decode(args)?;
                let taxonomy = snapshot
                    .map(|snapshot| GemmaTaxonomy {
                        revision: snapshot.projection.taxonomy_revision,
                        tags: snapshot.projection.tags,
                    })
                    .unwrap_or_default();
                Ok(serde_json::to_value(taxonomy_methods::list_tags(
                    &taxonomy,
                )?)?)
            }
            "list_perspectives" => {
                let _: super::super::methods::ListTaxonomyInput = decode(args)?;
                let taxonomy = snapshot
                    .map(|snapshot| GemmaTaxonomy {
                        revision: snapshot.projection.taxonomy_revision,
                        tags: snapshot.projection.tags,
                    })
                    .unwrap_or_default();
                Ok(serde_json::to_value(taxonomy_methods::list_perspectives(
                    &taxonomy,
                )?)?)
            }
            "classify" => {
                let input: super::super::methods::ClassifyInput = decode(args)?;
                let taxonomy = snapshot
                    .map(|snapshot| GemmaTaxonomy {
                        revision: snapshot.projection.taxonomy_revision,
                        tags: snapshot.projection.tags,
                    })
                    .unwrap_or_default();
                Ok(serde_json::to_value(taxonomy_methods::classify(
                    &taxonomy, input,
                )?)?)
            }
            "analyze_intent" => {
                let input: super::super::methods::AnalyzeIntentInput = decode(args)?;
                let taxonomy = snapshot
                    .map(|snapshot| GemmaTaxonomy {
                        revision: snapshot.projection.taxonomy_revision,
                        tags: snapshot.projection.tags,
                    })
                    .unwrap_or_default();
                Ok(serde_json::to_value(taxonomy_methods::analyze_intent(
                    &taxonomy, input,
                )?)?)
            }
            _ => anyhow::bail!("method is not a UI workspace read"),
        }
    }
}
