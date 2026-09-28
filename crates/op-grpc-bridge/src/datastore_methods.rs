//! Datastore method adapter over StateSync's committed public snapshot reader.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use op_plugins::state_plugins::datastore::{
    DataStoreObject, DataStoreObjectRef, ExportOutput, GetObjectInput, GetObjectOutput,
    GetStatsInput, GetStatsOutput, ListObjectsOutput, MAX_DATASTORE_RESPONSE_BYTES,
    MAX_DATASTORE_SELECTION, PluginSelection, PublicObjectExposure, QueryInput, QueryOutput,
    SnapshotBinding,
};

#[cfg(test)]
use crate::state_stream::PluginSnapshot;
use crate::state_stream::{
    StateSnapshot, read_public_state_snapshot, read_public_state_snapshot_in,
};

#[async_trait]
pub(crate) trait DatastoreSnapshotReader: Send + Sync {
    async fn read(&self, plugin_ids: &[String]) -> Result<StateSnapshot>;
}

pub(crate) struct CommittedPublicStateReader {
    fixture_paths: Option<(PathBuf, String)>,
}

impl CommittedPublicStateReader {
    pub(crate) fn production() -> Self {
        Self {
            fixture_paths: None,
        }
    }

    #[cfg(test)]
    fn in_dirs(catalog_dir: PathBuf, state_dir: String) -> Self {
        Self {
            fixture_paths: Some((catalog_dir, state_dir)),
        }
    }
}

#[async_trait]
impl DatastoreSnapshotReader for CommittedPublicStateReader {
    async fn read(&self, plugin_ids: &[String]) -> Result<StateSnapshot> {
        match &self.fixture_paths {
            Some((catalog, state)) => {
                read_public_state_snapshot_in(
                    catalog.clone(),
                    state.clone(),
                    plugin_ids.to_vec(),
                    false,
                )
                .await
            }
            None => read_public_state_snapshot(plugin_ids).await,
        }
    }
}

pub(crate) struct DatastoreMethods {
    reader: Arc<dyn DatastoreSnapshotReader>,
}

impl DatastoreMethods {
    pub(crate) fn production() -> Self {
        Self::new(Arc::new(CommittedPublicStateReader::production()))
    }

    pub(crate) fn new(reader: Arc<dyn DatastoreSnapshotReader>) -> Self {
        Self { reader }
    }

    pub(crate) async fn dispatch(
        &self,
        method: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        match method {
            "list_objects" => {
                let input: PluginSelection = serde_json::from_value(args.clone())?;
                validate_selection(&input.plugin_ids)?;
                let snapshot = self.reader.read(&input.plugin_ids).await?;
                let records = snapshot_records(snapshot)?;
                bounded_json(ListObjectsOutput {
                    binding: records.binding,
                    objects: records
                        .objects
                        .into_iter()
                        .map(|object| object.reference)
                        .collect(),
                })
            }
            "get_object" => {
                let input: GetObjectInput = serde_json::from_value(args.clone())?;
                validate_selection(std::slice::from_ref(&input.plugin_id))?;
                let snapshot = self
                    .reader
                    .read(std::slice::from_ref(&input.plugin_id))
                    .await?;
                let mut records = snapshot_records(snapshot)?;
                let object = records
                    .objects
                    .pop()
                    .context("sealed plugin was absent from committed public snapshot")?;
                ensure!(
                    records.objects.is_empty(),
                    "single-plugin read returned multiple objects"
                );
                bounded_json(GetObjectOutput {
                    binding: records.binding,
                    object,
                })
            }
            "query" => {
                let input: QueryInput = serde_json::from_value(args.clone())?;
                validate_selection(&input.plugin_ids)?;
                let snapshot = self.reader.read(&input.plugin_ids).await?;
                let mut records = snapshot_records(snapshot)?;
                if let Some(exposure) = input.exposure {
                    records
                        .objects
                        .retain(|object| object.reference.exposure == exposure);
                }
                bounded_json(QueryOutput {
                    binding: records.binding,
                    results: records.objects,
                })
            }
            "get_stats" => {
                let _: GetStatsInput = serde_json::from_value(args.clone())?;
                let records = snapshot_records(self.reader.read(&[]).await?)?;
                let state = records.state()?;
                bounded_json(GetStatsOutput {
                    binding: records.binding,
                    object_count: state.object_count,
                    present_count: state.present_count,
                    withheld_count: state.withheld_count,
                    unobserved_count: state.unobserved_count,
                    state_bytes: state.state_bytes,
                })
            }
            "export" => {
                let input: PluginSelection = serde_json::from_value(args.clone())?;
                validate_selection(&input.plugin_ids)?;
                let records = snapshot_records(self.reader.read(&input.plugin_ids).await?)?;
                bounded_json(ExportOutput {
                    binding: records.binding,
                    objects: records.objects,
                })
            }
            _ => bail!("unsupported datastore method: {method}"),
        }
    }
}

