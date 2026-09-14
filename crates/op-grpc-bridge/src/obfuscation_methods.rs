//! The obfuscation plugin owns only its explicit normalization rules. The
//! existing OpenFlow D-Bus controller owns wire encoding, live comparison,
//! barriers and readback. No legacy rule string is installed here.
use anyhow::{ensure, Context, Result};
use async_trait::async_trait;
use op_plugins::state_plugins::{
    openflow::{FlowAction, FlowEntry},
    openflow_obfuscation::{
        DeobfuscateFlowInput, ObfuscateFlowInput, ObfuscationOutput, ObfuscationRecord,
        ObfuscationState,
    },
    runtime_documents::RuntimeDocuments,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::Mutex;

const PLUGIN: &str = "openflow_obfuscation";

#[async_trait]
pub(crate) trait FlowOwner: Send + Sync {
    async fn matches(&self, bridge: &str, flow: &FlowEntry, absent: bool) -> Result<bool>;
    async fn exchange(
        &self,
        bridge: &str,
        flow: &FlowEntry,
        expected: Option<&FlowEntry>,
        remove: bool,
    ) -> Result<()>;
}

pub(crate) struct NativeFlowOwner;
#[async_trait]
impl FlowOwner for NativeFlowOwner {
    async fn matches(&self, bridge: &str, flow: &FlowEntry, absent: bool) -> Result<bool> {
        let bus = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            zbus::Connection::system(),
        )
        .await
        .context("OpenFlow owner bus connection timed out")??;
        let proxy = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            zbus::Proxy::new(
                &bus,
                "org.opdbus.v1.plugins.openflow",
                "/org/opdbus/v1/plugins/openflow",
                "org.opdbus.v1.plugins.openflow",
            ),
        )
        .await
        .context("OpenFlow owner proxy connection timed out")??;
        tokio::time::timeout(
            std::time::Duration::from_secs(15),
            proxy.call(
                "VerifyFlow",
                &(bridge, serde_json::to_string(flow)?, absent),
            ),
        )
        .await
        .context("OpenFlow native recovery read timed out")?
        .map_err(Into::into)
    }
    async fn exchange(
        &self,
        bridge: &str,
        flow: &FlowEntry,
        expected: Option<&FlowEntry>,
        remove: bool,
    ) -> Result<()> {
        let bus = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            zbus::Connection::system(),
        )
        .await
        .context("OpenFlow owner bus connection timed out")??;
        let proxy = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            zbus::Proxy::new(
                &bus,
                "org.opdbus.v1.plugins.openflow",
                "/org/opdbus/v1/plugins/openflow",
                "org.opdbus.v1.plugins.openflow",
            ),
        )
        .await
        .context("OpenFlow owner proxy connection timed out")??;
        let expected = expected
            .map(serde_json::to_string)
            .transpose()?
            .unwrap_or_default();
        let result: String = tokio::time::timeout(
            std::time::Duration::from_secs(45),
            proxy.call(
                "CompareExchangeFlow",
                &(bridge, serde_json::to_string(flow)?, expected, remove),
            ),
        )
        .await
        .context("OpenFlow owner timeout; reconcile before retry")??;
        let result: Value = serde_json::from_str(&result)?;
        ensure!(
            result.get("verified") == Some(&Value::Bool(true))
                && result.get("flow") == Some(&serde_json::to_value(flow)?),
            "OpenFlow owner did not verify the requested flow"
        );
        Ok(())
    }
}

