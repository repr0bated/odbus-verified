use super::*;
use op_core::projection_shm::{read_projection_snapshot_in, update_projection_with_credentials_in};
use std::{os::unix::fs::MetadataExt, sync::mpsc, time::Duration};

fn bytes(session: &str) -> (Vec<u8>, Vec<u8>) {
    (
        serde_json::to_vec(&serde_json::json!({"sleds":[{"session_id":session}]})).unwrap(),
        serde_json::to_vec(&serde_json::json!({"sleds":[{"session_id":session,"sealed_id":format!("sid1:{session}")}]})).unwrap(),
    )
}

fn commit(directory: &tempfile::TempDir, session: &str) {
    update_projection_with_credentials_in(
        directory.path().to_str().unwrap(),
        "identity_sled",
        directory.path().metadata().unwrap().gid(),
        |_| Ok(Some(bytes(session))),
    )
    .unwrap();
}

#[test]
fn interrupted_identity_pair_is_rejected_until_the_prior_commit_is_restored() {
    let directory = tempfile::tempdir().unwrap();
    let state_dir = directory.path().to_str().unwrap();
    commit(&directory, "committed");
    let manifest = directory.path().join(".manifest.json");
    let old_manifest = std::fs::read(&manifest).unwrap();
    let public = directory.path().join("identity_sled.json");
    let private = directory.path().join(".credentials/identity_sled.json");
    // Simulate a publisher interrupted after changing both files and before
    // completing the manifest. The core's publication tests exercise actual
    // rollback failures; this fixture proves identity consumers honor that
    // commit protocol and cannot inject the uncommitted credential.
    std::fs::write(&manifest, br#"{"generation":1,"publishing":true}"#).unwrap();
    let (pending_public, pending_private) = bytes("uncommitted");
    std::fs::write(&public, pending_public).unwrap();
    std::fs::write(&private, pending_private).unwrap();
    for credentials in [false, true] {
        let error = read_identity_sessions_in(state_dir, credentials).unwrap_err();
        assert!(error.to_string().contains("incomplete"));
    }
    let (old_public, old_private) = bytes("committed");
    std::fs::write(&public, old_public).unwrap();
    std::fs::write(&private, old_private).unwrap();
    std::fs::write(&manifest, old_manifest).unwrap();
    let read = read_identity_sessions_in(state_dir, true).unwrap();
    assert_eq!(read[0].sealed_id.as_deref(), Some("sid1:committed"));
    assert!(
        read_identity_sessions_in(state_dir, false).unwrap()[0]
            .sealed_id
            .is_none()
    );
    assert_eq!(
        read_projection_snapshot_in(state_dir, &["identity_sled".into()])
            .unwrap()
            .generation,
        1
    );
}

#[test]
fn credential_reader_waits_for_the_locked_identity_fold_to_commit() {
    let directory = tempfile::tempdir().unwrap();
    commit(&directory, "before");
    let state_dir = directory.path().to_str().unwrap().to_owned();
    let gid = directory.path().metadata().unwrap().gid();
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let writer_dir = state_dir.clone();
    let writer = std::thread::spawn(move || {
        update_projection_with_credentials_in(&writer_dir, "identity_sled", gid, |_| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(Some(bytes("after")))
        })
        .unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let (read_tx, read_rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        read_tx
            .send(read_identity_sessions_in(&state_dir, true))
            .unwrap();
    });
    assert!(read_rx.recv_timeout(Duration::from_millis(30)).is_err());
    release_tx.send(()).unwrap();
    let result = read_rx
        .recv_timeout(Duration::from_secs(3))
        .unwrap()
        .unwrap();
    assert_eq!(result[0].sealed_id.as_deref(), Some("sid1:after"));
    reader.join().unwrap();
    writer.join().unwrap();
}

#[test]
fn missing_or_invalid_private_projection_never_falls_back_to_public_credentials() {
    let directory = tempfile::tempdir().unwrap();
    let state_dir = directory.path().to_str().unwrap();
    commit(&directory, "committed");
    let public = directory.path().join("identity_sled.json");
    let private = directory.path().join(".credentials/identity_sled.json");
    std::fs::write(public, bytes("public_must_not_be_adopted").1).unwrap();
    std::fs::remove_file(&private).unwrap();
    assert!(matches!(
        read_identity_sessions_in(state_dir, true),
        Err(SessionProjectionError::Unavailable { .. })
    ));
    std::fs::write(private, b"invalid-json").unwrap();
    assert!(read_identity_sessions_in(state_dir, true).is_err());
}
