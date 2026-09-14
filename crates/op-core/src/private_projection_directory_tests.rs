use super::*;

#[test]
fn private_owners_reuse_committed_generations_without_sharing_payloads() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("private");
    let namespace = PrivateProjectionDirectory::prepare(&path).unwrap();
    let first = namespace.prepare_child("owner-a").unwrap();
    let other = namespace.prepare_child("owner-b").unwrap();
    let payload = br#"{"document_revision":1,"specs":{"a":{"children":["b"]},"b":{}}}"#;
    let committed = first
        .update("gemma_brain", |_| Ok(Some(payload.to_vec())))
        .unwrap();
    assert_eq!(committed.generation, 1);
    let ids = ["gemma_brain".to_string()];
    assert_eq!(
        first.read_snapshot(&ids).unwrap().states["gemma_brain"],
        payload
    );
    assert!(other.read_snapshot(&ids).unwrap().states.is_empty());
    assert!(
        read_projection_snapshot_in(root.path().to_str().unwrap(), &ids)
            .unwrap()
            .states
            .is_empty()
    );
    assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o7777, 0o700);
    assert_eq!(
        std::fs::metadata(path.join("owner-a")).unwrap().mode() & 0o7777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(path.join("owner-a/gemma_brain.json"))
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
    // The same manifest format/mode remains behind the 0700 directory.
    assert_eq!(
        std::fs::metadata(path.join("owner-a/.manifest.json"))
            .unwrap()
            .mode()
            & 0o777,
        0o644
    );
    assert_eq!(
        first
            .update("gemma_brain", |old| Ok(old.map(<[u8]>::to_vec)))
            .unwrap()
            .generation,
        1
    );
    assert_eq!(
        first
            .update("gemma_brain", |_| Ok(None))
            .unwrap()
            .generation,
        2
    );
    assert!(first.read_snapshot(&ids).unwrap().states.is_empty());
}

#[test]
fn held_namespace_and_owner_descriptors_survive_path_replacement() {
    let root = tempfile::tempdir().unwrap();
    let original = root.path().join("private");
    let moved = root.path().join("moved");
    let namespace = PrivateProjectionDirectory::prepare(&original).unwrap();
    let owner = namespace.prepare_child("owner").unwrap();
    std::fs::rename(&original, &moved).unwrap();
    let replacement = PrivateProjectionDirectory::prepare(&original).unwrap();
    let replacement_owner = replacement.prepare_child("owner").unwrap();
    owner
        .update("gemma_brain", |_| Ok(Some(b"old-inode".to_vec())))
        .unwrap();
    namespace.prepare_child("later").unwrap();
    assert!(moved.join("later").is_dir());
    assert!(!original.join("later").exists());
    let ids = ["gemma_brain".to_string()];
    assert!(replacement_owner
        .read_snapshot(&ids)
        .unwrap()
        .states
        .is_empty());
    assert_eq!(
        std::fs::read(moved.join("owner/gemma_brain.json")).unwrap(),
        b"old-inode"
    );
}

#[test]
fn private_namespace_rejects_permissive_or_redirected_paths_and_does_not_create_on_read() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("missing");
    assert!(PrivateProjectionDirectory::open(&path).unwrap().is_none());
    assert!(!path.exists());
    std::fs::create_dir(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(PrivateProjectionDirectory::prepare(&path).is_err());
    assert!(!path.join(LOCK_NAME).exists());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let reader = PrivateProjectionDirectory::open(&path).unwrap().unwrap();
    assert_eq!(
        reader
            .read_snapshot(&["gemma_brain".into()])
            .unwrap()
            .generation,
        0
    );
    assert!(!path.join(LOCK_NAME).exists());
    let namespace = PrivateProjectionDirectory::prepare(&path).unwrap();
    assert!(namespace.prepare_child("../escape").is_err());
    assert!(namespace.open_child("..").is_err());
    let outside = root.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, path.join("redirect")).unwrap();
    assert!(namespace.prepare_child("redirect").is_err());
    assert!(namespace.open_child("redirect").is_err());
    assert!(namespace
        .update("identity_sled", |_| Ok(Some(b"{}".to_vec())))
        .is_err());
    // Permissions are rechecked through the held descriptor before each use.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o750)).unwrap();
    assert!(namespace.read_snapshot(&["gemma_brain".into()]).is_err());
}

