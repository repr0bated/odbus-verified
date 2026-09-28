use super::*;
use serde_json::json;
use std::{
    cell::RefCell,
    sync::{Arc, Barrier, mpsc},
    time::Duration,
};

#[derive(Default)]
struct Hooks {
    fail: Option<&'static str>,
    barrier_calls: usize,
    pause: Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>,
}
thread_local! { static HOOKS: RefCell<Hooks> = const { RefCell::new(Hooks { fail: None, barrier_calls: 0, pause: None }) }; }

pub(super) fn checkpoint(point: &str) -> std::result::Result<(), CozoError> {
    let (fail, pause) = HOOKS.with(|hooks| {
        let mut hooks = hooks.borrow_mut();
        if point == "barrier" {
            hooks.barrier_calls += 1;
        }
        let fail = hooks.fail == Some(point);
        if fail {
            hooks.fail = None;
        }
        let pause = if point == "barrier" {
            hooks.pause.take()
        } else {
            None
        };
        (fail, pause)
    });
    if let Some((reached, release)) = pause {
        reached.send(()).unwrap();
        release.recv_timeout(Duration::from_secs(10)).unwrap();
    }
    if fail {
        return Err(error("injected runtime document boundary failure"));
    }
    Ok(())
}
fn reset() {
    HOOKS.with(|hooks| *hooks.borrow_mut() = Hooks::default());
}
fn inject(point: &'static str) {
    HOOKS.with(|hooks| hooks.borrow_mut().fail = Some(point));
}
fn barrier_calls() -> usize {
    HOOKS.with(|hooks| hooks.borrow().barrier_calls)
}
fn committed(result: RuntimeDocumentCommit) -> RuntimeDocumentRecord {
    match result {
        RuntimeDocumentCommit::Committed(row) => row,
        other => panic!("expected committed receipt, received {other:?}"),
    }
}

fn restart_document() -> RuntimeDocumentRecord {
    RuntimeDocumentRecord {
        revision: 1,
        document: json!({"page":{"items":[null,0,false,"x"]}}),
    }
}

#[test]
fn uncertain_debug_keeps_candidate_and_native_values_private() {
    let result = RuntimeDocumentCommit::Uncertain {
        candidate: RuntimeDocumentRecord {
            revision: 17,
            document: json!({"prompt":"private-prompt"}),
        },
        phase: RuntimeDocumentUncertainPhase::Persistence,
        error: CozoError::Other("private-query-bound-value".into()),
    };
    let rendered = format!("{result:?}");
    assert!(rendered.contains("runtime_document_uncertain"));
    assert!(rendered.contains("17"));
    assert!(!rendered.contains("private"));
    assert!(!rendered.contains("prompt"));
}

#[test]
fn native_failures_hide_diagnostics_in_all_standard_error_formats() {
    for code in [
        "runtime_document_read_failed",
        "runtime_document_write_failed",
        "runtime_document_commit_failed",
        "runtime_document_layout_failed",
        "runtime_document_barrier_failed",
        "runtime_document_decode_failed",
    ] {
        let error = native(code, "private-prompt owner-id query-bound-value");
        let CozoError::RuntimeDocument(detail) = &error else {
            panic!("typed private error required")
        };
        assert!(detail.trusted_diagnostic().contains("private-prompt"));
        assert_eq!(detail.code(), code);
        let rendered = format!("{error:?}");
        assert!(!rendered.contains("private-prompt"));
        let error = anyhow::Error::new(error);
        for rendered in [
            error.to_string(),
            format!("{error:?}"),
            format!("{error:#}"),
        ] {
            assert!(rendered.contains(code));
            assert!(!rendered.contains("private-prompt"));
            assert!(!rendered.contains("owner-id"));
            assert!(!rendered.contains("query-bound-value"));
        }
    }
}

