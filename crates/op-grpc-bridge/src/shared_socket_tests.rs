use super::*;
use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio_stream::StreamExt;

fn fixture() -> (tempfile::TempDir, Arc<SharedListenerController>) {
    let root = tempfile::tempdir().unwrap();
    let controller = SharedListenerController::open_with_lease_dir(
        root.path().join("container.sock"),
        &root.path().join("leases"),
    )
    .unwrap();
    (root, controller)
}

async fn call(controller: &SharedListenerController, method: &str, mut args: Value) -> Value {
    args["path"] = json!(controller.path().to_str().unwrap());
    let value = controller.dispatch(method, &args).await.unwrap();
    let schema = op_plugins::state_plugins::unix_socket::unix_socket_schema_derived();
    let output = serde_json::to_value(schema.methods[method].returns.as_ref().unwrap()).unwrap();
    if let Err(error) = jsonschema::Validator::new(&output)
        .unwrap()
        .validate(&value)
    {
        panic!("invalid {method} output: {error}");
    }
    value
}

async fn accept_native(
    controller: &Arc<SharedListenerController>,
    incoming: &mut ControlledIncoming,
) -> (UnixStream, ManagedUnixStream) {
    let client = UnixStream::connect(controller.path()).await.unwrap();
    let accepted = tokio::time::timeout(Duration::from_secs(1), incoming.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    (client, accepted)
}

#[tokio::test]
async fn bind_listen_backlog_and_accept_use_the_actual_incoming_transport() {
    let (_root, controller) = fixture();
    let mut incoming = controller.take_incoming().unwrap();
    assert!(controller.take_incoming().is_err());
    let bound = call(&controller, "bind", json!({})).await;
    assert_eq!(bound["phase"], "bound");
    assert_eq!(
        fs::symlink_metadata(controller.path()).unwrap().mode() & 0o777,
        SOCKET_MODE
    );
    assert!(std::os::unix::net::UnixStream::connect(controller.path()).is_err());
    let listening = call(&controller, "listen", json!({"backlog":2})).await;
    assert_eq!(listening["requested_backlog"], 2);
    assert_eq!(
        call(&controller, "listen", json!({"backlog":64})).await["requested_backlog"],
        64
    );
    assert!(controller.listen(0).is_err());
    assert!(controller.listen(65536).is_err());
    let client = UnixStream::connect(controller.path()).await.unwrap();
    // Connecting does not invent an event before tonic's sole incoming poll.
    let observed = call(&controller, "accept", json!({"after_sequence":0})).await;
    assert!(observed["event"].is_null());
    assert_eq!(observed["timed_out"], false);
    let accepted = incoming.next().await.unwrap().unwrap();
    let observed = call(&controller, "accept", json!({"after_sequence":0})).await;
    assert_eq!(observed["event"]["sequence"], 1);
    assert_eq!(
        observed["event"]["peer_uid"],
        nix::unistd::geteuid().as_raw()
    );
    assert_eq!(observed["event"]["peer_pid"], std::process::id());
    let mut request = tonic::Request::new(());
    request.extensions_mut().insert(accepted.connect_info());
    assert_eq!(
        extract_peer_cred(&request).unwrap().uid(),
        nix::unistd::geteuid().as_raw()
    );
    assert_eq!(controller.snapshot().unwrap().active_connections, 1);
    drop(accepted);
    drop(client);
    assert_eq!(controller.snapshot().unwrap().active_connections, 0);
    assert_eq!(
        call(&controller, "close", json!({})).await["phase"],
        "closed"
    );
}

#[tokio::test]
async fn force_close_shuts_down_real_connections_and_nonforce_refuses_them() {
    let (_root, controller) = fixture();
    controller.start(8).unwrap();
    let mut incoming = controller.take_incoming().unwrap();
    let (mut client, mut accepted) = accept_native(&controller, &mut incoming).await;
    assert!(
        controller
            .close(false)
            .unwrap_err()
            .to_string()
            .contains("active connections")
    );
    assert_eq!(controller.snapshot().unwrap().phase, "listening");
    let closed = call(&controller, "close", json!({"force":true})).await;
    assert_eq!(closed["active_connections"], 0);
    assert!(!controller.path().exists());
    let mut buffer = [0u8; 4];
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), client.read(&mut buffer))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), accepted.read(&mut buffer))
            .await
            .unwrap()
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn intentional_close_keeps_incoming_pending_and_rebind_resumes_same_acceptor() {
    let (_root, controller) = fixture();
    controller.start(8).unwrap();
    let mut incoming = controller.take_incoming().unwrap();
    controller.close(false).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(20), incoming.next())
            .await
            .is_err()
    );
    controller.bind().unwrap();
    assert_eq!(controller.snapshot().unwrap().binding_generation, 2);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), incoming.next())
            .await
            .is_err()
    );
    controller.listen(12).unwrap();
    let (_client, accepted) = accept_native(&controller, &mut incoming).await;
    assert_eq!(controller.snapshot().unwrap().requested_backlog, Some(12));
    assert_eq!(
        controller
            .observation(0, false)
            .unwrap()
            .event
            .unwrap()
            .binding_generation,
        2
    );
    drop(accepted);
}

