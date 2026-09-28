//! Typed canonical-method adapter over the bridge-owned Snowball chain.

use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use op_snowball::{RetentionPolicy, StreamingSnowball};
use serde_json::Value;
use tokio::sync::Mutex;

use super::snowball_plugin::{
    CreateSnapshotOutput, GetCurrentStateOutput, GetSnapshotOutput, ListSnapshotsOutput,
    RetentionView, RollbackOutput, SetRetentionInput, SetRetentionOutput, SnapshotInput,
    SnapshotRecord, SnowballEmptyInput, SnowballStatsOutput,
};

const MAX_RETENTION_PER_BUCKET: u32 = 10_000;

pub struct SnowballMethods {
    chain: Arc<StreamingSnowball>,
    canonical_operation: Mutex<()>,
}

impl SnowballMethods {
    pub fn from_shared(chain: Arc<StreamingSnowball>) -> Self {
        Self {
            chain,
            canonical_operation: Mutex::new(()),
        }
    }

    pub async fn dispatch(&self, method: &str, args: &Value) -> Result<Value> {
        match method {
            "create_snapshot" => {
                let _: SnowballEmptyInput = serde_json::from_value(args.clone())?;
                serde_json::to_value(self.create_snapshot().await?).map_err(Into::into)
            }
            "get_current_state" => {
                let _: SnowballEmptyInput = serde_json::from_value(args.clone())?;
                serde_json::to_value(self.get_current_state().await?).map_err(Into::into)
            }
            "get_snapshot" => {
                let input: SnapshotInput = serde_json::from_value(args.clone())?;
                serde_json::to_value(self.get_snapshot(&input.snapshot_name).await?)
                    .map_err(Into::into)
            }
            "get_stats" => {
                let _: SnowballEmptyInput = serde_json::from_value(args.clone())?;
                serde_json::to_value(self.get_stats().await?).map_err(Into::into)
            }
            "list_snapshots" => {
                let _: SnowballEmptyInput = serde_json::from_value(args.clone())?;
                serde_json::to_value(self.list_snapshots().await?).map_err(Into::into)
            }
            "rollback" => {
                let input: SnapshotInput = serde_json::from_value(args.clone())?;
                serde_json::to_value(self.rollback(&input.snapshot_name).await?).map_err(Into::into)
            }
            "set_retention" => {
                let input: SetRetentionInput = serde_json::from_value(args.clone())?;
                serde_json::to_value(self.set_retention(input).await?).map_err(Into::into)
            }
            _ => bail!("unsupported snowball method: {method}"),
        }
    }

    async fn create_snapshot(&self) -> Result<CreateSnapshotOutput> {
        let _guard = self.canonical_operation.lock().await;
        let name = self.chain.create_snapshot().await?;
        let snapshot = self.snapshot(&name).await?;
        Ok(CreateSnapshotOutput { snapshot })
    }

    async fn get_current_state(&self) -> Result<GetCurrentStateOutput> {
        let state = self.chain.current_state().await?;
        Ok(GetCurrentStateOutput {
            initialized: state.is_some(),
            state: state.map(simd_to_json).transpose()?,
        })
    }

    async fn list_snapshots(&self) -> Result<ListSnapshotsOutput> {
        let snapshots = self.chain.list_snapshots().await?;
        let snapshots = snapshots
            .into_iter()
            .map(|(name, created)| {
                Ok(SnapshotRecord {
                    counter: self.chain.state_snapshot_counter(&name)?,
                    name,
                    created,
                    state: None,
                })
            })
            .collect::<Result<_>>()?;
        Ok(ListSnapshotsOutput { snapshots })
    }

    async fn get_snapshot(&self, name: &str) -> Result<GetSnapshotOutput> {
        Ok(GetSnapshotOutput {
            snapshot: self.snapshot(name).await?,
        })
    }

    async fn snapshot(&self, name: &str) -> Result<SnapshotRecord> {
        let counter = self.chain.state_snapshot_counter(name)?;
        let created = self
            .chain
            .list_snapshots()
            .await?
            .into_iter()
            .find_map(|(candidate, created)| (candidate == name).then_some(created))
            .with_context(|| format!("Snowball snapshot not found: {name}"))?;
        let state = self
            .chain
            .read_snapshot_state(name)
            .await?
            .map(simd_to_json)
            .transpose()?;
        Ok(SnapshotRecord {
            name: name.to_string(),
            counter,
            created,
            state,
        })
    }

    async fn get_stats(&self) -> Result<SnowballStatsOutput> {
        let blocks = self.chain.blocks().await?;
        let snapshots = self.chain.list_snapshots().await?;
        let retention = retention_view(self.chain.retention_policy().await);
        Ok(SnowballStatsOutput {
            readable_block_count: u64::try_from(blocks.len())?,
            vectorized_block_count: u64::try_from(
                blocks.iter().filter(|block| block.has_vector).count(),
            )?,
            snapshot_count: u64::try_from(snapshots.len())?,
            latest_block_number: blocks.last().map(|block| block.block_num),
            retention,
        })
    }

