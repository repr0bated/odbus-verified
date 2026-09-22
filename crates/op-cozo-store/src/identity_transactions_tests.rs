use super::*;
use std::{
    cell::RefCell,
    sync::{Arc, Barrier},
    time::Duration,
};
thread_local! { static FAILURE: RefCell<Option<&'static str>> = const { RefCell::new(None) }; }
pub(super) fn fault(point: &str) -> std::result::Result<(), CozoError> {
    if FAILURE.with(|f| {
        let mut f = f.borrow_mut();
        if *f == Some(point) {
            *f = None;
            true
        } else {
            false
        }
    }) {
        return Err(err("injected identity transaction failure"));
    }
    Ok(())
}
fn inject(point: &'static str) {
    FAILURE.with(|f| *f.borrow_mut() = Some(point));
}
fn row(sid: &str) -> IdentitySledRecord {
    IdentitySledRecord {
        session_id: sid.into(),
        wireguard_pubkey: format!("key-{sid}"),
        interface: String::new(),
        peer_ip: String::new(),
        mutation_index: 0,
        session_genesis: String::new(),
        trace_id: String::new(),
        schema_version: 3,
        vector_id: String::new(),
        sealed_id: String::new(),
        principal_kind: None,
        btrfs_device_json: String::new(),
        instance_json: format!("{{\"name\":\"{sid}\"}}"),
        session_started_at: 1,
        last_seen_at: 1,
        active: false,
        expires_at: 0,
    }
}
fn genesis(sid: &str) -> GenesisInputsRecord {
    GenesisInputsRecord {
        session_id: sid.into(),
        arrival_timestamp: 10,
        chain_head_at_arrival: "head".into(),
        catalog_hash_at_arrival: "catalog".into(),
        head_timestamp_at_arrival: 9,
        schema_content_hash: "record-format".into(),
    }
}

fn legacy_credential_fixture() -> (IdentitySledRecord, GenesisInputsRecord, String) {
    let wireguard_pubkey = "QkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkI=".to_string();
    let session_id = op_identity::session::derive_session_id(&wireguard_pubkey);
    let genesis = GenesisInputsRecord {
        session_id: session_id.clone(),
        arrival_timestamp: 10,
        chain_head_at_arrival: "head".into(),
        catalog_hash_at_arrival: "catalog".into(),
        head_timestamp_at_arrival: 9,
        schema_content_hash: "record-format".into(),
    };
    let session_genesis = "ab".repeat(32);
    let trace_id = "cd".repeat(16);
    let legacy = op_identity::sealed_id::SealedId {
        principal_id: op_identity::session::derive_principal_id(&wireguard_pubkey),
        principal_kind: "wireguard-principal".into(),
        session_id: session_id.clone(),
        wireguard_pubkey: wireguard_pubkey.clone(),
        session_genesis: session_genesis.clone(),
        trace_id: trace_id.clone(),
        schema_version: 3,
        issued_at: genesis.arrival_timestamp,
        expires_at: 0,
        arrival_timestamp: genesis.arrival_timestamp,
        chain_head_at_arrival: genesis.chain_head_at_arrival.clone(),
        catalog_hash_at_arrival: genesis.catalog_hash_at_arrival.clone(),
        head_timestamp_at_arrival: genesis.head_timestamp_at_arrival,
        transport_scope: "dbus,grpc,mcp".into(),
    }
    .to_inline_ref()
    .unwrap();
    let current = op_identity::sealed_id::SealedId {
        transport_scope: "dbus,grpc,mcp,mqtt".into(),
        ..op_identity::sealed_id::SealedId::from_inline_ref(&legacy).unwrap()
    }
    .to_inline_ref()
    .unwrap();
    let row = IdentitySledRecord {
        session_id,
        wireguard_pubkey: wireguard_pubkey.clone(),
        interface: "wg0".into(),
        peer_ip: "10.0.0.7".into(),
        mutation_index: 7,
        session_genesis,
        trace_id,
        schema_version: 3,
        vector_id: "vec".into(),
        sealed_id: legacy,
        principal_kind: Some("human".into()),
        btrfs_device_json: "{\"device\":\"x\"}".into(),
        instance_json: "{\"name\":\"container\"}".into(),
        session_started_at: 1,
        last_seen_at: 8,
        active: true,
        expires_at: 0,
    };
    (row, genesis, current)
}
fn put(
    store: &CozoGraphShuttle,
    sid: &str,
    op: &str,
    record: IdentitySledRecord,
    genesis: Option<GenesisInputsRecord>,
) -> IdentityRevision {
    let expected = store.read_identity_revision(sid).unwrap();
    store
        .apply_identity_intent(
            sid,
            op,
            IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                expected,
                record: Some(record),
                genesis,
            })),
        )
        .unwrap()
        .current
}