#[tokio::test]
async fn unknown_files_symlinks_and_live_sockets_are_never_unlinked() {
    let (root, controller) = fixture();
    fs::write(controller.path(), b"unrelated").unwrap();
    assert!(
        controller
            .start(8)
            .unwrap_err()
            .to_string()
            .contains("no bridge ownership proof")
    );
    assert_eq!(fs::read(controller.path()).unwrap(), b"unrelated");
    fs::remove_file(controller.path()).unwrap();
    let target = root.path().join("target");
    fs::write(&target, b"still here").unwrap();
    std::os::unix::fs::symlink(&target, controller.path()).unwrap();
    assert!(controller.bind().is_err());
    assert!(
        fs::symlink_metadata(controller.path())
            .unwrap()
            .file_type()
            .is_symlink()
    );
    fs::remove_file(controller.path()).unwrap();
    let foreign = std::os::unix::net::UnixListener::bind(controller.path()).unwrap();
    let inode = fs::metadata(controller.path()).unwrap().ino();
    assert!(controller.start(8).is_err());
    assert_eq!(fs::metadata(controller.path()).unwrap().ino(), inode);
    let _client = std::os::unix::net::UnixStream::connect(controller.path()).unwrap();
    drop(foreign);
}

#[tokio::test]
async fn lease_is_exclusive_and_restart_recovers_only_matching_owned_stale_socket() {
    let (root, controller) = fixture();
    let path = controller.path().to_path_buf();
    let leases = root.path().join("leases");
    controller.start(8).unwrap();
    assert!(SharedListenerController::open_with_lease_dir(path.clone(), &leases).is_err());
    // Model a crash: descriptors close, socket pathname and synced proof remain.
    {
        let mut state = controller.lock().unwrap();
        drop(state.listener.take());
        state.ownership = None;
    }
    drop(controller);
    assert!(path.exists());
    let restarted = SharedListenerController::open_with_lease_dir(path.clone(), &leases).unwrap();
    restarted.start(8).unwrap();
    let mut incoming = restarted.take_incoming().unwrap();
    let (_client, _accepted) = accept_native(&restarted, &mut incoming).await;
}

#[tokio::test]
async fn matching_record_does_not_authorize_removing_a_live_listener() {
    let (root, controller) = fixture();
    controller.start(8).unwrap();
    let path = controller.path().to_path_buf();
    let live = {
        let mut state = controller.lock().unwrap();
        state.ownership = None;
        state.listener.take().unwrap()
    };
    drop(controller);
    let inode = fs::metadata(&path).unwrap().ino();
    let restarted =
        SharedListenerController::open_with_lease_dir(path.clone(), &root.path().join("leases"))
            .unwrap();
    assert!(
        restarted
            .start(8)
            .unwrap_err()
            .to_string()
            .contains("live listener")
    );
    assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
    drop(live);
}

#[tokio::test]
async fn close_refuses_to_unlink_a_replacement_socket() {
    let (root, controller) = fixture();
    controller.start(8).unwrap();
    fs::rename(controller.path(), root.path().join("old-owned.sock")).unwrap();
    let replacement = std::os::unix::net::UnixListener::bind(controller.path()).unwrap();
    let inode = fs::metadata(controller.path()).unwrap().ino();
    assert!(
        controller
            .close(false)
            .unwrap_err()
            .to_string()
            .contains("ownership changed")
    );
    assert_eq!(fs::metadata(controller.path()).unwrap().ino(), inode);
    drop(replacement);
}

