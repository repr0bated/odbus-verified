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
        nextdns: None,
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

fn credential_with_nextdns(
    sealed_id: &str,
    nextdns: Option<op_identity::sealed_id::NextDnsContext>,
) -> String {
    op_identity::sealed_id::SealedId {
        nextdns,
        ..op_identity::sealed_id::SealedId::from_inline_ref(sealed_id).unwrap()
    }
    .to_inline_ref()
    .unwrap()
}

fn nextdns_context(profile_id: &str, device_name: &str) -> op_identity::sealed_id::NextDnsContext {
    op_identity::sealed_id::NextDnsContext {
        profile_id: profile_id.into(),
        device_name: device_name.into(),
    }
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
    assert!(
        store
            .apply_identity_intent(
                &sid,
                "refresh-stale",
                IdentityIntent::RefreshTransportCredential {
                    expected: Box::new(first.clone()),
                    sealed_id: current_sealed_id.clone(),
                },
            )
            .is_err()
    );

    let altered_claim = op_identity::sealed_id::SealedId {
        transport_scope: "dbus,grpc,mcp,mqtt".into(),
        wireguard_pubkey: "Q0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0M=".into(),
        ..op_identity::sealed_id::SealedId::from_inline_ref(&row.sealed_id).unwrap()
    }
    .to_inline_ref()
    .unwrap();
    assert!(
        store
            .apply_identity_intent(
                &sid,
                "refresh-claim-change",
                IdentityIntent::RefreshTransportCredential {
                    expected: Box::new(first.clone()),
                    sealed_id: altered_claim.clone(),
                },
            )
            .is_err()
    );

    let disallowed_scope = op_identity::sealed_id::SealedId {
        transport_scope: "dbus,mqtt".into(),
        ..op_identity::sealed_id::SealedId::from_inline_ref(&row.sealed_id).unwrap()
    }
    .to_inline_ref()
    .unwrap();
    assert!(
        store
            .apply_identity_intent(
                &sid,
                "refresh-scope-change",
                IdentityIntent::RefreshTransportCredential {
                    expected: Box::new(first.clone()),
                    sealed_id: disallowed_scope,
                },
            )
            .is_err()
    );

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
fn transport_refresh_assigns_and_changes_nextdns_without_dropping_it() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let (row, genesis, current_sealed_id) = legacy_credential_fixture();
    let sid = row.session_id.clone();
    let first = put(&store, &sid, "seed-nextdns-refresh", row, Some(genesis));

    let assigned_context = nextdns_context("689ec7", "Ghostbridge-Incus");
    let assigned_credential =
        credential_with_nextdns(&current_sealed_id, Some(assigned_context.clone()));
    let assigned = store
        .apply_identity_intent(
            &sid,
            "assign-nextdns",
            IdentityIntent::RefreshTransportCredential {
                expected: Box::new(first),
                sealed_id: assigned_credential,
            },
        )
        .unwrap();
    assert!(!assigned.replayed);
    assert_eq!(
        op_identity::sealed_id::SealedId::from_inline_ref(
            &assigned.current.record.as_ref().unwrap().sealed_id
        )
        .unwrap()
        .nextdns,
        Some(assigned_context)
    );

    let changed_context = nextdns_context("abcdef", "Laptop-2");
    let changed_credential = credential_with_nextdns(
        &assigned.current.record.as_ref().unwrap().sealed_id,
        Some(changed_context.clone()),
    );
    let changed = store
        .apply_identity_intent(
            &sid,
            "change-nextdns",
            IdentityIntent::RefreshTransportCredential {
                expected: Box::new(assigned.current.clone()),
                sealed_id: changed_credential,
            },
        )
        .unwrap();
    assert!(!changed.replayed);
    assert_eq!(
        op_identity::sealed_id::SealedId::from_inline_ref(
            &changed.current.record.as_ref().unwrap().sealed_id
        )
        .unwrap()
        .nextdns,
        Some(changed_context)
    );

    let dropped =
        credential_with_nextdns(&changed.current.record.as_ref().unwrap().sealed_id, None);
    assert!(
        store
            .apply_identity_intent(
                &sid,
                "drop-nextdns",
                IdentityIntent::RefreshTransportCredential {
                    expected: Box::new(changed.current.clone()),
                    sealed_id: dropped,
                },
            )
            .is_err()
    );

    let unchanged = changed.current.record.as_ref().unwrap().sealed_id.clone();
    assert!(
        store
            .apply_identity_intent(
                &sid,
                "unchanged-refresh",
                IdentityIntent::RefreshTransportCredential {
                    expected: Box::new(changed.current),
                    sealed_id: unchanged,
                },
            )
            .is_err()
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
    assert!(
        store
            .apply_identity_intent(
                "sid-a",
                "invalid-binding",
                IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                    expected: first.clone(),
                    record: Some(conflicting),
                    genesis: first.genesis.clone()
                }))
            )
            .is_err()
    );
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
        assert!(
            store
                .apply_identity_intent(
                    "new",
                    point,
                    IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                        expected: expected.clone(),
                        record: Some(row("new")),
                        genesis: None
                    }))
                )
                .is_err()
        );
        assert_eq!(store.read_identity_revision("new").unwrap(), expected);
    }
    assert!(store.read_identity_recovery_snapshot().unwrap().is_empty());
}