#[test]
fn native_commit_errors_are_not_mistaken_for_transport_success() {
    assert!(unwrap_commit_reply::<(), _, &str>(Ok(Err("native commit conflict"))).is_err());
    assert!(unwrap_commit_reply::<(), &str, _>(Err("closed channel")).is_err());
    assert!(unwrap_commit_reply::<(), &str, &str>(Ok(Ok(()))).is_ok());
}

#[test]
fn transport_refresh_preserves_anchored_data_and_replays_exactly() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let (row, genesis, current_sealed_id) = legacy_credential_fixture();
    let sid = row.session_id.clone();
    let first = put(
        &store,
        &sid,
        "seed-refresh",
        row.clone(),
        Some(genesis.clone()),
    );
    let committed = store
        .apply_identity_intent(
            &sid,
            "refresh-1",
            IdentityIntent::RefreshTransportCredential {
                expected: Box::new(first.clone()),
                sealed_id: current_sealed_id.clone(),
            },
        )
        .unwrap();
    assert!(!committed.replayed);
    assert_eq!(committed.current.revision, first.revision + 1);
    let refreshed = committed.current.record.clone().unwrap();
    assert_eq!(refreshed.sealed_id, current_sealed_id);
    assert_eq!(refreshed.mutation_index, row.mutation_index + 1);
    let mut expected_row = row.clone();
    expected_row.sealed_id = current_sealed_id.clone();
    expected_row.mutation_index += 1;
    assert_eq!(refreshed, expected_row);
    assert_eq!(committed.current.genesis, Some(genesis));

    let replay = store
        .apply_identity_intent(
            &sid,
            "refresh-1",
            IdentityIntent::RefreshTransportCredential {
                expected: Box::new(first),
                sealed_id: current_sealed_id,
            },
        )
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.current, committed.current);
}

#[test]
fn transport_refresh_rejects_stale_reuse_claim_and_scope_changes() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let (row, genesis, current_sealed_id) = legacy_credential_fixture();
    let sid = row.session_id.clone();
    let first = put(&store, &sid, "seed-refresh", row.clone(), Some(genesis));
    let refreshed = store
        .apply_identity_intent(
            &sid,
            "refresh-1",
            IdentityIntent::RefreshTransportCredential {
                expected: Box::new(first.clone()),
                sealed_id: current_sealed_id.clone(),
            },
        )
        .unwrap();
    assert!(store
        .apply_identity_intent(
            &sid,
            "refresh-stale",
            IdentityIntent::RefreshTransportCredential {
                expected: Box::new(first.clone()),
                sealed_id: current_sealed_id.clone(),
            },
        )
        .is_err());

    let altered_claim = op_identity::sealed_id::SealedId {
        transport_scope: "dbus,grpc,mcp,mqtt".into(),
        wireguard_pubkey: "Q0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0M=".into(),
        ..op_identity::sealed_id::SealedId::from_inline_ref(&row.sealed_id).unwrap()
    }
    .to_inline_ref()
    .unwrap();
    assert!(store
        .apply_identity_intent(
            &sid,
            "refresh-claim-change",
            IdentityIntent::RefreshTransportCredential {
                expected: Box::new(first.clone()),
                sealed_id: altered_claim.clone(),
            },
        )
        .is_err());

    let disallowed_scope = op_identity::sealed_id::SealedId {
        transport_scope: "dbus,mqtt".into(),
        ..op_identity::sealed_id::SealedId::from_inline_ref(&row.sealed_id).unwrap()
    }
    .to_inline_ref()
    .unwrap();
    assert!(store
        .apply_identity_intent(
            &sid,
            "refresh-scope-change",
            IdentityIntent::RefreshTransportCredential {
                expected: Box::new(first.clone()),
                sealed_id: disallowed_scope,
            },
        )
        .is_err());

    let mut different_intent = first;
    different_intent.revision += 1;
    let reused = match store.apply_identity_intent(
        &sid,
        "refresh-1",
        IdentityIntent::RefreshTransportCredential {
            expected: Box::new(different_intent),
            sealed_id: current_sealed_id,
        },
    ) {
        Ok(_) => panic!("operation reuse with a different credential must fail"),
        Err(error) => error,
    };
    assert!(reused.to_string().contains("operation id reused"));
    assert_eq!(
        store.read_identity_revision(&sid).unwrap(),
        refreshed.current
    );
}