struct SnapshotRecords {
    binding: SnapshotBinding,
    objects: Vec<DataStoreObject>,
}

impl SnapshotRecords {
    fn state(&self) -> Result<op_plugins::state_plugins::datastore::DataStoreState> {
        let mut present_count = 0;
        let mut withheld_count = 0;
        let mut unobserved_count = 0;
        let mut state_bytes = 0u64;
        for object in &self.objects {
            state_bytes = state_bytes
                .checked_add(object.reference.state_bytes)
                .context("datastore state byte count overflow")?;
            match object.reference.exposure {
                PublicObjectExposure::Present => present_count += 1,
                PublicObjectExposure::Withheld => withheld_count += 1,
                PublicObjectExposure::Unobserved => unobserved_count += 1,
            }
        }
        Ok(op_plugins::state_plugins::datastore::DataStoreState {
            generation: self.binding.generation,
            catalog_hash: self.binding.catalog_hash.clone(),
            object_count: self.objects.len(),
            present_count,
            withheld_count,
            unobserved_count,
            state_bytes,
            objects: self
                .objects
                .iter()
                .map(|object| object.reference.clone())
                .collect(),
        })
    }
}

fn snapshot_records(snapshot: StateSnapshot) -> Result<SnapshotRecords> {
    ensure!(
        snapshot.plugins.len() <= MAX_DATASTORE_SELECTION,
        "datastore snapshot exceeds {MAX_DATASTORE_SELECTION} plugin records"
    );
    let binding = SnapshotBinding {
        generation: snapshot.generation,
        catalog_hash: snapshot.catalog_hash,
    };
    let mut objects = Vec::with_capacity(snapshot.plugins.len());
    for (plugin_id, plugin) in snapshot.plugins {
        let schema: op_state_store::PluginSchema = serde_json::from_value(plugin.schema.clone())
            .with_context(|| format!("sealed schema JSON is not a PluginSchema for {plugin_id}"))?;
        ensure!(
            schema.name == plugin_id,
            "sealed schema owner mismatch for {plugin_id}"
        );
        schema
            .validate_public_state_policy()
            .with_context(|| format!("invalid public-state policy for {plugin_id}"))?;
        let exposure = match (&plugin.state, schema.public_state.paths.is_empty()) {
            (Some(_), false) => PublicObjectExposure::Present,
            (None, true) => PublicObjectExposure::Withheld,
            (None, false) => PublicObjectExposure::Unobserved,
            (Some(_), true) => bail!("withheld plugin unexpectedly returned public state"),
        };
        let state_bytes = plugin
            .state
            .as_ref()
            .map(serde_json::to_vec)
            .transpose()?
            .map_or(0, |bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        objects.push(DataStoreObject {
            reference: DataStoreObjectRef {
                object_path: op_plugins::canonical::plugin_path(&plugin_id),
                plugin_id,
                schema_hash: plugin.schema_hash,
                exposure,
                state_bytes,
            },
            schema: plugin.schema,
            state: plugin.state,
        });
    }
    Ok(SnapshotRecords { binding, objects })
}

fn validate_selection(plugin_ids: &[String]) -> Result<()> {
    ensure!(
        plugin_ids.len() <= MAX_DATASTORE_SELECTION,
        "datastore selection exceeds {MAX_DATASTORE_SELECTION} plugin ids"
    );
    let mut unique = BTreeSet::new();
    for plugin_id in plugin_ids {
        ensure!(
            !plugin_id.is_empty() && plugin_id.len() <= 128,
            "plugin id length must be between 1 and 128 bytes"
        );
        ensure!(
            crate::sealed_schema_reader::valid_plugin_id(plugin_id),
            "invalid plugin id: {plugin_id}"
        );
        ensure!(unique.insert(plugin_id), "duplicate plugin id: {plugin_id}");
    }
    Ok(())
}

fn bounded_json<T: serde::Serialize>(value: T) -> Result<serde_json::Value> {
    let value = serde_json::to_value(value)?;
    ensure!(
        serde_json::to_vec(&value)?.len() <= MAX_DATASTORE_RESPONSE_BYTES,
        "datastore response exceeds {MAX_DATASTORE_RESPONSE_BYTES} bytes"
    );
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_state::StatePlugin;

    struct Fixture {
        _root: tempfile::TempDir,
        catalog: PathBuf,
        state_dir: String,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let catalog = root.path().join("blobs");
            let state_dir = root.path().join("state").to_str().unwrap().to_string();
            let child = op_state_store::PluginSchema::builder("child")
                .string_field("status", false, "status")
                .string_field("hidden", false, "hidden")
                .build();
            let present = op_state_store::PluginSchema::builder("present")
                .object_field("child", child.fields, false, "child")
                .string_field("secret", false, "secret")
                .public_state_path("/child/status")
                .build();
            let withheld = op_state_store::PluginSchema::builder("withheld")
                .string_field("secret", false, "secret")
                .build();
            let unobserved = op_state_store::PluginSchema::builder("unobserved")
                .string_field("status", false, "status")
                .public_state_path("/status")
                .build();
            let mut blobs = op_blob::BlobStore::open(&catalog).unwrap();
            for schema in [present, withheld, unobserved] {
                blobs
                    .write(&op_blob::blobify_plugin_schema(
                        &schema.name.clone(),
                        schema,
                    ))
                    .unwrap();
            }
            op_core::projection_shm::update_projection_in(&state_dir, "present", |_| {
                Ok(Some(
                    br#"{"child":{"status":"ready","hidden":"private"},"secret":"private"}"#
                        .to_vec(),
                ))
            })
            .unwrap();
            op_core::projection_shm::update_projection_in(&state_dir, "withheld", |_| {
                Ok(Some(br#"{"secret":"private"}"#.to_vec()))
            })
            .unwrap();
            Self {
                _root: root,
                catalog,
                state_dir,
            }
        }

        fn methods(&self) -> DatastoreMethods {
            DatastoreMethods::new(Arc::new(CommittedPublicStateReader::in_dirs(
                self.catalog.clone(),
                self.state_dir.clone(),
            )))
        }
    }

    fn assert_output_matches_contract(method: &str, output: &serde_json::Value) {
        let schema = op_plugins::state_plugins::datastore::DataStorePlugin::new()
            .schema()
            .unwrap();
        let returns =
            serde_json::to_value(schema.methods[method].returns.as_ref().unwrap()).unwrap();
        jsonschema::Validator::new(&returns)
            .unwrap()
            .validate(output)
            .unwrap_or_else(|error| {
                panic!("{method} output violates its sealed contract: {error}")
            });
    }

    #[tokio::test]
    async fn all_methods_read_one_real_committed_public_snapshot() {
        let fixture = Fixture::new();
        let methods = fixture.methods();

        let listed = methods
            .dispatch("list_objects", &serde_json::json!({}))
            .await
            .unwrap();
        assert_output_matches_contract("list_objects", &listed);
        assert_eq!(listed["objects"].as_array().unwrap().len(), 3);
        assert_eq!(listed["objects"][0]["plugin_id"], "present");
        assert_eq!(listed["objects"][0]["exposure"], "present");
        assert_eq!(listed["objects"][1]["plugin_id"], "unobserved");
        assert_eq!(listed["objects"][1]["exposure"], "unobserved");
        assert_eq!(listed["objects"][2]["plugin_id"], "withheld");
        assert_eq!(listed["objects"][2]["exposure"], "withheld");

        let object = methods
            .dispatch("get_object", &serde_json::json!({"plugin_id":"present"}))
            .await
            .unwrap();
        assert_output_matches_contract("get_object", &object);
        assert_eq!(
            object["object"]["state"],
            serde_json::json!({"child":{"status":"ready"}})
        );
        assert!(object["object"]["state"].get("secret").is_none());
        assert!(object["object"]["state"]["child"].get("hidden").is_none());

        let queried = methods
            .dispatch("query", &serde_json::json!({"exposure":"withheld"}))
            .await
            .unwrap();
        assert_output_matches_contract("query", &queried);
        assert_eq!(queried["results"].as_array().unwrap().len(), 1);
        assert!(queried["results"][0]["state"].is_null());

        let stats = methods
            .dispatch("get_stats", &serde_json::json!({}))
            .await
            .unwrap();
        assert_output_matches_contract("get_stats", &stats);
        assert_eq!(stats["present_count"], 1);
        assert_eq!(stats["withheld_count"], 1);
        assert_eq!(stats["unobserved_count"], 1);
        assert!(stats["state_bytes"].as_u64().unwrap() > 0);

        let exported = methods
            .dispatch("export", &serde_json::json!({"plugin_ids":["present"]}))
            .await
            .unwrap();
        assert_output_matches_contract("export", &exported);
        assert_eq!(exported["objects"].as_array().unwrap().len(), 1);
        assert_eq!(
            exported["binding"]["generation"],
            object["binding"]["generation"]
        );
    }

    #[tokio::test]
    async fn reseal_changes_catalog_binding_and_new_membership() {
        let fixture = Fixture::new();
        let methods = fixture.methods();
        let before = methods
            .dispatch("list_objects", &serde_json::json!({}))
            .await
            .unwrap();

        let added = op_state_store::PluginSchema::builder("added")
            .string_field("status", false, "status")
            .public_state_path("/status")
            .build();
        op_blob::BlobStore::open(&fixture.catalog)
            .unwrap()
            .write(&op_blob::blobify_plugin_schema("added", added))
            .unwrap();
        let after = methods
            .dispatch("list_objects", &serde_json::json!({}))
            .await
            .unwrap();
        assert_ne!(
            before["binding"]["catalog_hash"],
            after["binding"]["catalog_hash"]
        );
        assert_eq!(after["objects"].as_array().unwrap().len(), 4);
    }

    #[tokio::test]
    async fn unavailable_or_interrupted_backing_fails_closed() {
        let missing = tempfile::tempdir().unwrap();
        let reader = CommittedPublicStateReader::in_dirs(
            missing.path().join("absent"),
            missing.path().join("state").to_str().unwrap().to_string(),
        );
        assert!(reader.read(&[]).await.is_err());

        let fixture = Fixture::new();
        std::fs::write(
            std::path::Path::new(&fixture.state_dir).join(".manifest.json"),
            br#"{"generation":99,"publishing":true}"#,
        )
        .unwrap();
        assert!(
            fixture
                .methods()
                .dispatch("get_stats", &serde_json::json!({}))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn invalid_or_unbounded_requests_fail_before_read() {
        let fixture = Fixture::new();
        let methods = fixture.methods();
        assert!(
            methods
                .dispatch(
                    "list_objects",
                    &serde_json::json!({"plugin_ids":["present","present"]}),
                )
                .await
                .is_err()
        );
        assert!(
            methods
                .dispatch("query", &serde_json::json!({"raw_query":"?[x] := *x{x}"}))
                .await
                .is_err()
        );
        assert!(
            methods
                .dispatch("get_object", &serde_json::json!({"plugin_id":"../private"}))
                .await
                .is_err()
        );
        assert!(
            methods
                .dispatch(
                    "get_object",
                    &serde_json::json!({"plugin_id":"x".repeat(129)}),
                )
                .await
                .is_err()
        );

        let plugins = (0..=MAX_DATASTORE_SELECTION)
            .map(|index| {
                (
                    format!("plugin_{index}"),
                    PluginSnapshot {
                        schema_hash: "a".repeat(64),
                        schema: serde_json::json!({}),
                        state: None,
                    },
                )
            })
            .collect();
        assert!(
            snapshot_records(StateSnapshot {
                generation: 1,
                catalog_hash: "b".repeat(64),
                plugins,
            })
            .is_err()
        );
    }
}