#[test]
fn barrier_failure_leaves_committed_revision_recoverable_and_retry_idempotent() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let expected = store.read_identity_revision("one").unwrap();
    inject("barrier");
    assert!(
        store
            .apply_identity_intent(
                "one",
                "barrier-op",
                IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                    expected: expected.clone(),
                    record: Some(row("one")),
                    genesis: None
                }))
            )
            .is_err()
    );
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
    assert!(
        store
            .apply_identity_intent(
                "one",
                "stale",
                IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                    expected: absent.clone(),
                    record: Some(row("one")),
                    genesis: None
                }))
            )
            .is_err()
    );
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
    assert!(
        store
            .apply_identity_intent(
                "one",
                "resurrect",
                IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                    expected: absent,
                    record: Some(row("one")),
                    genesis: None
                }))
            )
            .is_err()
    );
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
    assert!(
        store
            .apply_identity_intent(
                "one",
                "overflow",
                IdentityIntent::AdvanceMutationIndex { event_id: u64::MAX }
            )
            .is_err()
    );
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
    use std::os::unix::fs::{PermissionsExt, symlink};
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
    assert!(
        store
            .read_identity_recovery_state(0)
            .unwrap()
            .events
            .is_empty()
    );
    assert!(store.read_identity_recovery_state(4097).is_err());
    cozo_run(&store.db,"?[session_id,seq,kind,subid,content,created_at] <- [['bad',-1,'mutation','','bad',30]] :put session_events {session_id,seq => kind,subid,content,created_at}",BTreeMap::new()).unwrap();
    assert!(store.read_identity_recovery_state(1).is_err());
}

#[test]
fn append_failure_and_overflow_do_not_claim_success_or_repeat_uncertain_event() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    inject("before_commit");
    assert!(
        store
            .append_session_event("one", "arrival", "", "aborted", 1)
            .is_err()
    );
    assert!(
        store
            .read_identity_recovery_state(10)
            .unwrap()
            .events
            .is_empty()
    );
    inject("barrier");
    assert!(
        store
            .append_session_event("one", "arrival", "", "uncertain", 2)
            .is_err()
    );
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
    assert!(
        store
            .append_session_event("full", "mutation", "", "overflow", 3)
            .is_err()
    );
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
    assert!(
        store
            .apply_identity_intent(
                "legacy-class",
                "failed-class",
                IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                    expected: expected.clone(),
                    record: Some(classified.clone()),
                    genesis: None,
                }))
            )
            .is_err()
    );
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
        assert!(
            store
                .apply_identity_intent(
                    "legacy-class",
                    "reclassify",
                    IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                        expected: committed.clone(),
                        record: Some(changed),
                        genesis: None,
                    }))
                )
                .is_err()
        );
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
    assert!(
        error
            .to_string()
            .contains("private identity store refuses custom Cozo options")
    );
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
    assert!(
        store
            .apply_identity_intent(
                "new",
                "drift",
                IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                    expected: expected.clone(),
                    record: Some(row("new")),
                    genesis: None,
                }))
            )
            .is_err()
    );
    assert_eq!(store.read_identity_revision("new").unwrap(), expected);
}