    async fn rollback(&self, name: &str) -> Result<RollbackOutput> {
        let _guard = self.canonical_operation.lock().await;
        self.chain.state_snapshot_counter(name)?;
        let restored_state = self.chain.rollback(name).await?;
        Ok(RollbackOutput {
            snapshot_name: name.to_string(),
            restored_current_state: simd_to_json(restored_state)?,
        })
    }

    async fn set_retention(&self, input: SetRetentionInput) -> Result<SetRetentionOutput> {
        let _guard = self.canonical_operation.lock().await;
        for count in [input.hourly, input.daily, input.weekly, input.quarterly] {
            ensure!(
                count <= MAX_RETENTION_PER_BUCKET,
                "Snowball retention bucket exceeds {MAX_RETENTION_PER_BUCKET}"
            );
        }
        ensure!(
            input.hourly + input.daily + input.weekly + input.quarterly > 0,
            "Snowball retention must keep at least one snapshot"
        );
        let policy = RetentionPolicy::new(
            input.hourly as usize,
            input.daily as usize,
            input.weekly as usize,
            input.quarterly as usize,
        );
        self.chain.set_retention_policy(policy).await?;
        Ok(SetRetentionOutput {
            retention: retention_view(policy),
            remaining_snapshots: u64::try_from(self.chain.list_snapshots().await?.len())?,
        })
    }
}

fn retention_view(policy: RetentionPolicy) -> RetentionView {
    RetentionView {
        hourly: policy.hourly,
        daily: policy.daily,
        weekly: policy.weekly,
        quarterly: policy.quarterly,
    }
}

