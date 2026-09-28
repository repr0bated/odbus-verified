use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

const PLUGIN: &str = "gemma_brain";
const OLD: &[u8] = br#"{"revision":1,"jobs":{"a":{"status":"running"}},"specs":{}}"#;
const NEW: &[u8] =
    br#"{"revision":2,"jobs":{"a":{"status":"complete"}},"specs":{"one":{"children":["a","b"]}}}"#;
fn seeded() -> (tempfile::TempDir, PrivateProjectionDirectory) {
    let dir = tempfile::tempdir().unwrap();
    let owner = PrivateProjectionDirectory::prepare(&dir.path().join("owner")).unwrap();
    owner.update(PLUGIN, |_| Ok(Some(OLD.to_vec()))).unwrap();
    (dir, owner)
}
fn unavailable(owner: &PrivateProjectionDirectory) {
    assert!(owner.read_snapshot(&[PLUGIN.into()]).is_err());
    assert!(owner.update(PLUGIN, |_| Ok(None)).is_err());
}
fn committed<T>(result: PrivateDocumentPublication<T>) -> ProjectionCommit {
    match result {
        PrivateDocumentPublication::Committed { projection, .. } => projection,
        PrivateDocumentPublication::Pending { error, .. } => panic!("unexpected pending: {error}"),
    }
}

#[test]
fn complete_document_and_children_publish_after_durable_callback() {
    let (dir, owner) = seeded();
    let result = owner
        .commit_document_durable(PLUGIN, "op-two", |previous| {
            assert_eq!(previous, Some(OLD));
            // Fixture inspection only: production callbacks never read SHM.
            let manifest =
                std::fs::read_to_string(dir.path().join("owner/.manifest.json")).unwrap();
            assert!(manifest.contains("document_pending"));
            assert!(manifest.contains("op-two"));
            assert!(!manifest.contains("credential_pending"));
            Ok((Some(NEW.to_vec()), 2u64))
        })
        .unwrap();
    let projection = committed(result);
    assert_eq!(projection.generation, 2);
    assert_eq!(projection.previous.as_deref(), Some(OLD));
    assert_eq!(projection.current.as_deref(), Some(NEW));
    assert_eq!(
        owner.read_snapshot(&[PLUGIN.into()]).unwrap().states[PLUGIN],
        NEW
    );
    assert!(owner.pending_document().unwrap().is_none());
}

#[test]
fn uncertain_durable_work_blocks_stale_reads_and_requires_exact_recovery() {
    let (dir, owner) = seeded();
    let result = owner
        .commit_document_durable::<()>(PLUGIN, "uncertain", |_| bail!("durable uncertainty"))
        .unwrap();
    assert!(matches!(
        result,
        PrivateDocumentPublication::Pending { durable: None, .. }
    ));
    unavailable(&owner);
    let reopened = PrivateProjectionDirectory::open(&dir.path().join("owner"))
        .unwrap()
        .unwrap();
    unavailable(&reopened);
    let calls = AtomicUsize::new(0);
    for (plugin, operation) in [(PLUGIN, "wrong"), ("other", "uncertain")] {
        assert!(
            reopened
                .recover_document(plugin, operation, || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok((Some(OLD.to_vec()), ()))
                })
                .is_err()
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    // Unrelated publication must retain the pending metadata.
    owner.update("other", |_| Ok(Some(b"{}".to_vec()))).unwrap();
    unavailable(&owner);
    assert_eq!(
        owner.pending_document().unwrap().unwrap().operation_id,
        "uncertain"
    );
    let projection = committed(
        reopened
            .recover_document(PLUGIN, "uncertain", || Ok((Some(NEW.to_vec()), ())))
            .unwrap(),
    );
    assert_eq!(projection.generation, 3);
    assert!(projection.previous.is_none());
    assert_eq!(projection.current.as_deref(), Some(NEW));
}

#[test]
fn write_sync_and_final_manifest_failures_never_expose_old_state_after_durability() {
    for (phase, occurrence) in [
        ("gemma_brain.json", 1),
        ("private_document_data_sync", 1),
        (MANIFEST_NAME, 2),
    ] {
        let (_dir, owner) = seeded();
        super::super::super::tests::inject(phase, occurrence, false);
        let calls = AtomicUsize::new(0);
        let result = owner
            .commit_document_durable(PLUGIN, "durable-two", |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok((Some(NEW.to_vec()), 2u64))
            })
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(matches!(
            result,
            PrivateDocumentPublication::Pending {
                durable: Some(2),
                ..
            }
        ));
        unavailable(&owner);
        let projection = committed(
            owner
                .recover_document(PLUGIN, "durable-two", || Ok((Some(NEW.to_vec()), 2u64)))
                .unwrap(),
        );
        assert_eq!(projection.current.as_deref(), Some(NEW));
    }
}