#[test]
fn root_repair_journal_fences_plan_operation_and_uncertain_phase() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let plan = "a".repeat(64);
    let absent = store.read_identity_revision("session").unwrap();
    store
        .apply_identity_intent(
            "session",
            "original-identity",
            IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                expected: absent,
                record: Some(row("session")),
                genesis: None,
            })),
        )
        .unwrap();
    let identity = store.read_identity_revision("session").unwrap();
    // Caller-authored events with a journal-like kind/content have no access
    // to the private repair fence, including after public event retention.
    store
        .append_session_event(
            "session",
            "root-repair-phase-v1",
            "mut.service.identity-sled.root.repair@v1",
            "{\"phase\":\"identity-committed\"}",
            1,
        )
        .unwrap();
    assert!(store.read_root_repair_journal("session").unwrap().is_none());
    let prepared = store
        .append_root_repair_phase("session", &plan, "original-op", "prepared", &identity, 1)
        .unwrap();
    assert_eq!(prepared.phase, "prepared");
    assert_eq!(
        store.read_root_repair_journal("session").unwrap(),
        Some(prepared)
    );
    assert!(
        store
            .append_root_repair_phase("session", &plan, "new-op", "mount-intent", &identity, 2)
            .is_err()
    );
    assert!(
        store
            .append_root_repair_phase(
                "session",
                &"b".repeat(64),
                "original-op",
                "mount-intent",
                &identity,
                2
            )
            .is_err()
    );
    let intent = store
        .append_root_repair_phase(
            "session",
            &plan,
            "original-op",
            "mount-intent",
            &identity,
            2,
        )
        .unwrap();
    assert_eq!(intent.phase, "mount-intent");
    assert!(
        store
            .append_root_repair_phase(
                "session",
                &plan,
                "original-op",
                "mount-intent",
                &identity,
                3
            )
            .is_err()
    );
    assert!(
        store
            .append_root_repair_phase(
                "session",
                &plan,
                "original-op",
                "pool-import-intent",
                &identity,
                3
            )
            .is_err()
    );
    assert_eq!(
        store.read_root_repair_journal("session").unwrap(),
        Some(intent.clone())
    );

    assert!(
        store
            .apply_identity_intent(
                "session",
                "unrelated-write",
                IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                    expected: identity.clone(),
                    record: Some(row("session")),
                    genesis: None,
                })),
            )
            .is_err()
    );
    assert_eq!(store.read_identity_revision("session").unwrap(), identity);
    assert_eq!(
        store.read_root_repair_journal("session").unwrap(),
        Some(intent)
    );
    store
        .append_root_repair_phase("session", &plan, "original-op", "mounted", &identity, 4)
        .unwrap();
    // Revocation remains available even during a repair. Its revision change
    // prevents the old native intent from publishing a restored SID1 row.
    store
        .apply_identity_intent(
            "session",
            "revocation",
            IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                expected: identity.clone(),
                record: None,
                genesis: None,
            })),
        )
        .unwrap();
    assert!(
        store
            .append_root_repair_phase(
                "session",
                &plan,
                "original-op",
                "pool-import-intent",
                &identity,
                5,
            )
            .is_err()
    );
}

