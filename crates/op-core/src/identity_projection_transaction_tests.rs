use super::*;
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};

fn root(dir: &tempfile::TempDir) -> &str {
    dir.path().to_str().unwrap()
}
fn gid() -> u32 {
    nix::unistd::getegid().as_raw()
}
fn pair(version: u64) -> Pair {
    Some((
        format!("{{\"version\":{version}}}").into_bytes(),
        format!("{{\"version\":{version},\"sealed_id\":\"private-{version}\"}}").into_bytes(),
    ))
}
fn seed(dir: &tempfile::TempDir) {
    update_projection_with_credentials_in(root(dir), IDENTITY, gid(), |_| Ok(pair(1))).unwrap();
}
fn unavailable(dir: &tempfile::TempDir) {
    assert!(read_credential_in(root(dir), IDENTITY).is_err());
    assert!(read_projection_snapshot_in(root(dir), &[IDENTITY.into()]).is_err());
    assert!(update_projection_in(root(dir), IDENTITY, |_| Ok(None)).is_err());
    assert!(
        update_projection_with_credentials_in(root(dir), IDENTITY, gid(), |_| Ok(None)).is_err()
    );
}
fn committed<T>(result: CredentialPublication<T>) -> ProjectionCommit {
    match result {
        CredentialPublication::Committed { projection, .. } => projection,
        CredentialPublication::Pending { error, .. } => panic!("unexpected pending: {error}"),
    }
}

#[test]
fn durable_callback_runs_after_synced_marker_and_sees_only_private_predecessor() {
    let dir = tempfile::tempdir().unwrap();
    seed(&dir);
    let receipt = committed(
        commit_identity_projection_durable_in(root(&dir), gid(), "op-two", |previous| {
            assert_eq!(
                previous,
                pair(1).as_ref().map(|(_, private)| private.as_slice())
            );
            // Fixture-only direct marker inspection avoids reentering the lock.
            let bytes = std::fs::read(dir.path().join(MANIFEST_NAME)).unwrap();
            let text = String::from_utf8(bytes).unwrap();
            assert!(text.contains("op-two"));
            Ok((pair(2), 2u64))
        })
        .unwrap(),
    );
    assert_eq!(receipt.generation, 2);
    assert_eq!(receipt.current, pair(2).map(|(public, _)| public));
    assert!(!receipt.current.unwrap().windows(7).any(|v| v == b"private"));
    assert!(
        pending_identity_publication_in(root(&dir))
            .unwrap()
            .is_none()
    );
}

#[test]
fn uncertain_callback_blocks_identity_but_preserves_unrelated_transactions() {
    let dir = tempfile::tempdir().unwrap();
    seed(&dir);
    let result =
        commit_identity_projection_durable_in::<()>(root(&dir), gid(), "uncertain", |_| {
            bail!("native commit outcome uncertain")
        })
        .unwrap();
    assert!(matches!(
        result,
        CredentialPublication::Pending { durable: None, .. }
    ));
    unavailable(&dir);
    let other = update_projection_in(root(&dir), "other", |_| Ok(Some(b"{}".to_vec()))).unwrap();
    assert_eq!(other.generation, 2);
    assert_eq!(
        read_projection_snapshot_in(root(&dir), &["other".into()])
            .unwrap()
            .states["other"],
        b"{}"
    );
    assert_eq!(
        pending_identity_publication_in(root(&dir))
            .unwrap()
            .unwrap()
            .operation_id,
        "uncertain"
    );
    unavailable(&dir);
    let called = AtomicUsize::new(0);
    assert!(
        recover_identity_projection_in(root(&dir), gid(), "wrong", || {
            called.fetch_add(1, Ordering::SeqCst);
            Ok((pair(9), ()))
        })
        .is_err()
    );
    assert_eq!(called.load(Ordering::SeqCst), 0);
    let receipt = committed(
        recover_identity_projection_in(root(&dir), gid(), "uncertain", || Ok((pair(3), ())))
            .unwrap(),
    );
    assert_eq!(receipt.generation, 3);
    assert!(
        receipt.previous.is_none(),
        "recovery never certifies a potentially mixed old public file"
    );
}

#[test]
fn every_post_durable_file_failure_keeps_gate_until_exact_durable_recovery() {
    for (name, occurrence) in [(IDENTITY_FILE, 1), (IDENTITY_FILE, 2), (MANIFEST_NAME, 2)] {
        let dir = tempfile::tempdir().unwrap();
        seed(&dir);
        super::super::tests::inject(name, occurrence, false);
        let result =
            commit_identity_projection_durable_in(root(&dir), gid(), "committed-db", |_| {
                Ok((pair(2), 42u64))
            })
            .unwrap();
        assert!(matches!(
            result,
            CredentialPublication::Pending {
                durable: Some(42),
                ..
            }
        ));
        unavailable(&dir);
        let receipt = committed(
            recover_identity_projection_in(root(&dir), gid(), "committed-db", || {
                Ok((pair(2), 42u64))
            })
            .unwrap(),
        );
        assert_eq!(receipt.current, pair(2).map(|(public, _)| public));
        assert_eq!(
            read_credential_in(root(&dir), IDENTITY).unwrap(),
            pair(2).map(|(_, private)| private)
        );
    }
}

