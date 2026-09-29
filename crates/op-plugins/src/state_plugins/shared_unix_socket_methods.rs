//! Durable native adapter for `shared_unix_socket` registration metadata.
//! The bridge injects its listener path; this module performs no socket or
//! filesystem operations.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::runtime_documents::{RuntimeDocument, RuntimeDocuments};
use super::shared_unix_socket::{
    CreateUnixSocketInput, CreateUnixSocketOutput, GetConfigOutput, ListRegistrationsOutput,
    SharedRegistration, SharedUnixSocketConfig, SharedUnixSocketState,
};

const DOCUMENT_ID: &str = "shared_unix_socket";
const LISTENER_OWNER: &str = "op-grpc-bridge";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SharedUnixSocketDocument {
    registrations: BTreeMap<String, SharedRegistration>,
}

pub struct SharedUnixSocketMethods {
    documents: Arc<RuntimeDocuments>,
    shared_socket: String,
}

impl SharedUnixSocketMethods {
    /// The path was selected by bridge startup. Validation here is data-only.
    pub fn new(documents: Arc<RuntimeDocuments>, shared_socket: impl AsRef<Path>) -> Result<Self> {
        let shared_socket = shared_socket.as_ref();
        ensure!(
            shared_socket.is_absolute(),
            "shared socket path must be absolute"
        );
        let shared_socket = shared_socket
            .to_str()
            .context("shared socket path must be valid UTF-8")?
            .to_string();
        Ok(Self {
            documents,
            shared_socket,
        })
    }

    pub async fn dispatch(&self, method: &str, args: &Value) -> Result<Value> {
        match method {
            "create_unix_socket" => {
                self.create_unix_socket(serde_json::from_value(args.clone())?)
                    .await
            }
            "list_registrations" => {
                validate_empty_input(args)?;
                self.list_registrations().await
            }
            "get_config" => {
                validate_empty_input(args)?;
                self.get_config().await
            }
            _ => bail!("unsupported shared_unix_socket method"),
        }
    }

    async fn create_unix_socket(&self, input: CreateUnixSocketInput) -> Result<Value> {
        validate_name(&input.name)?;
        let mut ports = input.ports;
        ensure!(!ports.is_empty(), "ports must not be empty");
        ensure!(
            ports.iter().all(|port| *port != 0),
            "ports must be non-zero"
        );
        ports.sort_unstable();
        ports.dedup();

        let name = input.name;
        let now = chrono::Utc::now().to_rfc3339();
        let new_id = uuid::Uuid::new_v4().to_string();
        let (row, (registration, created, changed)) = self
            .documents
            .update_with_result(DOCUMENT_ID, move |old| {
                let mut document = parse_optional_document(old)?;
                let receipt = if let Some(existing) = document.registrations.get_mut(&name) {
                    let changed = existing.ports != ports || existing.protocol != "grpc";
                    if changed {
                        existing.ports = ports;
                        existing.protocol = "grpc".to_string();
                        existing.updated_at = now;
                    }
                    (existing.clone(), false, changed)
                } else {
                    let registration = SharedRegistration {
                        id: new_id,
                        name: name.clone(),
                        ports,
                        protocol: "grpc".to_string(),
                        created_at: now.clone(),
                        updated_at: now,
                    };
                    document.registrations.insert(name, registration.clone());
                    (registration, true, true)
                };
                Ok((serde_json::to_value(document)?, receipt))
            })
            .await?;

        Ok(serde_json::to_value(CreateUnixSocketOutput {
            registration,
            created,
            changed,
            revision: row.revision,
        })?)
    }

    async fn list_registrations(&self) -> Result<Value> {
        let (document, revision) = self.read_document().await?;
        Ok(serde_json::to_value(ListRegistrationsOutput {
            registrations: document.registrations.into_values().collect(),
            revision,
        })?)
    }

    async fn get_config(&self) -> Result<Value> {
        let (document, revision) = self.read_document().await?;
        Ok(serde_json::to_value(GetConfigOutput {
            config: SharedUnixSocketConfig {
                shared_socket: self.shared_socket.clone(),
                listener_owner: LISTENER_OWNER.to_string(),
                registration_count: u64::try_from(document.registrations.len())?,
                revision,
            },
        })?)
    }

    async fn read_document(&self) -> Result<(SharedUnixSocketDocument, u64)> {
        match self.documents.read(DOCUMENT_ID).await? {
            Some(row) => Ok((parse_document(row.document)?, row.revision)),
            None => Ok((SharedUnixSocketDocument::default(), 0)),
        }
    }
}

fn parse_optional_document(value: Option<Value>) -> Result<SharedUnixSocketDocument> {
    value
        .map(parse_document)
        .transpose()
        .map(Option::unwrap_or_default)
}

fn parse_document(value: Value) -> Result<SharedUnixSocketDocument> {
    let document: SharedUnixSocketDocument =
        serde_json::from_value(value).context("invalid shared Unix-socket runtime document")?;
    validate_document(&document)?;
    Ok(document)
}