#[test]
fn root_repair_fence_allows_only_its_verified_final_identity_cas() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let absent = store.read_identity_revision("session").unwrap();
    store
        .apply_identity_intent(
            "session",
            "initial",
            IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                expected: absent,
                record: Some(row("session")),
                genesis: None,
            })),
        )
        .unwrap();
    let before = store.read_identity_revision("session").unwrap();
    let plan = "a".repeat(64);
    let stub_receipt = serde_json::json!({
        "status":"already_absent", "session_id":"session",
        "plan_sha256":plan, "operation_id":"repair-op"
    })
    .to_string();
    for (index, phase) in [
        "prepared",
        "mount-intent",
        "mounted",
        "pool-import-intent",
        "pool-ready",
        "stub-quarantine-intent",
        "stub-absent",
        "rebuild-intent",
        "rebuilt",
        "persist-intent",
        "persist-ready",
        "move-intent",
        "moved",
        "verified",
    ]
    .iter()
    .enumerate()
    {
        let row = if *phase == "stub-absent" {
            assert!(
                store
                    .append_root_repair_phase(
                        "session",
                        &plan,
                        "repair-op",
                        phase,
                        &before,
                        index as i64
                    )
                    .is_err()
            );
            let foreign_receipt = stub_receipt.replace("repair-op", "foreign-op");
            assert!(
                store
                    .append_root_repair_stub_absent(
                        "session",
                        &plan,
                        "repair-op",
                        &before,
                        index as i64,
                        &foreign_receipt,
                    )
                    .is_err()
            );
            store
                .append_root_repair_stub_absent(
                    "session",
                    &plan,
                    "repair-op",
                    &before,
                    index as i64,
                    &stub_receipt,
                )
                .unwrap()
        } else {
            store
                .append_root_repair_phase(
                    "session",
                    &plan,
                    "repair-op",
                    phase,
                    &before,
                    index as i64,
                )
                .unwrap()
        };
        assert_eq!(
            row.stub_receipt_json,
            if index >= 6 { &stub_receipt } else { "" }
        );
    }
    assert!(
        store
            .apply_identity_intent(
                "session",
                "other-op",
                IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                    expected: before.clone(),
                    record: Some(row("session")),
                    genesis: None,
                })),
            )
            .is_err()
    );
    store
        .apply_identity_intent(
            "session",
            "repair-op-identity",
            IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                expected: before.clone(),
                record: Some(row("session")),
                genesis: None,
            })),
        )
        .unwrap();
    let committed = store.read_identity_revision("session").unwrap();
    let committed_repair = store
        .append_root_repair_phase(
            "session",
            &plan,
            "repair-op",
            "identity-committed",
            &committed,
            14,
        )
        .unwrap();
    assert_eq!(committed_repair.stub_receipt_json, stub_receipt);
    assert_eq!(
        store
            .read_root_repair_journal("session")
            .unwrap()
            .unwrap()
            .stub_receipt_json,
        committed_repair.stub_receipt_json
    );
}

#[test]
fn absent_seed_provision_journal_fences_native_intent_and_unrelated_identity_cas() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let sid = "new-session";
    let plan = "a".repeat(64);
    let absent = store.read_identity_revision(sid).unwrap();
    store
        .append_session_event(
            sid,
            "seed-provision-phase-v1",
            "caller",
            "{\"phase\":\"identity-committed\"}",
            1,
        )
        .unwrap();
    assert!(store.read_seed_provision_journal(sid).unwrap().is_none());
    store
        .append_seed_provision_phase(sid, &plan, "seed-op", "prepared", &absent, 1)
        .unwrap();
    assert!(
        store
            .append_seed_provision_phase(sid, &plan, "other-op", "leaf-intent", &absent, 2)
            .is_err()
    );
    assert!(
        store
            .append_seed_provision_phase(sid, &"b".repeat(64), "seed-op", "leaf-intent", &absent, 2)
            .is_err()
    );
    store
        .append_seed_provision_phase(sid, &plan, "seed-op", "leaf-intent", &absent, 2)
        .unwrap();
    assert!(
        store
            .apply_identity_intent(
                sid,
                "interloper",
                IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                    expected: absent.clone(),
                    record: Some(row(sid)),
                    genesis: None,
                }))
            )
            .is_err()
    );
    for (index, phase) in [
        "leaf-ready",
        "persist-intent",
        "persist-ready",
        "pool-import-intent",
        "pool-ready",
        "instance-create-intent",
        "instance-ready",
        "verified",
    ]
    .iter()
    .enumerate()
    {
        store
            .append_seed_provision_phase(sid, &plan, "seed-op", phase, &absent, index as i64 + 3)
            .unwrap();
    }
    let mut committed_row = row(sid);
    committed_row.session_genesis = "genesis-anchor".into();
    store
        .apply_identity_intent(
            sid,
            "seed-op-identity",
            IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                expected: absent,
                record: Some(committed_row),
                genesis: Some(genesis(sid)),
            })),
        )
        .unwrap();
    let committed = store.read_identity_revision(sid).unwrap();
    let row = store
        .append_seed_provision_phase(sid, &plan, "seed-op", "identity-committed", &committed, 11)
        .unwrap();
    assert_eq!(row.committed_genesis, "genesis-anchor");
    assert_eq!(store.read_seed_provision_journal(sid).unwrap(), Some(row));
    let mut replaced = committed.record.clone().unwrap();
    replaced.btrfs_device_json = "{\"device_path\":\"/different-leaf\"}".into();
    assert!(
        store
            .apply_identity_intent(
                sid,
                "replace-leaf",
                IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                    expected: committed,
                    record: Some(replaced),
                    genesis: Some(genesis(sid)),
                }))
            )
            .is_err()
    );
}