#[test]
fn pre_intent_failure_and_generation_overflow_never_call_durable_writer() {
    let dir = tempfile::tempdir().unwrap();
    seed(&dir);
    let called = AtomicUsize::new(0);
    super::super::tests::inject(MANIFEST_NAME, 1, false);
    assert!(
        commit_identity_projection_durable_in(root(&dir), gid(), "not-called", |_| {
            called.fetch_add(1, Ordering::SeqCst);
            Ok((pair(2), ()))
        })
        .is_err()
    );
    assert_eq!(called.load(Ordering::SeqCst), 0);
    assert_eq!(
        read_credential_in(root(&dir), IDENTITY).unwrap(),
        pair(1).map(|(_, v)| v)
    );
    std::fs::write(
        dir.path().join(MANIFEST_NAME),
        format!("{{\"generation\":{},\"publishing\":false}}", u64::MAX),
    )
    .unwrap();
    assert!(
        commit_identity_projection_durable_in(root(&dir), gid(), "overflow", |_| {
            called.fetch_add(1, Ordering::SeqCst);
            Ok((pair(2), ()))
        })
        .is_err()
    );
    assert_eq!(called.load(Ordering::SeqCst), 0);
}

#[test]
fn directory_sync_failures_never_clear_pending_or_expose_old_credentials() {
    for phase in [
        "identity_marker_sync",
        "identity_private_sync",
        "identity_public_sync",
    ] {
        let dir = tempfile::tempdir().unwrap();
        seed(&dir);
        let calls = AtomicUsize::new(0);
        super::super::tests::inject(phase, 1, false);
        let result =
            commit_identity_projection_durable_in(root(&dir), gid(), "sync-failed", |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok((pair(2), 2u64))
            })
            .unwrap();
        let expected_calls = usize::from(phase != "identity_marker_sync");
        assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
        match result {
            CredentialPublication::Pending {
                durable,
                operation_id,
                ..
            } => {
                assert_eq!(durable, (expected_calls == 1).then_some(2));
                assert_eq!(operation_id, "sync-failed");
            }
            CredentialPublication::Committed { .. } => panic!("sync failure cannot commit"),
        }
        unavailable(&dir);
        // Recovery's fixture emulates exact durable reread: unchanged on marker
        // sync failure (the callback never ran), otherwise the new committed row.
        let version = if expected_calls == 0 { 1 } else { 2 };
        committed(
            recover_identity_projection_in(root(&dir), gid(), "sync-failed", || {
                Ok((pair(version), ()))
            })
            .unwrap(),
        );
        assert_eq!(
            read_credential_in(root(&dir), IDENTITY).unwrap(),
            pair(version).map(|(_, v)| v)
        );
    }
}

#[test]
fn panic_never_rolls_back_to_old_readable_credential_and_recovery_can_delete() {
    let dir = tempfile::tempdir().unwrap();
    seed(&dir);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        commit_identity_projection_durable_in::<()>(root(&dir), gid(), "panic", |_| {
            panic!("simulated cancellation/process unwind")
        })
    }));
    assert!(result.is_err());
    unavailable(&dir);
    let recovery = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        recover_identity_projection_in::<()>(root(&dir), gid(), "panic", || {
            panic!("failed durable recovery")
        })
    }));
    assert!(recovery.is_err());
    unavailable(&dir);
    committed(
        recover_identity_projection_in(root(&dir), gid(), "panic", || Ok((None, ()))).unwrap(),
    );
    assert!(read_credential_in(root(&dir), IDENTITY).unwrap().is_none());
    assert!(!dir.path().join(IDENTITY_FILE).exists());
}

#[test]
fn concurrent_reader_waits_for_entire_durable_and_pair_boundary() {
    let dir = tempfile::tempdir().unwrap();
    seed(&dir);
    let path = root(&dir).to_string();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let (entered, inside) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        commit_identity_projection_durable_in(&path, gid(), "concurrent", |_| {
            entered.send(()).unwrap();
            writer_barrier.wait();
            Ok((pair(2), ()))
        })
        .unwrap()
    });
    inside.recv().unwrap();
    let path = root(&dir).to_string();
    let (read_tx, read_rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        read_tx.send(read_credential_in(&path, IDENTITY)).unwrap();
    });
    assert!(read_rx.try_recv().is_err());
    barrier.wait();
    committed(writer.join().unwrap());
    assert_eq!(read_rx.recv().unwrap().unwrap(), pair(2).map(|(_, v)| v));
    reader.join().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn started_owned_blocking_commit_survives_waiter_cancellation() {
    let dir = tempfile::tempdir().unwrap();
    seed(&dir);
    let path = root(&dir).to_string();
    let (entered, inside) = tokio::sync::oneshot::channel();
    let (release, released) = mpsc::channel();
    let (finished, done) = tokio::sync::oneshot::channel();
    let worker = tokio::task::spawn_blocking(move || {
        let result =
            commit_identity_projection_durable_in(&path, gid(), "cancelled-waiter", |_| {
                entered.send(()).unwrap();
                released.recv().unwrap();
                Ok((pair(2), ()))
            });
        finished
            .send(result.is_ok_and(|r| matches!(r, CredentialPublication::Committed { .. })))
            .unwrap();
    });
    inside.await.unwrap();
    worker.abort();
    release.send(()).unwrap();
    assert!(done.await.unwrap());
    assert_eq!(
        read_credential_in(root(&dir), IDENTITY).unwrap(),
        pair(2).map(|(_, v)| v)
    );
}