#[test]
fn drifted_document_column_rejects_without_leaking_the_bound_private_workspace() {
    reset();
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    store
        .run_query("::remove plugin_runtime_documents", None)
        .unwrap();
    store
        .run_query(
            ":create plugin_runtime_documents {plugin_id: String => revision: Int, document: Int}",
            None,
        )
        .unwrap();
    let error = store
        .compare_exchange_runtime_document(
            "private-owner",
            None,
            json!({"prompt":"PRIVATE_SENTINEL_WORKSPACE_VALUE"}),
        )
        .unwrap_err();
    let CozoError::RuntimeDocument(detail) = &error else {
        panic!("native rejection must be typed")
    };
    assert_eq!(detail.code(), "runtime_document_write_failed");
    // Exercise a real native coercion failure with a private bound value.
    // Native Display may omit its nested cause; confidentiality must not
    // depend on whether that diagnostic happens to include the bound value.
    assert!(!detail.trusted_diagnostic().is_empty());
    let debug = format!("{error:?}");
    let error = anyhow::Error::new(error);
    for rendered in [
        debug,
        error.to_string(),
        format!("{error:?}"),
        format!("{error:#}"),
    ] {
        assert!(!rendered.contains("PRIVATE_SENTINEL_WORKSPACE_VALUE"));
        assert!(!rendered.contains("private-owner"));
        assert!(rendered.contains("runtime_document_write_failed"));
    }
    assert!(
        store
            .run_query("?[plugin_id] := *plugin_runtime_documents{plugin_id}", None)
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(barrier_calls(), 0);
}

#[test]
fn persistent_exact_row_returns_after_barrier_and_survives_reopen() {
    reset();
    if let Some(path) = std::env::var_os("OP_COZO_RUNTIME_RESTART_CHILD_PATH") {
        let marker = std::env::var_os("OP_COZO_RUNTIME_RESTART_CHILD_MARKER")
            .expect("writer completion marker path");
        let store = CozoGraphShuttle::new_persistent(std::path::PathBuf::from(path)).unwrap();
        let expected = restart_document();
        assert_eq!(
            committed(
                store
                    .compare_exchange_runtime_document("ui", None, expected.document.clone())
                    .unwrap()
            ),
            expected
        );
        assert_eq!(barrier_calls(), 1, "writer commit must cross its barrier");
        assert_eq!(store.read_runtime_document("ui").unwrap(), Some(expected));
        std::fs::write(std::path::PathBuf::from(marker), b"completed")
            .expect("mark writer test completion");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runtime");
    let marker = dir.path().join("writer-completed");
    // Cozo's multi_transaction worker can retain a DbInstance briefly after
    // the shuttle drops. A completed child process is a real close boundary.
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "runtime_document_transactions::tests::persistent_exact_row_returns_after_barrier_and_survives_reopen",
        ])
        .env("OP_COZO_RUNTIME_RESTART_CHILD_PATH", &path)
        .env("OP_COZO_RUNTIME_RESTART_CHILD_MARKER", &marker)
        .output()
        .expect("run isolated runtime document writer");
    assert!(
        child.status.success(),
        "isolated runtime document writer failed: {}",
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(marker.is_file(), "isolated writer test did not execute");

    let store = CozoGraphShuttle::new_persistent(path).unwrap();
    assert_eq!(
        store.read_runtime_document_durable("ui").unwrap(),
        Some(restart_document())
    );
    assert_eq!(
        barrier_calls(),
        1,
        "parent durable read must cross its barrier"
    );
}

#[test]
fn independent_clones_compare_complete_expected_row_and_only_one_wins() {
    reset();
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let first = committed(
        store
            .compare_exchange_runtime_document("ui", None, json!(0))
            .unwrap(),
    );
    let wrong = RuntimeDocumentRecord {
        revision: first.revision,
        document: json!("wrong"),
    };
    assert!(
        store
            .compare_exchange_runtime_document("ui", Some(&wrong), json!(1))
            .is_err()
    );
    let go = Arc::new(Barrier::new(3));
    let mut jobs = vec![];
    for value in [1, 2] {
        let store = store.clone();
        let expected = first.clone();
        let go = go.clone();
        jobs.push(std::thread::spawn(move || {
            go.wait();
            store.compare_exchange_runtime_document("ui", Some(&expected), json!(value))
        }));
    }
    go.wait();
    let results = jobs
        .into_iter()
        .map(|job| job.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let row = store.read_runtime_document("ui").unwrap().unwrap();
    assert_eq!(row.revision, 2);
    assert!(row.document == json!(1) || row.document == json!(2));
}

#[test]
fn malformed_rows_and_revision_exhaustion_never_write_or_flush() {
    reset();
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    for (revision, document) in [(-1i64, "{}"), (1, "malformed-json"), (i64::MAX, "{}")] {
        store.run_query("?[plugin_id, revision, document] <- [['ui',$revision,$document]] :put plugin_runtime_documents {plugin_id => revision, document}", Some(json!({"revision":revision,"document":document}))).unwrap();
        let expected = if revision == i64::MAX {
            Some(RuntimeDocumentRecord {
                revision: i64::MAX as u64,
                document: json!({}),
            })
        } else {
            None
        };
        assert!(
            store
                .compare_exchange_runtime_document("ui", expected.as_ref(), json!({"changed":true}))
                .is_err()
        );
        let raw = store.run_query("?[revision, document] := *plugin_runtime_documents{plugin_id, revision, document}, plugin_id='ui'", None).unwrap();
        assert_eq!(raw[0]["revision"], revision);
        assert_eq!(raw[0]["document"], document);
    }
    assert_eq!(barrier_calls(), 0);
}

#[test]
fn failure_before_commit_keeps_old_row_and_never_calls_barrier() {
    reset();
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let previous = committed(
        store
            .compare_exchange_runtime_document("ui", None, json!("old"))
            .unwrap(),
    );
    reset();
    inject("before_commit");
    assert!(
        store
            .compare_exchange_runtime_document("ui", Some(&previous), json!("new"))
            .is_err()
    );
    assert_eq!(store.read_runtime_document("ui").unwrap(), Some(previous));
    assert_eq!(barrier_calls(), 0);
}

#[test]
fn lost_commit_reply_and_failed_barrier_return_uncertain_not_false_rollback() {
    for (point, expected_phase) in [
        ("commit_reply", RuntimeDocumentUncertainPhase::Commit),
        ("barrier", RuntimeDocumentUncertainPhase::Persistence),
    ] {
        reset();
        let dir = tempfile::tempdir().unwrap();
        let store = CozoGraphShuttle::new_persistent(dir.path().join("runtime")).unwrap();
        let previous = committed(
            store
                .compare_exchange_runtime_document("ui", None, json!("old"))
                .unwrap(),
        );
        inject(point);
        let outcome = store
            .compare_exchange_runtime_document("ui", Some(&previous), json!("new"))
            .unwrap();
        let RuntimeDocumentCommit::Uncertain {
            candidate, phase, ..
        } = outcome
        else {
            panic!("uncertainty must not produce successful receipt")
        };
        assert_eq!(phase, expected_phase);
        assert_eq!(candidate.revision, 2);
        assert_eq!(candidate.document, json!("new"));
        // Fault is after an actual native commit. The visible row may differ
        // despite no successful receipt, so rollback/no-effect is not claimed.
        assert_eq!(
            store.read_runtime_document("ui").unwrap(),
            Some(candidate.clone())
        );
        assert!(
            store
                .compare_exchange_runtime_document("ui", Some(&previous), json!("new"))
                .is_err()
        );
        assert_eq!(
            store.read_runtime_document_durable("ui").unwrap(),
            Some(candidate)
        );
    }
}

#[test]
fn explicit_recovery_barrier_failure_yields_no_durable_read_receipt() {
    reset();
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    committed(
        store
            .compare_exchange_runtime_document("ui", None, json!(1))
            .unwrap(),
    );
    inject("barrier");
    assert!(store.read_runtime_document_durable("ui").is_err());
    assert_eq!(
        store.read_runtime_document("ui").unwrap().unwrap().document,
        json!(1)
    );
}

#[test]
fn barrier_holds_shared_owner_until_persistence_finishes() {
    let dir = tempfile::tempdir().unwrap();
    let store = CozoGraphShuttle::new_persistent(dir.path().join("runtime")).unwrap();
    let writer = store.clone();
    let (entered_tx, entered) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let first = std::thread::spawn(move || {
        reset();
        HOOKS.with(|hooks| hooks.borrow_mut().pause = Some((entered_tx, release_rx)));
        writer.compare_exchange_runtime_document("ui", None, json!(1))
    });
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(store.runtime_document_transaction.try_lock().is_err());
    let second_store = store.clone();
    let (finished, finish_rx) = mpsc::channel();
    let second = std::thread::spawn(move || {
        let row = second_store.read_runtime_document("ui").unwrap().unwrap();
        let result = second_store.compare_exchange_runtime_document("ui", Some(&row), json!(2));
        finished.send(()).unwrap();
        result
    });
    assert!(matches!(
        finish_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    release.send(()).unwrap();
    assert_eq!(committed(first.join().unwrap().unwrap()).revision, 1);
    assert_eq!(committed(second.join().unwrap().unwrap()).revision, 2);
}

#[test]
fn options_and_replaced_data_directory_fail_before_native_write() {
    reset();
    for corruption in ["options", "replacement", "symlink"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("runtime");
        let store = CozoGraphShuttle::new_persistent(root.clone()).unwrap();
        match corruption {
            "options" => std::fs::write(root.join("options"), b"untrusted-layout").unwrap(),
            "replacement" => {
                std::fs::rename(root.join("data"), root.join("old-data")).unwrap();
                std::fs::create_dir(root.join("data")).unwrap();
            }
            "symlink" => {
                std::fs::rename(root.join("data"), root.join("old-data")).unwrap();
                std::os::unix::fs::symlink(root.join("old-data"), root.join("data")).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            store
                .compare_exchange_runtime_document("ui", None, json!(1))
                .is_err()
        );
        assert!(
            store
                .run_query("?[plugin_id] := *plugin_runtime_documents{plugin_id}", None)
                .unwrap()
                .as_array()
                .unwrap()
                .is_empty()
        );
        // Restore paths before native handle drop, keeping fixtures isolated.
        if corruption != "options" {
            if corruption == "replacement" {
                std::fs::remove_dir(root.join("data")).unwrap();
            } else {
                std::fs::remove_file(root.join("data")).unwrap();
            }
            std::fs::rename(root.join("old-data"), root.join("data")).unwrap();
        }
    }
    assert_eq!(barrier_calls(), 0);
}