#[test]
fn row_genesis_revision_commit_together_and_anchored_fields_are_immutable() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let mut anchored = row("sid-a");
    anchored.sealed_id = "fixture-authored-SID1-bytes".into();
    anchored.session_genesis = "fixture-anchor".into();
    anchored.trace_id = "fixture-trace".into();
    let first = put(
        &store,
        "sid-a",
        "first",
        anchored.clone(),
        Some(genesis("sid-a")),
    );
    assert_eq!(first.revision, 1);
    assert_eq!(first.record, Some(anchored.clone()));
    assert_eq!(first.genesis, Some(genesis("sid-a")));
    let mut conflicting = anchored;
    conflicting.sealed_id = "substitute".into();
    assert!(store
        .apply_identity_intent(
            "sid-a",
            "invalid-binding",
            IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                expected: first.clone(),
                record: Some(conflicting),
                genesis: first.genesis.clone()
            }))
        )
        .is_err());
    assert_eq!(store.read_identity_revision("sid-a").unwrap(), first);
    let all = store.read_identity_recovery_snapshot().unwrap();
    assert_eq!(all, [first]);
}

#[test]
fn mid_transaction_errors_do_not_commit_partial_row_genesis_or_revision() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    for point in ["after_row", "before_revision", "before_commit"] {
        let expected = store.read_identity_revision("new").unwrap();
        inject(point);
        assert!(store
            .apply_identity_intent(
                "new",
                point,
                IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                    expected: expected.clone(),
                    record: Some(row("new")),
                    genesis: None
                }))
            )
            .is_err());
        assert_eq!(store.read_identity_revision("new").unwrap(), expected);
    }
    assert!(store.read_identity_recovery_snapshot().unwrap().is_empty());
}

#[test]
fn barrier_failure_leaves_committed_revision_recoverable_and_retry_idempotent() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let expected = store.read_identity_revision("one").unwrap();
    inject("barrier");
    assert!(store
        .apply_identity_intent(
            "one",
            "barrier-op",
            IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                expected: expected.clone(),
                record: Some(row("one")),
                genesis: None
            }))
        )
        .is_err());
    let recovered = store.read_identity_recovery_snapshot().unwrap();
    assert_eq!(recovered[0].revision, 1);
    assert_eq!(recovered[0].operation_id, "barrier-op");
    let retried = store
        .apply_identity_intent(
            "one",
            "barrier-op",
            IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                expected,
                record: Some(row("one")),
                genesis: None,
            })),
        )
        .unwrap();
    assert!(retried.replayed);
    assert_eq!(retried.current.revision, 1);
}

