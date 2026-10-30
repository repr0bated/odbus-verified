pub async fn profile_collect(timing: std::path::PathBuf, vectors: std::path::PathBuf) -> anyhow::Result<(Vec<ChainBlockRef>, usize)> {
    let root = timing.parent().unwrap().to_path_buf();
    let chain = StreamingSnowball {
        base_path: root.clone(), timing_subvol: timing, vector_subvol: vectors,
        state_subvol: root.join("state"), snapshot_interval: SnapshotInterval::from_env(),
        retention_policy: Arc::new(RwLock::new(RetentionPolicy::minimal())),
        snapshot_lock: Arc::new(Mutex::new(())), writer_lock_path: root.join(".writer.lock"),
        last_snapshot_time: Arc::new(RwLock::new(Instant::now())),
        block_counter: Arc::new(RwLock::new(0)),
        writer: SnowballWriter::spawn("profile-writer".into(), None)?,
    };
    PROFILE_BODY
}