#[test]
fn marker_sync_failure_prevents_durable_callback() {
    let (_dir, owner) = seeded();
    super::super::super::tests::inject("private_document_marker_sync", 1, false);
    let calls = AtomicUsize::new(0);
    let result = owner
        .commit_document_durable(PLUGIN, "not-started", |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok((Some(NEW.to_vec()), ()))
        })
        .unwrap();
    assert!(matches!(
        result,
        PrivateDocumentPublication::Pending { durable: None, .. }
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    unavailable(&owner);
    committed(
        owner
            .recover_document(PLUGIN, "not-started", || Ok((Some(OLD.to_vec()), ())))
            .unwrap(),
    );
    assert_eq!(
        owner.read_snapshot(&[PLUGIN.into()]).unwrap().states[PLUGIN],
        OLD
    );
}

#[test]
fn proven_pre_effect_rejection_clears_marker_without_changing_state() {
    let (_dir, owner) = seeded();
    let generation = owner.read_snapshot(&[PLUGIN.into()]).unwrap().generation;
    let error = owner
        .commit_document_durable_with_decision::<()>(PLUGIN, "rejected", |_| {
            Ok(PrivateDocumentCommit::Abort {
                error: anyhow::anyhow!("revision conflict"),
            })
        })
        .err()
        .expect("rejection must remain an error");
    assert_eq!(error.to_string(), "revision conflict");
    assert!(owner.pending_document().unwrap().is_none());
    let snapshot = owner.read_snapshot(&[PLUGIN.into()]).unwrap();
    assert_eq!(snapshot.generation, generation);
    assert_eq!(snapshot.states[PLUGIN], OLD);
}

#[test]
fn rejected_operation_cleanup_failure_retains_exact_pending_gate() {
    let (_dir, owner) = seeded();
    super::super::super::tests::inject(MANIFEST_NAME, 2, false);
    let result = owner
        .commit_document_durable_with_decision::<()>(PLUGIN, "rejected-cleanup", |_| {
            Ok(PrivateDocumentCommit::Abort {
                error: anyhow::anyhow!("revision conflict"),
            })
        })
        .unwrap();
    assert!(matches!(
        result,
        PrivateDocumentPublication::Pending { durable: None, .. }
    ));
    unavailable(&owner);
    let pending = owner.pending_document().unwrap().unwrap();
    assert_eq!(pending.plugin_id, PLUGIN);
    assert_eq!(pending.operation_id, "rejected-cleanup");
}

#[test]
fn panic_and_document_deletion_use_the_same_gate() {
    let (_dir, owner) = seeded();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        owner.commit_document_durable::<()>(PLUGIN, "interrupted", |_| {
            panic!("durable callback interrupted")
        })
    }));
    assert!(result.is_err());
    unavailable(&owner);
    let projection = committed(
        owner
            .recover_document(PLUGIN, "interrupted", || Ok((None, ())))
            .unwrap(),
    );
    assert!(projection.current.is_none());
    assert!(
        owner
            .read_snapshot(&[PLUGIN.into()])
            .unwrap()
            .states
            .is_empty()
    );
    assert!(
        owner
            .commit_document_durable("identity_sled", "invalid", |_| Ok((None, ())))
            .is_err()
    );
}

#[test]
fn post_durable_contract_rejection_retains_receipt_without_publishing() {
    let (dir, owner) = seeded();
    let result = owner
        .commit_document_durable_with_decision(PLUGIN, "changed-contract", |_| {
            Ok(PrivateDocumentCommit::RetainPending {
                durable: 2u64,
                error: anyhow::anyhow!("contract changed after durable commit"),
            })
        })
        .unwrap();
    assert!(matches!(
        result,
        PrivateDocumentPublication::Pending {
            durable: Some(2),
            ..
        }
    ));
    unavailable(&owner);
    assert_eq!(
        std::fs::read(dir.path().join("owner/gemma_brain.json")).unwrap(),
        OLD
    );
    let result = owner
        .recover_document_with_decision(PLUGIN, "changed-contract", || {
            Ok(PrivateDocumentCommit::RetainPending {
                durable: 2u64,
                error: anyhow::anyhow!("recovered row still lacks current render admission"),
            })
        })
        .unwrap();
    assert!(matches!(
        result,
        PrivateDocumentPublication::Pending {
            durable: Some(2),
            ..
        }
    ));
    unavailable(&owner);
    assert_eq!(
        owner.pending_document().unwrap().unwrap().operation_id,
        "changed-contract"
    );
    let final_projection = committed(
        owner
            .recover_document_with_decision(PLUGIN, "changed-contract", || {
                Ok(PrivateDocumentCommit::Publish {
                    current: Some(NEW.to_vec()),
                    durable: 2u64,
                })
            })
            .unwrap(),
    );
    assert_eq!(final_projection.current.as_deref(), Some(NEW));
    assert_eq!(final_projection.generation, 2);
}