#[test]
fn compare_exchange_conflicts_and_delete_tombstone_prevent_stale_resurrection() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let absent = store.read_identity_revision("one").unwrap();
    let first = put(&store, "one", "create", row("one"), None);
    assert!(store
        .apply_identity_intent(
            "one",
            "stale",
            IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                expected: absent.clone(),
                record: Some(row("one")),
                genesis: None
            }))
        )
        .is_err());
    let deletion = store
        .apply_identity_intent(
            "one",
            "delete",
            IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                expected: first.clone(),
                record: None,
                genesis: None,
            })),
        )
        .unwrap();
    assert_eq!(deletion.current.revision, 2);
    assert!(deletion.current.record.is_none());
    assert!(store
        .apply_identity_intent(
            "one",
            "resurrect",
            IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                expected: absent,
                record: Some(row("one")),
                genesis: None
            }))
        )
        .is_err());
    let repeated = store
        .apply_identity_intent(
            "one",
            "delete",
            IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                expected: first,
                record: None,
                genesis: None,
            })),
        )
        .unwrap();
    assert!(repeated.replayed);
    assert_eq!(
        store.read_identity_recovery_snapshot().unwrap(),
        [deletion.current]
    );
}

#[test]
fn concurrent_intents_preserve_sibling_rows_and_advance_only_index() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    put(&store, "one", "create-one", row("one"), None);
    let sibling = put(&store, "two", "create-two", row("two"), None);
    let barrier = Arc::new(Barrier::new(3));
    let workers: Vec<_> = [500, 10]
        .into_iter()
        .map(|index| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .apply_identity_intent(
                        "one",
                        &format!("advance-{index}"),
                        IdentityIntent::AdvanceMutationIndex { event_id: index },
                    )
                    .unwrap();
            })
        })
        .collect();
    barrier.wait();
    for worker in workers {
        worker.join().unwrap();
    }
    let current = store.read_identity_revision("one").unwrap();
    assert_eq!(current.record.unwrap().mutation_index, 500);
    assert_eq!(current.revision, 3);
    assert_eq!(store.read_identity_revision("two").unwrap(), sibling);
    assert!(store
        .apply_identity_intent(
            "one",
            "overflow",
            IdentityIntent::AdvanceMutationIndex { event_id: u64::MAX }
        )
        .is_err());
}

#[test]
fn recovery_rejects_corrupt_rows_instead_of_dropping_them() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let mut broken = row("bad");
    broken.instance_json = "{broken".into();
    store.put_identity_sled(&broken).unwrap(); // emulate legacy on-disk corruption
    assert!(store.read_identity_revision("bad").is_err());
    assert!(store.read_identity_recovery_snapshot().is_err());
    broken.instance_json = "[]".into();
    store.put_identity_sled(&broken).unwrap();
    assert!(store.read_identity_recovery_snapshot().is_err());
}

fn reopen(path: &Path) -> CozoGraphShuttle {
    for _ in 0..100 {
        if let Ok(store) = CozoGraphShuttle::new_persistent(path.to_path_buf()) {
            return store;
        }
        // Cozo's transaction worker may still own the RocksDB process lock.
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("persistent fixture did not release its RocksDB lock")
}

#[test]
fn synced_identity_rows_genesis_and_deletion_revision_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity");
    let committed;
    {
        let store = CozoGraphShuttle::new_persistent(path.clone()).unwrap();
        let mut anchored = row("retained");
        anchored.sealed_id = "fixture-only-SID1".into();
        anchored.session_genesis = "anchor".into();
        anchored.trace_id = "trace".into();
        anchored.principal_kind = Some("service".into());
        committed = put(
            &store,
            "retained",
            "persist",
            anchored,
            Some(genesis("retained")),
        );
        let first = put(&store, "removed", "create-removed", row("removed"), None);
        store
            .apply_identity_intent(
                "removed",
                "remove",
                IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                    expected: first,
                    record: None,
                    genesis: None,
                })),
            )
            .unwrap();
    }
    let store = reopen(&path);
    assert_eq!(store.read_identity_revision("retained").unwrap(), committed);
    let deleted = store.read_identity_revision("removed").unwrap();
    assert_eq!(deleted.revision, 2);
    assert_eq!(deleted.operation_id, "remove");
    assert!(deleted.record.is_none());
    assert_eq!(store.read_identity_recovery_snapshot().unwrap().len(), 2);
}

#[test]
fn persistent_barrier_refuses_symlink_or_nonowned_writable_directory() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("actual");
    std::fs::create_dir(&target).unwrap();
    let link = dir.path().join("link");
    symlink(&target, &link).unwrap();
    assert!(open_persistent_directory(&link).is_err());
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(open_persistent_directory(&target).is_err());
}