fn simd_to_json(value: simd_json::OwnedValue) -> Result<Value> {
    serde_json::to_value(value).context("convert Snowball state to transport JSON")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    struct BtrfsFixture(tempfile::TempDir);

    impl BtrfsFixture {
        fn path(&self) -> &Path {
            self.0.path()
        }
    }

    impl Drop for BtrfsFixture {
        fn drop(&mut self) {
            fn collect(path: &Path, subvolumes: &mut Vec<PathBuf>) {
                let Ok(entries) = std::fs::read_dir(path) else {
                    return;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                        continue;
                    };
                    if metadata.is_dir() && !metadata.file_type().is_symlink() {
                        collect(&path, subvolumes);
                        if metadata.ino() == 256 {
                            subvolumes.push(path);
                        }
                    }
                }
            }
            let mut subvolumes = Vec::new();
            collect(self.path(), &mut subvolumes);
            for path in subvolumes {
                // Only test-owned snapshot members change RO state here.
                let _ = Command::new("btrfs")
                    .args(["property", "set", "-ts"])
                    .arg(&path)
                    .args(["ro", "false"])
                    .output();
                let output = Command::new("btrfs")
                    .args(["subvolume", "delete"])
                    .arg(&path)
                    .output();
                if let Ok(output) = output {
                    if !output.status.success() {
                        eprintln!(
                            "test subvolume cleanup failed for {}: {}",
                            path.display(),
                            String::from_utf8_lossy(&output.stderr)
                        );
                    }
                }
            }
        }
    }

    async fn fixture() -> (BtrfsFixture, Arc<StreamingSnowball>, SnowballMethods) {
        let dir = BtrfsFixture(tempfile::tempdir().unwrap());
        let chain = Arc::new(
            StreamingSnowball::new(dir.path().join("snowball"))
                .await
                .unwrap(),
        );
        let methods = SnowballMethods::from_shared(Arc::clone(&chain));
        (dir, chain, methods)
    }

    async fn snapshot_readback(methods: &SnowballMethods, names: &[String]) -> Value {
        let empty = serde_json::json!({});
        let mut records = Vec::new();
        for name in names {
            records.push(
                methods
                    .dispatch("get_snapshot", &serde_json::json!({"snapshot_name": name}))
                    .await
                    .unwrap(),
            );
        }
        serde_json::json!({
            "current": methods.dispatch("get_current_state", &empty).await.unwrap(),
            "list": methods.dispatch("list_snapshots", &empty).await.unwrap(),
            "records": records,
        })
    }

    #[tokio::test]
    async fn failed_unknown_rollback_preserves_state_and_retained_snapshots_after_reopen() {
        let (dir, chain, methods) = fixture().await;
        chain
            .set_retention_policy(RetentionPolicy::new(10, 0, 0, 0))
            .await
            .unwrap();
        let mut names = Vec::new();
        for generation in 1..=2 {
            chain
                .write_current_state(&simd_json::json!({
                    "generation": generation,
                    "unrelated": format!("state-{generation}"),
                }))
                .await
                .unwrap();
            let created = methods
                .dispatch("create_snapshot", &serde_json::json!({}))
                .await
                .unwrap();
            assert_eq!(created["snapshot"]["state"]["generation"], generation);
            names.push(created["snapshot"]["name"].as_str().unwrap().to_owned());
        }
        assert_ne!(names[0], names[1]);
        let before = snapshot_readback(&methods, &names).await;
        assert_eq!(before["current"]["state"]["generation"], 2);
        assert_eq!(before["list"]["snapshots"].as_array().unwrap().len(), 2);
        assert!(
            methods
                .dispatch(
                    "rollback",
                    &serde_json::json!({"snapshot_name": "SNP-999999"})
                )
                .await
                .is_err()
        );
        assert!(
            !tokio::fs::try_exists(dir.path().join("snowball/snapshots/SNP-999999"))
                .await
                .unwrap()
        );
        assert_eq!(snapshot_readback(&methods, &names).await, before);

        drop(methods);
        drop(chain);
        let reopened = Arc::new(
            StreamingSnowball::new(dir.path().join("snowball"))
                .await
                .unwrap(),
        );
        let reopened_methods = SnowballMethods::from_shared(reopened);
        assert_eq!(snapshot_readback(&reopened_methods, &names).await, before);
    }

    #[tokio::test]
    async fn seven_methods_use_persistent_snapshot_state() {
        let (dir, chain, methods) = fixture().await;
        let empty = serde_json::json!({});

        let initial = methods.dispatch("get_current_state", &empty).await.unwrap();
        assert_eq!(initial["initialized"], false);

        chain
            .write_current_state(&simd_json::json!({"generation": 1}))
            .await
            .unwrap();
        let first = methods.dispatch("create_snapshot", &empty).await.unwrap();
        let first_name = first["snapshot"]["name"].as_str().unwrap().to_string();
        assert_eq!(first["snapshot"]["state"]["generation"], 1);

        chain
            .write_current_state(&simd_json::json!({"generation": 2}))
            .await
            .unwrap();
        methods.dispatch("create_snapshot", &empty).await.unwrap();
        let listed = methods.dispatch("list_snapshots", &empty).await.unwrap();
        assert_eq!(listed["snapshots"].as_array().unwrap().len(), 2);

        let fetched = methods
            .dispatch(
                "get_snapshot",
                &serde_json::json!({"snapshot_name": first_name}),
            )
            .await
            .unwrap();
        assert_eq!(fetched["snapshot"]["state"]["generation"], 1);

        let rolled_back = methods
            .dispatch(
                "rollback",
                &serde_json::json!({"snapshot_name": first_name}),
            )
            .await
            .unwrap();
        assert_eq!(rolled_back["restored_current_state"]["generation"], 1);
        let current = methods.dispatch("get_current_state", &empty).await.unwrap();
        assert_eq!(current["state"]["generation"], 1);

        let stats = methods.dispatch("get_stats", &empty).await.unwrap();
        assert_eq!(stats["snapshot_count"], 2);
        assert_eq!(stats["readable_block_count"], 0);

        let retained = methods
            .dispatch(
                "set_retention",
                &serde_json::json!({"hourly":1,"daily":0,"weekly":0,"quarterly":0}),
            )
            .await
            .unwrap();
        assert_eq!(retained["remaining_snapshots"], 1);

        drop(methods);
        drop(chain);
        let reopened = Arc::new(
            StreamingSnowball::new(dir.path().join("snowball"))
                .await
                .unwrap(),
        );
        let reopened_methods = SnowballMethods::from_shared(reopened);
        let reopened_stats = reopened_methods
            .dispatch("get_stats", &empty)
            .await
            .unwrap();
        assert_eq!(reopened_stats["retention"]["hourly"], 1);
        assert_eq!(reopened_stats["snapshot_count"], 1);
    }

    #[tokio::test]
    async fn rejects_missing_invalid_and_unsafe_requests() {
        let (_dir, chain, methods) = fixture().await;
        assert!(
            methods
                .dispatch(
                    "get_snapshot",
                    &serde_json::json!({"snapshot_name":"../state"})
                )
                .await
                .is_err()
        );
        assert!(
            methods
                .dispatch(
                    "rollback",
                    &serde_json::json!({"snapshot_name":"SNP-999999"})
                )
                .await
                .is_err()
        );
        assert!(
            methods
                .dispatch(
                    "set_retention",
                    &serde_json::json!({"hourly":0,"daily":0,"weekly":0,"quarterly":0}),
                )
                .await
                .is_err()
        );
        assert!(
            methods
                .dispatch("get_stats", &serde_json::json!({"unexpected":true}))
                .await
                .is_err()
        );

        let corrupt = chain.base_path().join("snapshots/SNP-000001");
        tokio::fs::create_dir_all(corrupt.join("state"))
            .await
            .unwrap();
        tokio::fs::write(corrupt.join(".committed"), b"")
            .await
            .unwrap();
        tokio::fs::write(corrupt.join("state/current.json"), b"not-json")
            .await
            .unwrap();
        assert!(
            methods
                .dispatch(
                    "rollback",
                    &serde_json::json!({"snapshot_name":"SNP-000001"}),
                )
                .await
                .is_err()
        );
        assert_eq!(chain.current_state().await.unwrap(), None);
    }
}