pub(crate) struct ObfuscationMethods {
    documents: Arc<RuntimeDocuments>,
    owner: Arc<dyn FlowOwner>,
    operation: Mutex<()>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct DurableFlows {
    revision: u64,
    flows: BTreeMap<String, ObfuscationRecord>,
    pending: Option<PendingFlow>,
    completed: Option<PendingFlow>,
}

#[derive(Clone, Serialize, Deserialize)]
struct PendingFlow {
    method: String,
    arguments: Value,
    flow_id: String,
    record: ObfuscationRecord,
    installed: bool,
}

impl PendingFlow {
    fn is_retry(&self, method: &str, args: &Value) -> bool {
        self.method == method && self.arguments == *args
    }
    fn output(&self, revision: u64) -> Result<Value> {
        Ok(serde_json::to_value(ObfuscationOutput {
            flow_id: self.flow_id.clone(),
            revision,
            installed: self.installed,
            record: self.record.clone(),
        })?)
    }
}

impl ObfuscationMethods {
    pub(crate) fn new(documents: Arc<RuntimeDocuments>, owner: Arc<dyn FlowOwner>) -> Self {
        Self {
            documents,
            owner,
            operation: Mutex::new(()),
        }
    }

    pub(crate) async fn dispatch(
        &self,
        method: &str,
        args: Value,
    ) -> Result<(Value, ObfuscationState)> {
        let _operation = self.operation.lock().await;
        let row = self.documents.read_durable(PLUGIN).await?;
        let mut document_revision = row.as_ref().map_or(0, |row| row.revision);
        let mut document: DurableFlows = row
            .map(|row| serde_json::from_value(row.document))
            .transpose()?
            .unwrap_or_default();
        if let Some(completed) = &document.completed {
            if completed.is_retry(method, &args) {
                ensure!(
                    self.owner
                        .matches(
                            &completed.record.bridge,
                            &completed.record.flow,
                            !completed.installed
                        )
                        .await?,
                    "completed normalization flow changed outside its owner"
                );
                return Ok((
                    completed.output(document.revision)?,
                    ObfuscationState {
                        revision: document.revision,
                        flows: document.flows,
                    },
                ));
            }
        }
        if let Some(pending) = &document.pending {
            ensure!(pending.is_retry(method, &args), "normalization operation {} for {} needs its exact request retried before another change", pending.method, pending.flow_id);
            return self.finish(document, document_revision).await;
        }
        let revision = document.revision;
        let flows = &document.flows;
        let original_arguments = args.clone();
        let (flow_id, record, installed) = match method {
            "obfuscate_flow" => {
                let input: ObfuscateFlowInput = serde_json::from_value(args)?;
                ensure!(
                    revision == input.expected_revision,
                    "obfuscation revision conflict"
                );
                let record = normalize(input)?;
                let flow_id = flow_identity(&record.bridge, &record.flow)?;
                ensure!(
                    flows.len() < 1024 || flows.contains_key(&flow_id),
                    "obfuscation flow count limit"
                );
                (flow_id, record, true)
            }
            "deobfuscate_flow" => {
                let input: DeobfuscateFlowInput = serde_json::from_value(args)?;
                ensure!(
                    revision == input.expected_revision,
                    "obfuscation revision conflict"
                );
                let record = flows
                    .get(&input.flow_id)
                    .context("unknown owned obfuscation flow")?
                    .clone();
                ensure!(
                    record.bridge == input.bridge,
                    "obfuscation flow belongs to another bridge"
                );
                (input.flow_id, record, false)
            }
            _ => anyhow::bail!("unknown obfuscation method"),
        };
        document.pending = Some(PendingFlow {
            method: method.into(),
            arguments: original_arguments,
            flow_id,
            record,
            installed,
        });
        // Persist the exact intent before touching the switch. A process loss
        // leaves enough information to reconcile without inventing a receipt.
        document_revision = self.persist(&document, document_revision).await?;
        self.finish(document, document_revision).await
    }

    async fn persist(&self, document: &DurableFlows, revision: u64) -> Result<u64> {
        let value = serde_json::to_value(document)?;
        Ok(self
            .documents
            .update_at_revision_with_result(PLUGIN, revision, move |_| Ok((value, ())))
            .await?
            .0
            .revision)
    }

