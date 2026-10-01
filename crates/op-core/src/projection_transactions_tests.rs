use super::*;
use std::{
    cell::RefCell,
    sync::{Arc, Barrier, mpsc},
    time::Duration,
};

// Inject a single native-write failure at the file/manifest boundary. This
// leaves rollback writes operational, exercising the public transaction API.
thread_local! {
    static FAILURE: RefCell<Option<(String, usize, bool)>> = const { RefCell::new(None) };
}

pub(super) fn write_fault(name: &str) -> Result<()> {
    let action = FAILURE.with(|fault| {
        let mut fault = fault.borrow_mut();
        match fault.as_mut() {
            Some((target, remaining, panic)) if target == name => {
                *remaining -= 1;
                if *remaining == 0 {
                    let panic = *panic;
                    *fault = None;
                    Some(panic)
                } else {
                    None
                }
            }
            _ => None,
        }
    });
    if let Some(panic) = action {
        if panic {
            panic!("simulated publisher interruption before commit");
        }
        bail!("injected file publication failure")
    }
    Ok(())
}

pub(super) fn inject(name: &str, occurrence: usize, panic: bool) {
    FAILURE.with(|fault| *fault.borrow_mut() = Some((name.into(), occurrence, panic)));
}

fn root(directory: &tempfile::TempDir) -> &str {
    directory.path().to_str().unwrap()
}

fn snapshot(directory: &tempfile::TempDir) -> ProjectionSnapshot {
    read_projection_snapshot_in(root(directory), &["plugin".into()]).unwrap()
}

fn write(directory: &tempfile::TempDir, bytes: &[u8]) -> ProjectionCommit {
    update_projection_in(root(directory), "plugin", |_| Ok(Some(bytes.to_vec()))).unwrap()
}

#[test]
fn bounded_snapshot_rejects_aggregate_size_without_partial_state() {
    let directory = tempfile::tempdir().unwrap();
    let bytes = br#"{"value":1}"#;
    write(&directory, bytes);
    update_projection_in(root(&directory), "second", |_| Ok(Some(bytes.to_vec()))).unwrap();
    let ids = ["plugin".into(), "second".into()];
    assert!(
        read_projection_snapshot_bounded_in(root(&directory), &ids, bytes.len() as u64).is_err()
    );
    let all = read_projection_snapshot_bounded_in(root(&directory), &ids, (bytes.len() * 2) as u64)
        .unwrap();
    assert_eq!(all.states.len(), 2);
    assert_eq!(all.generation, 2);
    assert_eq!(snapshot(&directory).states["plugin"], bytes);
}