#[tokio::test]
async fn cursor_boundaries_use_real_accept_events_and_reject_expired_history() {
    let (_root, controller) = fixture();
    controller.start(8).unwrap();
    let mut incoming = controller.take_incoming().unwrap();
    assert!(
        controller
            .observation(1, false)
            .unwrap_err()
            .to_string()
            .contains("ahead")
    );
    for _ in 0..EVENT_CAPACITY + 2 {
        let (_client, _accepted) = accept_native(&controller, &mut incoming).await;
    }
    assert!(
        controller
            .observation(0, false)
            .unwrap_err()
            .to_string()
            .contains("expired")
    );
    assert_eq!(
        controller
            .observation(2, false)
            .unwrap()
            .event
            .unwrap()
            .sequence,
        3
    );
    assert!(
        controller
            .observation(EVENT_CAPACITY as u64 + 3, false)
            .is_err()
    );
    let input = json!({"path":controller.path(),"generation":"previous-bridge","after_sequence":2});
    assert!(
        controller
            .dispatch("accept", &input)
            .await
            .unwrap_err()
            .to_string()
            .contains("generation changed")
    );
}

#[tokio::test]
async fn observer_waits_for_native_accept_and_reports_actual_timeout() {
    let (_root, controller) = fixture();
    controller.start(8).unwrap();
    let mut incoming = controller.take_incoming().unwrap();
    let waiting = controller.clone();
    let task = tokio::spawn(async move {
        call(
            &waiting,
            "accept",
            json!({"after_sequence":0,"timeout_secs":1}),
        )
        .await
    });
    let (_client, _accepted) = accept_native(&controller, &mut incoming).await;
    let output = task.await.unwrap();
    assert_eq!(output["event"]["sequence"], 1);
    assert_eq!(output["timed_out"], false);
    let timed_out = call(
        &controller,
        "accept",
        json!({"after_sequence":1,"timeout_secs":1}),
    )
    .await;
    assert!(timed_out["event"].is_null());
    assert_eq!(timed_out["timed_out"], true);
    assert!(
        controller
            .dispatch("bind", &json!({"path":"/tmp/not-configured"}))
            .await
            .is_err()
    );
    assert!(
        controller
            .dispatch("bind", &json!({"path":controller.path(),"protocol":"raw"}))
            .await
            .is_err()
    );
    assert!(
        controller
            .dispatch(
                "accept",
                &json!({"path":controller.path(),"timeout_secs":61})
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn failed_bind_setup_cleans_its_owned_inode_and_checks_generation_before_bind() {
    let (root, controller) = fixture();
    controller.lock().unwrap().binding_generation = u64::MAX;
    assert!(
        controller
            .bind()
            .unwrap_err()
            .to_string()
            .contains("generation exhausted")
    );
    assert!(!controller.path().exists());
    controller.lock().unwrap().binding_generation = 0;
    // A read-only lease descriptor injects a real persistence error after the
    // kernel bind. No live host filesystem or socket is used by this fixture.
    let readonly_path = root.path().join("readonly-lease");
    fs::write(&readonly_path, b"").unwrap();
    let readonly = Flock::lock(
        File::open(readonly_path).unwrap(),
        FlockArg::LockExclusiveNonblock,
    )
    .unwrap();
    controller.lock().unwrap().lease.file = readonly;
    let error = controller.bind().unwrap_err();
    assert!(
        error
            .to_string()
            .contains("newly bound owned pathname was removed"),
        "{error:#}"
    );
    assert!(!controller.path().exists());
    assert_eq!(controller.snapshot().unwrap().phase, "closed");
}

#[test]
fn bind_rejects_a_parent_writable_by_other_users() {
    let root = tempfile::tempdir().unwrap();
    let controller = SharedListenerController::open_with_lease_dir(
        root.path().join("container.sock"),
        &root.path().join("leases"),
    )
    .unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o777)).unwrap();
    let error = controller.bind().unwrap_err();
    assert!(
        error
            .to_string()
            .contains("socket parent must be service-owned")
    );
    assert!(!controller.path().exists());
}