#[test]
fn failed_private_publication_preserves_previous_committed_document() {
    let root = tempfile::tempdir().unwrap();
    let owner = PrivateProjectionDirectory::prepare(&root.path().join("owner")).unwrap();
    owner
        .update("gemma_brain", |_| Ok(Some(b"before".to_vec())))
        .unwrap();
    super::super::tests::inject("gemma_brain.json", 1, false);
    assert!(owner
        .update("gemma_brain", |_| Ok(Some(b"after".to_vec())))
        .is_err());
    let snapshot = owner.read_snapshot(&["gemma_brain".into()]).unwrap();
    assert_eq!(snapshot.generation, 1);
    assert_eq!(snapshot.states["gemma_brain"], b"before");
}

#[test]
fn independent_handles_serialize_complete_document_folds() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("owner");
    let initial = PrivateProjectionDirectory::prepare(&path).unwrap();
    initial
        .update("gemma_brain", |_| Ok(Some(b"0".to_vec())))
        .unwrap();
    let threads = (0..4)
        .map(|_| {
            let owner = PrivateProjectionDirectory::open(&path).unwrap().unwrap();
            std::thread::spawn(move || {
                for _ in 0..10 {
                    owner
                        .update("gemma_brain", |old| {
                            let count: u64 = std::str::from_utf8(old.unwrap())?.parse()?;
                            Ok(Some((count + 1).to_string().into_bytes()))
                        })
                        .unwrap();
                }
            })
        })
        .collect::<Vec<_>>();
    for worker in threads {
        worker.join().unwrap();
    }
    let snapshot = initial.read_snapshot(&["gemma_brain".into()]).unwrap();
    assert_eq!(snapshot.generation, 41);
    assert_eq!(snapshot.states["gemma_brain"], b"40");
}

#[test]
fn private_identity_lock_order_rejects_aliases_and_finishes_concurrent_revocation() {
    use std::sync::mpsc;
    use std::time::Duration;
    let temporary = tempfile::tempdir().unwrap();
    let identity = temporary.path().join("identity");
    std::fs::create_dir(&identity).unwrap();
    std::fs::set_permissions(&identity, std::fs::Permissions::from_mode(0o700)).unwrap();
    let identity_text = identity.to_str().unwrap();
    let gid = nix::unistd::getegid().as_raw();
    update_projection_with_credentials_in(identity_text, "identity_sled", gid, |_| {
        Ok(Some((b"{}".to_vec(), b"private-credential".to_vec())))
    })
    .unwrap();
    let namespace = PrivateProjectionDirectory::prepare(&temporary.path().join("jobs")).unwrap();
    namespace.ensure_distinct_from(&identity).unwrap();
    let alias = PrivateProjectionDirectory::open(&identity)
        .unwrap()
        .unwrap();
    assert!(alias.ensure_distinct_from(&identity).is_err());
    let link = temporary.path().join("redirect");
    std::os::unix::fs::symlink(&identity, &link).unwrap();
    assert!(namespace.ensure_distinct_from(&link).is_err());
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let identity_owned = identity_text.to_owned();
    let revocation = std::thread::spawn(move || {
        commit_identity_projection_durable_in(&identity_owned, gid, "delete-credential", |_| {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            Ok((None, ()))
        })
        .unwrap()
    });
    entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let (reading_tx, reading_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let identity_owned = identity_text.to_owned();
    let writer = namespace.clone();
    let publication = std::thread::spawn(move || {
        let result = writer.commit_document_durable_with_decision("tched_router", "output", |_| {
            reading_tx.send(()).unwrap();
            // Approved one-way acquisition only: private job file lock ->
            // canonical identity reader. Identity writers never acquire jobs.
            let credential = read_credential_in(&identity_owned, "identity_sled")?;
            ensure!(credential.is_none(), "revocation did not become visible");
            Ok(PrivateDocumentCommit::RetainPending {
                durable: (),
                error: anyhow::anyhow!("admission withdrawn"),
            })
        });
        done_tx.send(()).unwrap();
        result.unwrap()
    });
    reading_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    release_tx.send(()).unwrap();
    done_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(matches!(
        revocation.join().unwrap(),
        CredentialPublication::Committed { .. }
    ));
    assert!(matches!(
        publication.join().unwrap(),
        PrivateDocumentPublication::Pending { .. }
    ));
    assert!(namespace.read_snapshot(&["tched_router".into()]).is_err());
}