#[test]
fn versioned_sessions_and_tombstones_reject_every_legacy_credential_writer() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let legacy = row("legacy");
    store.put_identity_sled(&legacy).unwrap();
    store.put_identity_genesis(&genesis("legacy")).unwrap();
    store.touch_identity_sled("legacy", 2, false).unwrap();
    let current = store.read_identity_revision("legacy").unwrap();
    assert_eq!(current.revision, 0);
    let versioned = store
        .apply_identity_intent(
            "legacy",
            "adopt",
            IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                expected: current.clone(),
                record: current.record,
                genesis: current.genesis,
            })),
        )
        .unwrap()
        .current;
    for deleted in [false, true] {
        let expected = if deleted {
            store
                .apply_identity_intent(
                    "legacy",
                    "delete",
                    IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                        expected: versioned.clone(),
                        record: None,
                        genesis: None,
                    })),
                )
                .unwrap()
                .current
        } else {
            versioned.clone()
        };
        assert!(store.put_identity_sled(&legacy).is_err());
        assert!(store.put_identity_genesis(&genesis("legacy")).is_err());
        assert!(store.touch_identity_sled("legacy", 3, true).is_err());
        assert!(store.delete_identity_sled("legacy").is_err());
        assert_eq!(store.read_identity_revision("legacy").unwrap(), expected);
    }
}

#[test]
fn durability_boundary_pins_actual_data_directory_and_refuses_options_override() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cozo");
    let boundary = PersistentIdentityBoundary::open(&path).unwrap();
    boundary.require_default_layout().unwrap();
    assert_eq!(
        boundary.data.metadata().unwrap().ino(),
        std::fs::metadata(path.join("data")).unwrap().ino()
    );
    std::fs::rename(path.join("data"), path.join("old-data")).unwrap();
    std::fs::create_dir(path.join("data")).unwrap();
    assert!(boundary.verify().is_err());
    std::fs::remove_dir(path.join("data")).unwrap();
    symlink(path.join("old-data"), path.join("data")).unwrap();
    assert!(PersistentIdentityBoundary::open(&path).is_err());

    let custom = dir.path().join("custom");
    std::fs::create_dir(&custom).unwrap();
    std::fs::write(
        custom.join("options"),
        b"fixture: no native database is opened",
    )
    .unwrap();
    let boundary = PersistentIdentityBoundary::open(&custom).unwrap();
    boundary.verify().unwrap();
    assert!(boundary.require_default_layout().is_err());
    // Even a newly introduced options file invalidates an otherwise pinned root.
    let ordinary = PersistentIdentityBoundary::open(&dir.path().join("ordinary")).unwrap();
    std::fs::write(ordinary.path.join("options"), b"new override").unwrap();
    assert!(ordinary.verify().is_err());
}

#[test]
fn non_utf8_database_path_is_rejected_before_any_directory_is_created() {
    use std::os::unix::ffi::OsStringExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir
        .path()
        .join(std::ffi::OsString::from_vec(vec![b'x', 0xff]));
    assert!(CozoGraphShuttle::new_persistent(path.clone()).is_err());
    assert!(!path.exists());
}

#[test]
fn bounded_recovery_events_are_strict_ordered_and_retained_after_identity_deletion() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let one = put(&store, "one", "create", row("one"), None);
    store
        .append_session_event("one", "arrival", "", "first", 10)
        .unwrap();
    store
        .append_session_event("one", "mutation", "", "second", 20)
        .unwrap();
    store
        .append_session_event("two", "arrival", "", "tie", 20)
        .unwrap();
    store
        .apply_identity_intent(
            "one",
            "delete",
            IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                expected: one,
                record: None,
                genesis: None,
            })),
        )
        .unwrap();
    let state = store.read_identity_recovery_state(2).unwrap();
    assert!(state.identities[0].record.is_none());
    assert_eq!(
        state
            .events
            .iter()
            .map(|event| (event.session_id.as_str(), event.seq, event.content.as_str()))
            .collect::<Vec<_>>(),
        [("one", 1, "second"), ("two", 0, "tie")]
    );
    assert_eq!(
        store.read_identity_recovery_state(3).unwrap().events.len(),
        3
    );
    assert!(store
        .read_identity_recovery_state(0)
        .unwrap()
        .events
        .is_empty());
    assert!(store.read_identity_recovery_state(4097).is_err());
    cozo_run(&store.db,"?[session_id,seq,kind,subid,content,created_at] <- [['bad',-1,'mutation','','bad',30]] :put session_events {session_id,seq => kind,subid,content,created_at}",BTreeMap::new()).unwrap();
    assert!(store.read_identity_recovery_state(1).is_err());
}