    async fn finish(
        &self,
        mut document: DurableFlows,
        revision: u64,
    ) -> Result<(Value, ObfuscationState)> {
        let pending = document
            .pending
            .clone()
            .context("no pending normalization operation")?;
        let prior = document
            .flows
            .get(&pending.flow_id)
            .map(|record| &record.flow);
        if !self
            .owner
            .matches(
                &pending.record.bridge,
                &pending.record.flow,
                !pending.installed,
            )
            .await?
        {
            if let Err(error) = self
                .owner
                .exchange(
                    &pending.record.bridge,
                    &pending.record.flow,
                    prior,
                    !pending.installed,
                )
                .await
            {
                // A proven unchanged switch permits clearing a failed intent.
                // Transport uncertainty preserves it for an exact retry.
                let prior_matches = match prior {
                    Some(flow) => {
                        self.owner
                            .matches(&pending.record.bridge, flow, false)
                            .await
                    }
                    None => {
                        self.owner
                            .matches(&pending.record.bridge, &pending.record.flow, true)
                            .await
                    }
                };
                if matches!(prior_matches, Ok(true)) {
                    document.pending = None;
                    self.persist(&document, revision).await?;
                }
                return Err(error);
            }
        }
        ensure!(
            self.owner
                .matches(
                    &pending.record.bridge,
                    &pending.record.flow,
                    !pending.installed
                )
                .await?,
            "normalization readback changed before receipt commit"
        );
        if pending.installed {
            document
                .flows
                .insert(pending.flow_id.clone(), pending.record.clone());
        } else {
            document.flows.remove(&pending.flow_id);
        }
        document.revision = document
            .revision
            .checked_add(1)
            .context("normalization revision exhausted")?;
        document.pending = None;
        document.completed = Some(pending.clone());
        self.persist(&document, revision).await.context(
            "native flow verified; retry the identical request to recover its durable receipt",
        )?;
        Ok((
            pending.output(document.revision)?,
            ObfuscationState {
                revision: document.revision,
                flows: document.flows,
            },
        ))
    }
}

fn normalize(input: ObfuscateFlowInput) -> Result<ObfuscationRecord> {
    ensure!(
        !input.bridge.is_empty()
            && input.bridge.len() <= 15
            && input
                .bridge
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
        "invalid bridge name"
    );
    ensure!(
        input.ttl != 0 && input.flow.table < 255 && input.flow.priority != 0,
        "invalid normalization table/priority/TTL"
    );
    ensure!(
        input.flow.cookie.is_none(),
        "normalization cookie is owner-assigned"
    );
    ensure!(
        input.flow.idle_timeout == 0 && input.flow.hard_timeout == 0,
        "owned normalization flows require explicit removal"
    );
    ensure!(
        input.flow.match_fields.contains_key("ip")
            || input.flow.match_fields.contains_key("ipv6")
            || input.flow.match_fields.contains_key("tcp")
            || input.flow.match_fields.contains_key("udp"),
        "TTL normalization requires an explicit IP match"
    );
    ensure!(
        !input.flow.actions.is_empty()
            && !input
                .flow
                .actions
                .iter()
                .any(|a| matches!(a, FlowAction::SetField { field, .. } if field == "nw_ttl")),
        "explicit forwarding actions must not override normalization TTL"
    );
    // Normalize HashMap order before hashing so the same exact rule has a
    // stable ID independent of JSON field insertion order.
    let hash = flow_identity_hash(&input.bridge, &input.flow)?;
    let cookie = (u64::from_be_bytes(hash.as_bytes()[..8].try_into()?) & 0x0000_ffff_ffff_ffff)
        | 0x4f42_0000_0000_0000;
    let mut flow = input.flow;
    flow.cookie = Some(cookie);
    flow.actions.insert(
        0,
        FlowAction::SetField {
            field: "nw_ttl".into(),
            value: input.ttl.to_string(),
        },
    );
    Ok(ObfuscationRecord {
        bridge: input.bridge,
        flow,
        ttl: input.ttl,
    })
}

fn flow_identity_hash(bridge: &str, flow: &FlowEntry) -> Result<blake3::Hash> {
    let identity = serde_json::json!({"bridge":bridge,"table":flow.table,"priority":flow.priority,"match":flow.match_fields.iter().collect::<BTreeMap<_,_>>()});
    Ok(blake3::hash(&serde_json::to_vec(&identity)?))
}

fn flow_identity(bridge: &str, flow: &FlowEntry) -> Result<String> {
    Ok(flow_identity_hash(bridge, flow)?.to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Switch(Mutex<Option<FlowEntry>>);
    #[async_trait]
    impl FlowOwner for Switch {
        async fn matches(&self, _bridge: &str, flow: &FlowEntry, absent: bool) -> Result<bool> {
            let current = self.0.lock().await;
            Ok(if absent {
                current.is_none()
            } else {
                current.as_ref() == Some(flow)
            })
        }
        async fn exchange(
            &self,
            _bridge: &str,
            flow: &FlowEntry,
            expected: Option<&FlowEntry>,
            remove: bool,
        ) -> Result<()> {
            let mut current = self.0.lock().await;
            ensure!(current.as_ref() == expected, "switch conflict");
            *current = if remove { None } else { Some(flow.clone()) };
            Ok(())
        }
    }
    fn input() -> Value {
        serde_json::json!({"bridge":"fixture0","expected_revision":0,"ttl":64,"flow":{"table":7,"priority":123,"match_fields":{"ip":"","in_port":"3"},"actions":[{"type":"output","port":"4"}]}})
    }

    #[tokio::test]
    async fn restart_recovers_confirmed_effect_from_durable_intent() {
        let owner = Arc::new(Switch::default());
        let documents = Arc::new(RuntimeDocuments::new(
            op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
        ));
        let request = input();
        let record = normalize(serde_json::from_value(request.clone()).unwrap()).unwrap();
        let flow_id = flow_identity(&record.bridge, &record.flow).unwrap();
        let pending = PendingFlow {
            method: "obfuscate_flow".into(),
            arguments: request.clone(),
            flow_id: flow_id.clone(),
            record: record.clone(),
            installed: true,
        };
        let persisted = DurableFlows {
            pending: Some(pending),
            ..Default::default()
        };
        documents
            .update(PLUGIN, move |_| Ok(serde_json::to_value(persisted)?))
            .await
            .unwrap();
        // The process died after the native effect and before its receipt.
        *owner.0.lock().await = Some(record.flow);
        let restarted = ObfuscationMethods::new(documents, owner);
        let (first, state) = restarted
            .dispatch("obfuscate_flow", request.clone())
            .await
            .unwrap();
        assert_eq!(state.revision, 1);
        assert!(state.flows.contains_key(&flow_id));
        let (retry, again) = restarted.dispatch("obfuscate_flow", request).await.unwrap();
        assert_eq!(first, retry);
        assert_eq!(again.revision, 1);
    }

    #[tokio::test]
    async fn normalize_remove_and_reject_external_drift() {
        let owner = Arc::new(Switch::default());
        let docs = Arc::new(RuntimeDocuments::new(
            op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
        ));
        let service = ObfuscationMethods::new(docs, owner.clone());
        let (reply, state) = service.dispatch("obfuscate_flow", input()).await.unwrap();
        let flow_id = reply["flow_id"].as_str().unwrap();
        let installed = owner.0.lock().await.clone().unwrap();
        assert!(
            matches!(&installed.actions[0], FlowAction::SetField { field, value } if field == "nw_ttl" && value == "64")
        );
        assert_eq!(state.flows[flow_id].flow, installed);
        let removal =
            serde_json::json!({"bridge":"fixture0","flow_id":flow_id,"expected_revision":1});
        owner.0.lock().await.as_mut().unwrap().priority = 999;
        assert!(service
            .dispatch("deobfuscate_flow", removal.clone())
            .await
            .is_err());
        *owner.0.lock().await = Some(installed);
        let (reply, state) = service.dispatch("deobfuscate_flow", removal).await.unwrap();
        assert_eq!(reply["installed"], false);
        assert!(state.flows.is_empty());
        assert!(owner.0.lock().await.is_none());
    }
}