fn validate_document(document: &SharedUnixSocketDocument) -> Result<()> {
    let mut ids = BTreeSet::new();
    for (key, registration) in &document.registrations {
        validate_name(&registration.name)?;
        ensure!(key == &registration.name, "registration key/name mismatch");
        ensure!(
            uuid::Uuid::parse_str(&registration.id).is_ok(),
            "registration id is not a UUID"
        );
        ensure!(ids.insert(&registration.id), "duplicate registration id");
        ensure!(registration.protocol == "grpc", "unsupported protocol");
        ensure!(!registration.ports.is_empty(), "ports must not be empty");
        ensure!(
            registration.ports.iter().all(|port| *port != 0),
            "ports must be non-zero"
        );
        ensure!(
            registration.ports.windows(2).all(|pair| pair[0] < pair[1]),
            "ports must be sorted and de-duplicated"
        );
        let created = chrono::DateTime::parse_from_rfc3339(&registration.created_at)
            .context("invalid registration created_at")?;
        let updated = chrono::DateTime::parse_from_rfc3339(&registration.updated_at)
            .context("invalid registration updated_at")?;
        ensure!(
            updated >= created,
            "registration updated_at predates created_at"
        );
    }
    Ok(())
}

/// Convert an owned durable row into startup registration state. Validation is
/// identical to the live method path, so corrupt rows cannot enter the bridge's
/// restoration pipeline.
pub fn restored_shared_registration_state(row: RuntimeDocument) -> Result<SharedUnixSocketState> {
    let document = parse_document(row.document)?;
    Ok(SharedUnixSocketState {
        registrations: document.registrations.into_values().collect(),
    })
}

fn validate_name(name: &str) -> Result<()> {
    ensure!(!name.is_empty(), "name must not be empty");
    ensure!(name.len() <= 256, "name is too long");
    ensure!(
        name.trim() == name,
        "name must not have surrounding whitespace"
    );
    ensure!(
        !name.chars().any(char::is_control),
        "name must not contain control characters"
    );
    Ok(())
}

fn validate_empty_input(args: &Value) -> Result<()> {
    ensure!(
        args.as_object().is_some_and(serde_json::Map::is_empty),
        "method takes no arguments"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_cozo_store::CozoGraphShuttle;
    use serde_json::json;

    fn fixture(path: &str) -> SharedUnixSocketMethods {
        SharedUnixSocketMethods::new(
            Arc::new(RuntimeDocuments::new(
                CozoGraphShuttle::new_in_memory().unwrap(),
            )),
            path,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn all_methods_return_durable_actual_metadata() {
        let service = fixture("/fixture/bridge-owned.sock");
        let created = service
            .dispatch(
                "create_unix_socket",
                &json!({"name":"qdrant","ports":[6334,6333,6333]}),
            )
            .await
            .unwrap();
        assert_eq!(created["created"], true);
        assert_eq!(created["registration"]["ports"], json!([6333, 6334]));
        let id = created["registration"]["id"].as_str().unwrap().to_string();

        let repeated = service
            .dispatch(
                "create_unix_socket",
                &json!({"name":"qdrant","ports":[6333,6334]}),
            )
            .await
            .unwrap();
        assert_eq!(repeated["created"], false);
        assert_eq!(repeated["changed"], false);
        assert_eq!(repeated["registration"]["id"], id);

        let updated = service
            .dispatch(
                "create_unix_socket",
                &json!({"name":"qdrant","ports":[6335]}),
            )
            .await
            .unwrap();
        assert_eq!(updated["created"], false);
        assert_eq!(updated["changed"], true);
        assert_eq!(updated["registration"]["id"], id);

        let listed = service
            .dispatch("list_registrations", &json!({}))
            .await
            .unwrap();
        assert_eq!(listed["registrations"].as_array().unwrap().len(), 1);
        assert_eq!(listed["registrations"][0]["ports"], json!([6335]));
        assert_eq!(listed["revision"], 3);

        let config = service.dispatch("get_config", &json!({})).await.unwrap();
        assert_eq!(
            config["config"]["shared_socket"],
            "/fixture/bridge-owned.sock"
        );
        assert_eq!(config["config"]["listener_owner"], LISTENER_OWNER);
        assert_eq!(config["config"]["registration_count"], 1);
        assert_eq!(config["config"]["revision"], 3);

        let row = service.documents.read(DOCUMENT_ID).await.unwrap().unwrap();
        let restored = restored_shared_registration_state(row).unwrap();
        assert_eq!(restored.registrations.len(), 1);
        assert_eq!(restored.registrations[0].id, id);
    }

    #[tokio::test]
    async fn invalid_inputs_do_not_commit() {
        assert!(SharedUnixSocketMethods::new(
            Arc::new(RuntimeDocuments::new(
                CozoGraphShuttle::new_in_memory().unwrap(),
            )),
            "relative.sock",
        )
        .is_err());
        let service = fixture("/fixture/bridge-owned.sock");
        for args in [
            json!({"name":"","ports":[1]}),
            json!({"name":" qdrant","ports":[1]}),
            json!({"name":"qdrant","ports":[]}),
            json!({"name":"qdrant","ports":[0]}),
        ] {
            assert!(service.dispatch("create_unix_socket", &args).await.is_err());
        }
        assert!(service
            .dispatch("list_registrations", &json!({"unexpected":true}))
            .await
            .is_err());
        let listed = service
            .dispatch("list_registrations", &json!({}))
            .await
            .unwrap();
        assert_eq!(listed["revision"], 0);
        assert!(listed["registrations"].as_array().unwrap().is_empty());

        let invalid = RuntimeDocument {
            revision: 1,
            document: json!({"registrations":{"wrong-key":{
                "id":uuid::Uuid::new_v4().to_string(),
                "name":"other-name","ports":[2,1],"protocol":"grpc",
                "created_at":"2026-09-13T00:00:00Z","updated_at":"2026-09-13T00:00:00Z"
            }}}),
        };
        assert!(restored_shared_registration_state(invalid).is_err());
    }
}