#[test]
fn raw_json_commit_update_delete_and_noop_keep_exact_generations() {
    let directory = tempfile::tempdir().unwrap();
    let first = write(&directory, br#"{"child":{"false":false,"zero":0}}"#);
    assert_eq!(first.generation, 1);
    assert_eq!(
        std::fs::metadata(directory.path().join("plugin.json"))
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::read(directory.path().join("plugin.json")).unwrap(),
        first.current.clone().unwrap()
    );
    let before = snapshot(&directory);
    assert_eq!(before.states["plugin"], first.current.clone().unwrap());
    assert_eq!(
        write(&directory, first.current.as_ref().unwrap()).generation,
        1
    );
    let second = update_projection_in(root(&directory), "plugin", |old| {
        assert_eq!(old, first.current.as_deref());
        Ok(Some(br#"{"child":{"false":false,"zero":1}}"#.to_vec()))
    })
    .unwrap();
    assert_eq!(second.generation, 2);
    assert_eq!(second.previous, first.current);
    let deleted = update_projection_in(root(&directory), "plugin", |_| Ok(None)).unwrap();
    assert_eq!(deleted.generation, 3);
    assert!(deleted.current.is_none());
    assert!(!directory.path().join("plugin.json").exists());
    assert!(snapshot(&directory).states.is_empty());
}

#[test]
fn file_and_manifest_failures_restore_previous_committed_bytes() {
    for (name, occurrence) in [("plugin.json", 1), (MANIFEST_NAME, 1), (MANIFEST_NAME, 2)] {
        let directory = tempfile::tempdir().unwrap();
        write(&directory, br#"{"value":"old"}"#);
        let manifest = std::fs::read(directory.path().join(MANIFEST_NAME)).unwrap();
        inject(name, occurrence, false);
        assert!(
            update_projection_in(root(&directory), "plugin", |_| Ok(Some(
                br#"{"value":"new"}"#.to_vec()
            )))
            .is_err()
        );
        assert_eq!(snapshot(&directory).generation, 1);
        assert_eq!(snapshot(&directory).states["plugin"], br#"{"value":"old"}"#);
        assert_eq!(
            std::fs::read(directory.path().join(MANIFEST_NAME)).unwrap(),
            manifest
        );
    }
}

#[test]
fn interrupted_publication_is_unreadable_and_never_certified_by_next_write() {
    let directory = tempfile::tempdir().unwrap();
    write(&directory, br#"{"value":"old"}"#);
    inject(MANIFEST_NAME, 2, true);
    let interrupted = std::panic::catch_unwind(|| {
        let _ = update_projection_in(root(&directory), "plugin", |_| {
            Ok(Some(br#"{"value":"partial"}"#.to_vec()))
        });
    });
    assert!(interrupted.is_err());
    assert_eq!(
        std::fs::read(directory.path().join("plugin.json")).unwrap(),
        br#"{"value":"partial"}"#
    );
    assert!(
        read_projection_snapshot_in(root(&directory), &["plugin".into()])
            .unwrap_err()
            .to_string()
            .contains("incomplete")
    );
    assert!(
        update_projection_in(root(&directory), "plugin", |_| Ok(Some(b"{}".to_vec()))).is_err()
    );
}

#[test]
fn generation_overflow_rejects_before_changing_data() {
    let directory = tempfile::tempdir().unwrap();
    write(&directory, b"{}");
    std::fs::write(
        directory.path().join(MANIFEST_NAME),
        format!("{{\"generation\":{}}}", u64::MAX),
    )
    .unwrap();
    let error = update_projection_in(root(&directory), "plugin", |_| {
        Ok(Some(br#"{"changed":true}"#.to_vec()))
    })
    .unwrap_err();
    assert!(error.to_string().contains("overflow"));
    assert_eq!(snapshot(&directory).generation, u64::MAX);
    assert_eq!(snapshot(&directory).states["plugin"], b"{}");
}

#[test]
fn readers_wait_until_the_entire_locked_fold_and_commit_finish() {
    let directory = tempfile::tempdir().unwrap();
    write(&directory, b"0");
    let barrier = Arc::new(Barrier::new(2));
    let (release_tx, release_rx) = mpsc::channel();
    let writer_root = root(&directory).to_owned();
    let writer_barrier = barrier.clone();
    let writer = std::thread::spawn(move || {
        update_projection_in(&writer_root, "plugin", |_| {
            writer_barrier.wait();
            release_rx.recv().unwrap();
            Ok(Some(b"1".to_vec()))
        })
        .unwrap()
    });
    barrier.wait();
    let (read_tx, read_rx) = mpsc::channel();
    let reader_root = root(&directory).to_owned();
    let reader = std::thread::spawn(move || {
        read_tx
            .send(read_projection_snapshot_in(&reader_root, &["plugin".into()]).unwrap())
            .unwrap();
    });
    assert!(read_rx.recv_timeout(Duration::from_millis(30)).is_err());
    release_tx.send(()).unwrap();
    assert_eq!(writer.join().unwrap().generation, 2);
    let read = read_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(read.generation, 2);
    assert_eq!(read.states["plugin"], b"1");
    reader.join().unwrap();
}

#[test]
fn independent_handles_serialize_folds_without_lost_updates_or_mixed_reads() {
    let directory = tempfile::tempdir().unwrap();
    write(&directory, b"0");
    let mut threads = Vec::new();
    for _ in 0..4 {
        let state_root = root(&directory).to_owned();
        threads.push(std::thread::spawn(move || {
            for _ in 0..24 {
                update_projection_in(&state_root, "plugin", |old| {
                    let number = std::str::from_utf8(old.unwrap())?.parse::<u64>()?;
                    Ok(Some((number + 1).to_string().into_bytes()))
                })
                .unwrap();
                let snapshot =
                    read_projection_snapshot_in(&state_root, &["plugin".into()]).unwrap();
                let number = std::str::from_utf8(&snapshot.states["plugin"])
                    .unwrap()
                    .parse::<u64>()
                    .unwrap();
                assert_eq!(snapshot.generation, number + 1);
            }
        }));
    }
    for thread in threads {
        thread.join().unwrap();
    }
    let final_state = snapshot(&directory);
    assert_eq!(final_state.generation, 97);
    assert_eq!(final_state.states["plugin"], b"96");
}

#[test]
fn generic_snapshot_never_contains_credentials_and_pair_rolls_back_together() {
    let directory = tempfile::tempdir().unwrap();
    let group = nix::unistd::getgid().as_raw();
    let commit =
        update_projection_with_credentials_in(root(&directory), "identity_sled", group, |old| {
            assert!(old.is_none());
            Ok(Some((
                br#"{"session_id":"one"}"#.to_vec(),
                br#"{"session_id":"one","sealed_id":"secret"}"#.to_vec(),
            )))
        })
        .unwrap();
    assert!(
        !String::from_utf8(commit.current.unwrap())
            .unwrap()
            .contains("sealed_id")
    );
    let public = read_projection_snapshot_in(root(&directory), &["identity_sled".into()]).unwrap();
    assert_eq!(
        std::fs::metadata(directory.path().join("identity_sled.json"))
            .unwrap()
            .mode()
            & 0o777,
        0o644
    );
    assert_eq!(public.states.len(), 1);
    assert_eq!(public.states["identity_sled"], br#"{"session_id":"one"}"#);
    assert_eq!(
        read_credential_in(root(&directory), "identity_sled")
            .unwrap()
            .unwrap(),
        br#"{"session_id":"one","sealed_id":"secret"}"#
    );
    inject(MANIFEST_NAME, 2, false);
    assert!(
        update_projection_with_credentials_in(root(&directory), "identity_sled", group, |_| {
            Ok(Some((
                br#"{"session_id":"two"}"#.to_vec(),
                br#"{"sealed_id":"replacement"}"#.to_vec(),
            )))
        })
        .is_err()
    );
    assert_eq!(
        read_credential_in(root(&directory), "identity_sled")
            .unwrap()
            .unwrap(),
        br#"{"session_id":"one","sealed_id":"secret"}"#
    );
    assert_eq!(
        read_projection_snapshot_in(root(&directory), &["identity_sled".into()])
            .unwrap()
            .generation,
        1
    );
    assert!(
        read_projection_snapshot_in(root(&directory), &[".credentials/identity_sled".into()])
            .is_err()
    );
    let private_file = directory.path().join(".credentials/identity_sled.json");
    let lock_metadata = std::fs::metadata(directory.path().join(LOCK_NAME)).unwrap();
    assert_eq!(lock_metadata.mode() & 0o777, 0o640);
    assert_eq!(lock_metadata.gid(), group);
    assert_eq!(
        std::fs::metadata(&private_file).unwrap().mode() & 0o777,
        0o640
    );
    let unchanged =
        update_projection_with_credentials_in(root(&directory), "identity_sled", group, |old| {
            Ok(Some((
                br#"{"session_id":"one"}"#.to_vec(),
                old.unwrap().to_vec(),
            )))
        })
        .unwrap();
    assert_eq!(unchanged.generation, 1);
    inject(MANIFEST_NAME, 2, false);
    assert!(
        update_projection_with_credentials_in(root(&directory), "identity_sled", group, |_| Ok(
            None
        ))
        .is_err()
    );
    assert!(private_file.exists());
    assert_eq!(
        read_credential_in(root(&directory), "identity_sled")
            .unwrap()
            .unwrap(),
        br#"{"session_id":"one","sealed_id":"secret"}"#
    );
    let deleted =
        update_projection_with_credentials_in(root(&directory), "identity_sled", group, |_| {
            Ok(None)
        })
        .unwrap();
    assert_eq!(deleted.generation, 2);
    assert!(deleted.current.is_none());
    assert!(!private_file.exists());
    assert!(!directory.path().join("identity_sled.json").exists());
    assert_eq!(
        update_projection_with_credentials_in(root(&directory), "identity_sled", group, |_| Ok(
            None
        ))
        .unwrap()
        .generation,
        2
    );
}

#[test]
fn old_raw_file_modes_are_restricted_without_content_or_generation_changes() {
    let directory = tempfile::tempdir().unwrap();
    write(&directory, br#"{"opaque":"private"}"#);
    let path = directory.path().join("plugin.json");
    let old_manifest = std::fs::read(directory.path().join(MANIFEST_NAME)).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let unchanged = write(&directory, br#"{"opaque":"private"}"#);
    assert_eq!(unchanged.generation, 1);
    assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    restrict_projection_file_in(root(&directory), "plugin").unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
    assert_eq!(std::fs::read(path).unwrap(), br#"{"opaque":"private"}"#);
    assert_eq!(
        std::fs::read(directory.path().join(MANIFEST_NAME)).unwrap(),
        old_manifest
    );
    assert!(restrict_projection_file_in(root(&directory), "identity_sled").is_err());
}

#[test]
fn missing_manifest_or_unselected_files_never_become_state_authority() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("orphan.json"), b"secret-orphan").unwrap();
    assert!(
        read_projection_snapshot_in(root(&directory), &["orphan".into()])
            .unwrap()
            .states
            .is_empty()
    );
    assert!(
        !directory.path().join(LOCK_NAME).exists(),
        "reader must not create lock files"
    );
    assert!(
        update_projection_in(root(&directory), "orphan", |_| Ok(Some(b"{}".to_vec()))).is_err()
    );
    assert_eq!(
        std::fs::read(directory.path().join("orphan.json")).unwrap(),
        b"secret-orphan"
    );
    write(&directory, b"{}");
    assert_eq!(snapshot(&directory).states.len(), 1);
    assert!(!snapshot(&directory).states.contains_key("orphan"));
}

#[test]
fn unsafe_lock_paths_links_permissions_and_plugin_paths_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("foreign");
    std::fs::write(&target, b"untouched").unwrap();
    std::os::unix::fs::symlink(&target, directory.path().join(LOCK_NAME)).unwrap();
    assert!(prepare_projection_directory_in(root(&directory)).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"untouched");
    std::fs::remove_file(directory.path().join(LOCK_NAME)).unwrap();
    std::fs::hard_link(&target, directory.path().join(LOCK_NAME)).unwrap();
    assert!(prepare_projection_directory_in(root(&directory)).is_err());
    std::fs::remove_file(directory.path().join(LOCK_NAME)).unwrap();
    std::fs::write(directory.path().join(LOCK_NAME), b"").unwrap();
    for mode in [0o644, 0o660, 0o666, 0o604] {
        std::fs::set_permissions(
            directory.path().join(LOCK_NAME),
            std::fs::Permissions::from_mode(mode),
        )
        .unwrap();
        assert!(prepare_projection_directory_in(root(&directory)).is_err());
        assert!(read_projection_snapshot_in(root(&directory), &[]).is_err());
    }
    std::fs::remove_file(directory.path().join(LOCK_NAME)).unwrap();
    prepare_projection_directory_in(root(&directory)).unwrap();
    for id in ["../secret", "a/b", "a\\b", ".manifest", "", "bad\0id"] {
        assert!(update_projection_in(root(&directory), id, |_| Ok(Some(b"{}".to_vec()))).is_err());
    }
    let alias = directory.path().join("alias");
    std::os::unix::fs::symlink(directory.path(), &alias).unwrap();
    assert!(read_projection_snapshot_in(alias.to_str().unwrap(), &[]).is_err());
}

#[test]
fn root_owned_reader_source_does_not_grant_nonowner_writer_authority() {
    assert!(trusted_directory_owner(1000, 1000, true));
    assert!(trusted_directory_owner(0, 1000, false));
    assert!(!trusted_directory_owner(0, 1000, true));
    assert!(!trusted_directory_owner(1001, 1000, false));
    assert!(!trusted_directory_owner(1001, 1000, true));
}

#[test]
fn existing_generation_manifest_is_read_without_changes_after_writer_preparation() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("plugin.json"), b"{\"existing\":true}").unwrap();
    std::fs::write(directory.path().join(MANIFEST_NAME), b"{\"generation\":9}").unwrap();
    assert!(read_projection_snapshot_in(root(&directory), &["plugin".into()]).is_err());
    prepare_projection_directory_in(root(&directory)).unwrap();
    let before = std::fs::metadata(directory.path().join(MANIFEST_NAME))
        .unwrap()
        .modified()
        .unwrap();
    let state = snapshot(&directory);
    assert_eq!(state.generation, 9);
    assert_eq!(state.states["plugin"], b"{\"existing\":true}");
    assert_eq!(
        std::fs::metadata(directory.path().join(MANIFEST_NAME))
            .unwrap()
            .modified()
            .unwrap(),
        before
    );
}