#[test]
fn append_failure_and_overflow_do_not_claim_success_or_repeat_uncertain_event() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    inject("before_commit");
    assert!(store
        .append_session_event("one", "arrival", "", "aborted", 1)
        .is_err());
    assert!(store
        .read_identity_recovery_state(10)
        .unwrap()
        .events
        .is_empty());
    inject("barrier");
    assert!(store
        .append_session_event("one", "arrival", "", "uncertain", 2)
        .is_err());
    let recovery = store.read_identity_recovery_state(10).unwrap();
    assert_eq!(recovery.events.len(), 1);
    assert_eq!(recovery.events[0].seq, 0);
    assert_eq!(recovery.events[0].content, "uncertain");
    // Explicit recovery reads committed history; it does not retry the append.
    assert_eq!(
        store.read_identity_recovery_state(10).unwrap().events.len(),
        1
    );
    cozo_run(&store.db,"?[session_id,seq,kind,subid,content,created_at] <- [[$sid,$seq,'mutation','','max',1]] :put session_events {session_id,seq => kind,subid,content,created_at}", BTreeMap::from([("sid".into(),DataValue::Str("full".into())),("seq".into(),dv_int(i64::MAX))])).unwrap();
    assert!(store
        .append_session_event("full", "mutation", "", "overflow", 3)
        .is_err());
    assert_eq!(store.list_session_events("full", 0).unwrap().len(), 1);
}

#[test]
fn concurrent_event_appends_allocate_distinct_sequences() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let workers: Vec<_> = (0..2)
        .map(|i| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .append_session_event("one", "mutation", "", &format!("event-{i}"), i + 1)
                    .unwrap()
            })
        })
        .collect();
    barrier.wait();
    let mut sequences: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    sequences.sort_unstable();
    assert_eq!(sequences, [0, 1]);
    assert_eq!(
        store.read_identity_recovery_state(10).unwrap().events.len(),
        2
    );
}

#[test]
fn actor_class_is_atomic_immutable_and_joined_without_rewriting_legacy_columns() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let legacy = row("legacy-class");
    store.put_identity_sled(&legacy).unwrap();
    let columns = cozo_run(&store.db, "::columns identity_sessions", BTreeMap::new()).unwrap();
    assert_eq!(columns.rows.len(), 16);
    let before = cozo_run(
        &store.db,
        &select(ROW_FIELDS, "identity_sessions", false),
        BTreeMap::new(),
    )
    .unwrap();
    let expected = store.read_identity_revision("legacy-class").unwrap();
    assert!(expected.record.as_ref().unwrap().principal_kind.is_none());
    let mut classified = legacy.clone();
    classified.principal_kind = Some("human".into());
    assert!(
        store.put_identity_sled(&classified).is_err(),
        "legacy API must never silently discard class"
    );
    inject("before_commit");
    assert!(store
        .apply_identity_intent(
            "legacy-class",
            "failed-class",
            IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                expected: expected.clone(),
                record: Some(classified.clone()),
                genesis: None,
            }))
        )
        .is_err());
    assert_eq!(
        store.read_identity_revision("legacy-class").unwrap(),
        expected
    );
    let committed = store
        .apply_identity_intent(
            "legacy-class",
            "classify",
            IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                expected,
                record: Some(classified.clone()),
                genesis: None,
            })),
        )
        .unwrap()
        .current;
    assert_eq!(
        store
            .get_identity_sled("legacy-class")
            .unwrap()
            .unwrap()
            .principal_kind,
        Some("human".into())
    );
    assert_eq!(
        store.list_identity_sessions().unwrap()[0].principal_kind,
        Some("human".into())
    );
    assert_eq!(
        store.read_identity_recovery_snapshot().unwrap().as_slice(),
        std::slice::from_ref(&committed)
    );
    let after = cozo_run(
        &store.db,
        &select(ROW_FIELDS, "identity_sessions", false),
        BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(before.rows, after.rows);
    assert_eq!(
        columns.rows,
        cozo_run(&store.db, "::columns identity_sessions", BTreeMap::new())
            .unwrap()
            .rows
    );
    for new_class in [
        None,
        Some("service".into()),
        Some("wireguard-principal".into()),
    ] {
        let mut changed = classified.clone();
        changed.principal_kind = new_class;
        assert!(store
            .apply_identity_intent(
                "legacy-class",
                "reclassify",
                IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                    expected: committed.clone(),
                    record: Some(changed),
                    genesis: None,
                }))
            )
            .is_err());
        assert_eq!(
            store.read_identity_revision("legacy-class").unwrap(),
            committed
        );
    }
}