fn physical_fixture(store: &CozoGraphShuttle) -> IdentityPhysicalRootMigration {
    let (mut record, genesis, _) = legacy_credential_fixture();
    let sid = record.session_id.clone();
    record.btrfs_device_json = serde_json::json!({"device_path":"/old/leaf", "mount_point":"/old/persist", "btrfs_uuid":"old-fsid", "cozo_id":"retained-owner", "attached":true}).to_string();
    let absent = store.read_identity_revision(&sid).unwrap();
    for (at, phase) in [
        "prepared",
        "leaf-intent",
        "leaf-ready",
        "persist-intent",
        "persist-ready",
        "pool-import-intent",
        "pool-ready",
        "instance-create-intent",
        "instance-ready",
        "verified",
    ]
    .iter()
    .enumerate()
    {
        store
            .append_seed_provision_phase(
                &sid,
                &"a".repeat(64),
                "seed-fixture",
                phase,
                &absent,
                at as i64 + 1,
            )
            .unwrap();
    }
    store
        .apply_identity_intent(
            &sid,
            "seed-fixture-identity",
            IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                expected: absent,
                record: Some(record),
                genesis: Some(genesis),
            })),
        )
        .unwrap();
    let expected = store.read_identity_revision(&sid).unwrap();
    let seed = store
        .append_seed_provision_phase(
            &sid,
            &"a".repeat(64),
            "seed-fixture",
            "identity-committed",
            &expected,
            11,
        )
        .unwrap();
    let mut record = expected.record.clone().unwrap();
    record.btrfs_device_json = serde_json::json!({"device_path":"/new/leaf", "mount_point":"/new/persist", "btrfs_uuid":"new-fsid", "cozo_id":"retained-owner", "attached":true}).to_string();
    record.instance_json =
        serde_json::json!({"name":sid, "status":"Stopped", "native_uuid":"new-native"}).to_string();
    record.mutation_index += 1;
    IdentityPhysicalRootMigration {
        expected,
        record,
        expected_root_repair: None,
        expected_seed_provision: Some(seed),
        plan_sha256: "b".repeat(64),
        provenance_json: "{\"receipt\":\"kernel-proof\"}".into(),
    }
}

fn physical_submit(
    store: &CozoGraphShuttle,
    migration: IdentityPhysicalRootMigration,
) -> std::result::Result<IdentityCommit, CozoError> {
    let sid = migration.expected.session_id.clone();
    let operation = format!("physical-migrate-{}", migration.plan_sha256);
    store.apply_identity_intent(
        &sid,
        &operation,
        IdentityIntent::MigratePhysicalRoot(Box::new(migration)),
    )
}

#[test]
fn physical_migration_preserves_authority_history_and_replays() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let migration = physical_fixture(&store);
    let sid = migration.expected.session_id.clone();
    let old = migration.expected.clone();
    let committed = physical_submit(&store, migration).unwrap();
    let receipt = store.read_physical_root_migration(&sid).unwrap().unwrap();
    assert_eq!(receipt.committed_revision, old.revision + 1);
    assert_eq!(committed.current.genesis, old.genesis);
    let mut preserved = committed.current.record.clone().unwrap();
    preserved
        .btrfs_device_json
        .clone_from(&old.record.as_ref().unwrap().btrfs_device_json);
    preserved
        .instance_json
        .clone_from(&old.record.as_ref().unwrap().instance_json);
    preserved.mutation_index -= 1;
    assert_eq!(Some(preserved), old.record);
    let old_seed = store.read_seed_provision_journal(&sid).unwrap();
    let replay = IdentityPhysicalRootMigration {
        expected: old.clone(),
        record: committed.current.record.clone().unwrap(),
        expected_root_repair: None,
        expected_seed_provision: old_seed.clone(),
        plan_sha256: receipt.plan_sha256.clone(),
        provenance_json: receipt.provenance_json.clone(),
    };
    assert!(physical_submit(&store, replay).unwrap().replayed);
    assert_eq!(store.read_seed_provision_journal(&sid).unwrap(), old_seed);
    let history = cozo_run(&store.db, "?[old_btrfs_device_json,old_instance_json,old_journals_json] := *identity_physical_root_history{session_id,old_btrfs_device_json,old_instance_json,old_journals_json}", BTreeMap::new()).unwrap();
    assert_eq!(history.rows.len(), 1);
    assert_eq!(
        string(&history.rows[0], 0).unwrap(),
        old.record.as_ref().unwrap().btrfs_device_json
    );
    assert_eq!(
        string(&history.rows[0], 1).unwrap(),
        old.record.as_ref().unwrap().instance_json
    );
    assert!(
        string(&history.rows[0], 2)
            .unwrap()
            .contains("identity-committed")
    );
    let inventory = store
        .inspect_identity_inventory(crate::IdentityInventoryQuery {
            after_session_id: None,
            limit: 50,
        })
        .unwrap();
    assert_eq!(
        inventory.entries[0].physical_root_migration_revision,
        Some(committed.current.revision)
    );
    let public = serde_json::to_string(&inventory).unwrap();
    assert!(
        !public.contains("kernel-proof")
            && !public.contains("retained-owner")
            && !public.contains("SID1")
    );
    for changed in ["device", "instance"] {
        let mut row = committed.current.record.clone().unwrap();
        if changed == "device" {
            row.btrfs_device_json = "{}".into();
        } else {
            row.instance_json = "{}".into();
        }
        assert!(
            store
                .apply_identity_intent(
                    &sid,
                    "ordinary-overwrite",
                    IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                        expected: committed.current.clone(),
                        record: Some(row),
                        genesis: committed.current.genesis.clone()
                    }))
                )
                .is_err()
        );
    }
}