#[test]
fn private_constructor_closes_directory_traversal_before_opening_native_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity-private");
    std::fs::create_dir(&path).unwrap();
    std::fs::create_dir(path.join("data")).unwrap();
    for directory in [&path, &path.join("data")] {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let store = CozoGraphShuttle::new_persistent_private(path.clone()).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o7777, 0o700);
    assert_eq!(
        std::fs::metadata(path.join("data")).unwrap().mode() & 0o7777,
        0o700
    );
    let committed = put(&store, "private", "create-private", row("private"), None);
    assert_eq!(store.read_identity_revision("private").unwrap(), committed);
    // Future files may follow the process umask, but cannot be reached through
    // either private directory by group/other users. Do not mutate global umask.
    let boundary = store.identity_directory.as_ref().unwrap();
    assert!(boundary.private);
    boundary.verify().unwrap();
}

#[test]
fn generic_constructor_preserves_existing_directory_modes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("generic");
    std::fs::create_dir(&path).unwrap();
    std::fs::create_dir(path.join("data")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(path.join("data"), std::fs::Permissions::from_mode(0o750)).unwrap();
    let store = CozoGraphShuttle::new_persistent(path.clone()).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o755);
    assert_eq!(
        std::fs::metadata(path.join("data")).unwrap().mode() & 0o777,
        0o750
    );
    assert!(!store.identity_directory.as_ref().unwrap().private);
}

#[test]
fn private_constructor_refuses_symlink_and_options_before_native_open() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    std::fs::create_dir(&target).unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
    let link = dir.path().join("link");
    symlink(&target, &link).unwrap();
    assert!(CozoGraphShuttle::new_persistent_private(link).is_err());
    assert_eq!(std::fs::metadata(&target).unwrap().mode() & 0o777, 0o755);
    assert!(!target.join("manifest").exists());
    let options = dir.path().join("options-root");
    std::fs::create_dir(&options).unwrap();
    std::fs::set_permissions(&options, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(options.join("options"), b"unverified WAL/data redirection").unwrap();
    let error = match CozoGraphShuttle::new_persistent_private(options.clone()) {
        Ok(_) => panic!("private store must refuse unknown storage paths"),
        Err(error) => error,
    };
    assert!(error
        .to_string()
        .contains("private identity store refuses custom Cozo options"));
    assert_eq!(std::fs::metadata(&options).unwrap().mode() & 0o777, 0o700);
    assert!(
        !options.join("manifest").exists(),
        "native database open must not occur"
    );
}

#[test]
fn private_boundary_rejects_permission_drift_before_later_identity_effects() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private-drift");
    let store = CozoGraphShuttle::new_persistent_private(path.clone()).unwrap();
    let expected = store.read_identity_revision("new").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(store
        .apply_identity_intent(
            "new",
            "drift",
            IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                expected: expected.clone(),
                record: Some(row("new")),
                genesis: None,
            }))
        )
        .is_err());
    assert_eq!(store.read_identity_revision("new").unwrap(), expected);
}