#[test]
fn physical_migration_rejects_changed_authority_journal_and_overflow() {
    for change in [
        "key",
        "sid1",
        "principal",
        "active",
        "last-seen",
        "cozo",
        "journal",
        "oversize",
        "index",
    ] {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        let mut migration = physical_fixture(&store);
        let old = migration.expected.clone();
        match change {
            "key" => migration.record.wireguard_pubkey = "other".into(),
            "sid1" => migration.record.sealed_id = "other".into(),
            "principal" => migration.record.principal_kind = Some("service".into()),
            "active" => migration.record.active = !migration.record.active,
            "last-seen" => migration.record.last_seen_at += 1,
            "cozo" => {
                migration.record.btrfs_device_json = migration
                    .record
                    .btrfs_device_json
                    .replace("retained-owner", "new-owner")
            }
            "journal" => {
                migration
                    .expected_seed_provision
                    .as_mut()
                    .unwrap()
                    .plan_sha256 = "c".repeat(64)
            }
            "oversize" => migration.provenance_json = "x".repeat(65537),
            "index" => migration.record.mutation_index = i64::MAX,
            _ => unreachable!(),
        }
        assert!(physical_submit(&store, migration).is_err(), "{change}");
        assert_eq!(store.read_identity_revision(&old.session_id).unwrap(), old);
        assert!(
            store
                .read_physical_root_migration(&old.session_id)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn physical_migration_rolls_back_receipt_and_identity_together() {
    for point in ["after_row", "before_revision", "before_commit"] {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        let migration = physical_fixture(&store);
        let old = migration.expected.clone();
        inject(point);
        assert!(physical_submit(&store, migration).is_err());
        assert_eq!(store.read_identity_revision(&old.session_id).unwrap(), old);
        assert!(
            store
                .read_physical_root_migration(&old.session_id)
                .unwrap()
                .is_none()
        );
        assert!(
            cozo_run(
                &store.db,
                "?[session_id] := *identity_physical_root_history{session_id}",
                BTreeMap::new()
            )
            .unwrap()
            .rows
            .is_empty()
        );
    }
}

#[test]
fn physical_migration_refuses_conflict_incomplete_history_and_repeat() {
    for failure in ["revision", "incomplete", "absent"] {
        let store = CozoGraphShuttle::new_in_memory().unwrap();
        let mut migration = physical_fixture(&store);
        let sid = migration.expected.session_id.clone();
        match failure {
            "revision" => {
                store
                    .apply_identity_intent(
                        &sid,
                        "advance",
                        IdentityIntent::AdvanceMutationIndex { event_id: 99 },
                    )
                    .unwrap();
            }
            "incomplete" => {
                cozo_run(&store.db, "?[session_id,phase] <- [[$sid,'verified']] :update identity_seed_provisions {session_id => phase}", sid_params(&sid)).unwrap();
                migration.expected_seed_provision.as_mut().unwrap().phase = "verified".into();
            }
            "absent" => {
                migration.expected_seed_provision = None;
            }
            _ => unreachable!(),
        }
        let before = store.read_identity_revision(&sid).unwrap();
        assert!(physical_submit(&store, migration).is_err(), "{failure}");
        assert_eq!(store.read_identity_revision(&sid).unwrap(), before);
        assert!(store.read_physical_root_migration(&sid).unwrap().is_none());
    }
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let migration = physical_fixture(&store);
    let old = migration.expected.clone();
    let seed = migration.expected_seed_provision.clone();
    let commit = physical_submit(&store, migration).unwrap();
    let mut replay = IdentityPhysicalRootMigration {
        expected: old,
        record: commit.current.record.clone().unwrap(),
        expected_root_repair: None,
        expected_seed_provision: seed.clone(),
        plan_sha256: "b".repeat(64),
        provenance_json: "{\"receipt\":\"kernel-proof\"}".into(),
    };
    replay.expected.operation_id = "fabricated-old-op".into();
    assert!(physical_submit(&store, replay).is_err());
    let mut second = commit.current.record.clone().unwrap();
    second.btrfs_device_json = second.btrfs_device_json.replace("new-fsid", "another-fsid");
    second.mutation_index += 1;
    assert!(
        physical_submit(
            &store,
            IdentityPhysicalRootMigration {
                expected: commit.current,
                record: second,
                expected_root_repair: None,
                expected_seed_provision: seed,
                plan_sha256: "c".repeat(64),
                provenance_json: "{}".into()
            }
        )
        .is_err()
    );
}

#[test]
fn physical_migration_survives_private_persistent_reopen_and_power_updates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private");
    let (sid, receipt) = {
        let store = CozoGraphShuttle::new_persistent_private(path.clone()).unwrap();
        let migration = physical_fixture(&store);
        let sid = migration.expected.session_id.clone();
        let current = physical_submit(&store, migration).unwrap().current;
        let mut record = current.record.clone().unwrap();
        let mut instance: Value = serde_json::from_str(&record.instance_json).unwrap();
        instance["status"] = Value::String("Running".into());
        record.instance_json = instance.to_string();
        store
            .apply_identity_intent(
                &sid,
                "power-observation",
                IdentityIntent::CompareExchange(Box::new(IdentityCompareExchange {
                    expected: current.clone(),
                    record: Some(record),
                    genesis: current.genesis,
                })),
            )
            .unwrap();
        let receipt = store.read_physical_root_migration(&sid).unwrap().unwrap();
        (sid, receipt)
    };
    let store = reopen(&path);
    assert_eq!(
        store.read_physical_root_migration(&sid).unwrap(),
        Some(receipt.clone())
    );
    let row = store.read_identity_revision(&sid).unwrap().record.unwrap();
    assert!(physical_root_instance_binding_matches(
        &receipt.instance_json,
        &row.instance_json
    ));
    assert_eq!(
        store
            .read_seed_provision_journal(&sid)
            .unwrap()
            .unwrap()
            .phase,
        "identity-committed"
    );
}

#[test]
fn physical_migration_power_readback_cannot_relax_native_security() {
    let pinned = serde_json::json!({"name":"identity", "status":"Stopped", "status_code":102,
        "config":{"volatile.last_state.power":"STOPPED", "security.privileged":"false", "volatile.last_state.idmap":"pinned"},
        "expanded_config":{"volatile.last_state.power":"STOPPED", "security.nesting":"false"}});
    let mut running = pinned.clone();
    running["status"] = Value::String("Running".into());
    running["status_code"] = Value::from(103);
    for field in ["config", "expanded_config"] {
        running[field]["volatile.last_state.power"] = Value::String("RUNNING".into());
    }
    assert!(physical_root_instance_binding_matches(
        &pinned.to_string(),
        &running.to_string()
    ));
    for (field, key, replacement) in [
        ("config", "volatile.last_state.power", "STOPPED"),
        ("config", "volatile.last_state.idmap", "changed"),
        ("config", "security.privileged", "true"),
        ("expanded_config", "security.nesting", "true"),
    ] {
        let mut changed = running.clone();
        changed[field][key] = Value::String(replacement.into());
        assert!(
            !physical_root_instance_binding_matches(&pinned.to_string(), &changed.to_string()),
            "{key}"
        );
    }
}
