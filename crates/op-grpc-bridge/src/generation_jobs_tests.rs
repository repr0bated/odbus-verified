// Tungstenite fixes the handshake callback's error type to an HTTP response.
#![allow(clippy::result_large_err)]

use super::*;
use crate::{
    grpc_server::{OperationGrpcServer, DECLARED_CAPABILITY_HEADER},
    human_principal_dispatch::tests::{
        pk, register, temp_cozo, temp_shm_with_schemas, test_engine, TempCozo, TempShm,
    },
    identity_sled_dispatch::{tests::write_identity, IdentitySledStore},
    oracle_assertion::HumanPrincipalIdentity,
    proto::{state_sync_server::StateSync, GetStateRequest, SubscribeRequest},
    tched_router_runtime::TchedRouterRuntimeClient,
};
use futures::{SinkExt, StreamExt};
use op_plugins::state_plugins::{
    human_principal::HumanPrincipalPlugin, identity_sled::IdentitySledPlugin,
    large_language_model::LargeLanguageModelPlugin, tched_router::TchedRouterPlugin,
};
use op_state::StatePlugin;
use serde_json::json;
use std::time::Duration;
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{
        handshake::server::{Request as WsRequest, Response as WsResponse},
        Message,
    },
};
use tonic::Request;

const LIMIT: Duration = Duration::from_secs(20);
struct GrantsEnvironment(Option<std::ffi::OsString>);
impl Drop for GrantsEnvironment {
    fn drop(&mut self) {
        match self.0.take() {
            // FIXME: Audit that the environment access only happens in single-threaded code.
            Some(path) => unsafe { std::env::set_var("OP_GRANTS_PATH", path) },
            // FIXME: Audit that the environment access only happens in single-threaded code.
            None => unsafe { std::env::remove_var("OP_GRANTS_PATH") },
        }
    }
}
struct NativeFixture {
    endpoint: String,
    frames: tokio::sync::mpsc::Sender<Option<Value>>,
    identity_header: tokio::sync::oneshot::Receiver<String>,
    prompt: tokio::sync::oneshot::Receiver<Value>,
    closed: tokio::sync::oneshot::Receiver<()>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for NativeFixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl NativeFixture {
    async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (frames, mut received) = tokio::sync::mpsc::channel::<Option<Value>>(128);
        let (identity_tx, identity_header) = tokio::sync::oneshot::channel();
        let (prompt_tx, prompt) = tokio::sync::oneshot::channel();
        let (closed_tx, closed) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_hdr_async(
                stream,
                move |request: &WsRequest, mut response: WsResponse| {
                    assert_eq!(request.uri().path(), "/ws/chat");
                    assert!(request
                        .uri()
                        .query()
                        .unwrap()
                        .contains("agent_alias=fixture"));
                    assert!(request
                        .uri()
                        .query()
                        .unwrap()
                        .contains("stateless_generation=1"));
                    assert_eq!(request.headers()["authorization"], "Bearer fixture-token");
                    let forwarded = request.headers()["x-opdbus-sealed-id-bin"]
                        .to_str()
                        .expect("forwarded SID1 header must be text");
                    assert!(
                        op_identity::sealed_id::SealedId::from_inline_ref(&format!(
                            "{}{}",
                            op_identity::sealed_id::INLINE_PREFIX,
                            forwarded
                        ))
                        .is_ok(),
                        "forwarded identity must be an authored SID1"
                    );
                    let _ = identity_tx.send(forwarded.to_owned());
                    response.headers_mut().insert(
                        "Sec-WebSocket-Protocol",
                        "tched_router.generation.v1".parse().unwrap(),
                    );
                    Ok(response)
                },
            )
            .await
            .unwrap();
            let connect = socket.next().await.unwrap().unwrap();
            let connect: Value = serde_json::from_str(connect.to_text().unwrap()).unwrap();
            let session = connect["session_id"].as_str().unwrap();
            socket.send(Message::Text(json!({"type":"session_start","session_id":session,"resumed":false,"message_count":0,"stateless_generation":1}).to_string())).await.unwrap();
            socket.send(Message::Text(json!({"type":"connected","stateless_generation":1,"provider":"fixture.direct","model":"fixture-model","identity_authenticated":"SID1"}).to_string())).await.unwrap();
            let prompt = socket.next().await.unwrap().unwrap();
            assert!(
                matches!(&prompt, Message::Text(_)),
                "native prompt must be a text frame"
            );
            let prompt: Value = serde_json::from_str(prompt.to_text().unwrap()).unwrap();
            assert_eq!(prompt["type"], "message");
            let _ = prompt_tx.send(prompt);
            loop {
                tokio::select! {
                    frame=received.recv()=>match frame {Some(Some(frame))=>if socket.send(Message::Text(frame.to_string())).await.is_err(){break},_=>break},
                    message=socket.next()=>match message {Some(Ok(Message::Ping(data)))=>{let _=socket.send(Message::Pong(data)).await;},Some(Ok(Message::Close(_)))|None|Some(Err(_))=>break,_=>{}}
                }
            }
            let _ = socket.close(None).await;
            let _ = closed_tx.send(());
        });
        Self {
            endpoint,
            frames,
            identity_header,
            prompt,
            closed,
            task,
        }
    }
    async fn chunk(&self, text: &str) {
        self.frames
            .send(Some(json!({"type":"chunk","content":text})))
            .await
            .unwrap();
    }
    async fn done(&self, text: &str) {
        self.frames.send(Some(json!({"type":"done","full_response":text,"provider":"fixture.direct","model":"fixture-model"}))).await.unwrap();
    }
}
struct Fixture {
    engine: Arc<MutationEngine>,
    server: OperationGrpcServer,
    first: HumanPrincipalIdentity,
    second: HumanPrincipalIdentity,
    documents: Arc<RuntimeDocuments>,
    private: PrivateProjectionDirectory,
    catalog: PathBuf,
    grants: PathBuf,
    _environment: GrantsEnvironment,
    _root: tempfile::TempDir,
    _shm: TempShm,
    _cozo: TempCozo,
}
impl Fixture {
    async fn new(endpoint: String) -> Self {
        let cozo = temp_cozo();
        let shm = temp_shm_with_schemas(&[
            HumanPrincipalPlugin::new().schema().unwrap(),
            IdentitySledPlugin::new().schema().unwrap(),
            TchedRouterPlugin::new().schema().unwrap(),
            LargeLanguageModelPlugin::new().schema().unwrap(),
        ]);
        let engine = test_engine();
        engine
            .install_identity_sled_store_for_test(IdentitySledStore::new(
                op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
            ))
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        let documents = Arc::new(RuntimeDocuments::new(
            op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
        ));
        let private =
            PrivateProjectionDirectory::prepare(&root.path().join("generations")).unwrap();
        let catalog = crate::sealed_schema_reader::blob_catalog_dir_from_env();
        engine
            .configure_generation_jobs(
                private.clone(),
                documents.clone(),
                catalog.clone(),
                Arc::new(TchedRouterRuntimeClient::new(
                    endpoint,
                    "ignored-global-default".into(),
                    Some("fixture-token".into()),
                )),
            )
            .unwrap();
        let mut ids = Vec::new();
        for (byte, name) in [(0xD1, "generation-first"), (0xD2, "generation-second")] {
            let key = pk(byte);
            register(&key, name).await.unwrap();
            let sled = write_identity(&engine, &key).await.unwrap();
            ids.push(HumanPrincipalIdentity {
                principal_id: op_identity::session::derive_principal_id(&key),
                human_pubkey: key,
                session_id: sled.session_id,
                session_genesis: sled.genesis.unwrap(),
                expires_at: 0,
            });
        }
        let grants = root.path().join("grants.json");
        let environment = GrantsEnvironment(std::env::var_os("OP_GRANTS_PATH"));
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_GRANTS_PATH", &grants) };
        let fixture = Self {
            server: OperationGrpcServer::new(engine.clone()),
            engine,
            first: ids.remove(0),
            second: ids.remove(0),
            documents,
            private,
            catalog,
            grants,
            _environment: environment,
            _root: root,
            _shm: shm,
            _cozo: cozo,
        };
        fixture.grants(true, true);
        fixture
    }
    fn grants(&self, start: bool, read: bool) {
        let mut caps = vec![
            CANCEL_CAPABILITY,
            DELETE_CAPABILITY,
            RECOVER_CAPABILITY,
            "llm.invoke",
        ];
        if start {
            caps.push(START_CAPABILITY)
        }
        if read {
            caps.push(READ_CAPABILITY)
        }
        std::fs::write(&self.grants,serde_json::to_vec(&json!({(self.first.principal_id.clone()):{"capabilities":caps},
            (self.second.principal_id.clone()):{"capabilities":[START_CAPABILITY,READ_CAPABILITY,CANCEL_CAPABILITY,DELETE_CAPABILITY,RECOVER_CAPABILITY,"llm.invoke"]}})).unwrap()).unwrap();
    }
    fn owner(&self, who: &HumanPrincipalIdentity) -> GenerationOwner {
        GenerationOwner::new(who.principal_id.clone(), who.session_id.clone()).unwrap()
    }
    fn caller(&self, who: &HumanPrincipalIdentity) -> GenerationCaller {
        GenerationCaller {
            actor: who.principal_id.clone(),
            session: who.session_id.clone(),
            genesis: who.session_genesis.clone(),
        }
    }
    async fn call(&self, who: &HumanPrincipalIdentity, method: &str, args: Value) -> Result<Value> {
        let schema = TchedRouterPlugin::new().schema().unwrap();
        let cap = schema.methods[method].required_capability.as_deref();
        Ok(self
            .engine
            .dispatch_method_call_with_identity(
                PLUGIN,
                method,
                &args.to_string(),
                cap,
                &who.principal_id,
                Some(&who.session_id),
                Some(&who.session_genesis),
            )
            .await?["result"]
            .clone())
    }
    async fn activate(&self, who: &HumanPrincipalIdentity) {
        self.call(who, "recover_generation_jobs", json!({}))
            .await
            .unwrap();
    }
    fn request<T>(&self, body: T, who: &HumanPrincipalIdentity, cap: Option<&str>) -> Request<T> {
        let mut req = Request::new(body);
        req.extensions_mut().insert(who.clone());
        if let Some(cap) = cap {
            req.metadata_mut()
                .insert(DECLARED_CAPABILITY_HEADER, cap.parse().unwrap());
        }
        req
    }
    async fn get(
        &self,
        who: &HumanPrincipalIdentity,
        suffix: &str,
    ) -> Result<crate::proto::GetStateResponse, tonic::Status> {
        self.server
            .get_state(self.request(
                GetStateRequest {
                    plugin_id: PLUGIN.into(),
                    object_path: path(suffix),
                },
                who,
                Some(READ_CAPABILITY),
            ))
            .await
            .map(tonic::Response::into_inner)
    }
    async fn ticket(&self) -> GenerationSubscription {
        self.engine
            .generation_jobs()
            .unwrap()
            .subscribe_jobs(&self.engine, self.caller(&self.first), READ_CAPABILITY)
            .await
            .unwrap()
    }
    async fn wait_status(&self, id: &str, status: GenerationStatus) -> GenerationJob {
        self.wait_status_with_limit(id, status, LIMIT).await
    }
    async fn wait_status_with_limit(
        &self,
        id: &str,
        status: GenerationStatus,
        limit: Duration,
    ) -> GenerationJob {
        self.wait_job(id, limit, |job| job.status == status).await
    }
    async fn wait_committed_chunks(&self, id: &str, count: u32) -> GenerationJob {
        self.wait_job(id, LIMIT, |job| job.committed_chunks >= count)
            .await
    }
    async fn wait_job(
        &self,
        id: &str,
        limit: Duration,
        predicate: impl Fn(&GenerationJob) -> bool,
    ) -> GenerationJob {
        tokio::time::timeout(limit, async {
            let publisher = self.engine.generation_jobs().unwrap();
            let mut ticket = self.ticket().await;
            let mut receipt = ticket.initial.clone();
            loop {
                if let Some(job) = receipt
                    .state
                    .as_ref()
                    .and_then(|s| s.generation_jobs.get(id))
                    && predicate(job)
                {
                    return job.clone();
                }
                ticket.receiver.recv().await.unwrap();
                receipt = publisher
                    .refresh_subscription(&self.engine, &ticket)
                    .await
                    .unwrap();
            }
        })
        .await
        .unwrap()
    }
}
fn path(suffix: &str) -> String {
    format!("/org/opdbus/v1/plugins/tched_router/{suffix}")
}
fn string_value(response: crate::proto::GetStateResponse) -> String {
    match response.value.unwrap().kind.unwrap() {
        prost_types::value::Kind::StringValue(text) => text,
        _ => panic!("scalar descendant changed type"),
    }
}

#[tokio::test]
async fn llm_stream_facade_and_job_listing_use_private_committed_state() {
    let mut native = NativeFixture::new().await;
    let fixture = Fixture::new(native.endpoint.clone()).await;
    fixture.activate(&fixture.first).await;
    fixture.activate(&fixture.second).await;
    let schema = LargeLanguageModelPlugin::new().schema().unwrap();
    let started = fixture
        .engine
        .dispatch_method_call_with_identity(
            "large_language_model",
            "stream_generate",
            &json!({"agent_alias":"fixture","prompt":"PRIVATE_FACADE_PROMPT"}).to_string(),
            schema.methods["stream_generate"]
                .required_capability
                .as_deref(),
            &fixture.first.principal_id,
            Some(&fixture.first.session_id),
            Some(&fixture.first.session_genesis),
        )
        .await
        .unwrap();
    let id = started["result"]["job"]["job_id"].as_str().unwrap();
    let prompt = tokio::time::timeout(LIMIT, &mut native.prompt)
        .await
        .unwrap()
        .unwrap();
    let forwarded = tokio::time::timeout(LIMIT, &mut native.identity_header)
        .await
        .unwrap()
        .unwrap();
    let expected = crate::native_sled_identity::credential_for(
        &fixture.first.principal_id,
        &fixture.first.session_id,
        &fixture.first.session_genesis,
    )
    .unwrap();
    assert!(
        forwarded == expected,
        "native request forwarded a different SID1"
    );
    assert_eq!(prompt["type"], "message");
    assert_eq!(prompt["content"], "PRIVATE_FACADE_PROMPT");
    native.chunk("facade output").await;
    let running = fixture.wait_committed_chunks(id, 1).await;
    assert_eq!(running.output, "facade output");
    assert_eq!(
        string_value(
            fixture
                .get(&fixture.first, &format!("generation_jobs/{id}/output"))
                .await
                .unwrap()
        ),
        "facade output"
    );
    native.done("facade output").await;
    let completed = fixture.wait_status(id, GenerationStatus::Completed).await;
    assert_eq!(completed.output, "facade output");
    let listed = fixture
        .call(&fixture.first, "get_generation_jobs", json!({}))
        .await
        .unwrap();
    let jobs = listed["state"]["generation_jobs"].as_object().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[id], serde_json::to_value(&completed).unwrap());
    let other = fixture
        .call(&fixture.second, "get_generation_jobs", json!({}))
        .await
        .unwrap();
    let other_workspace: GenerationWorkspaceOutput = serde_json::from_value(other.clone()).unwrap();
    assert!(other_workspace
        .state
        .is_none_or(|state| state.generation_jobs.is_empty()));
    assert!(!other.to_string().contains("PRIVATE_FACADE_PROMPT"));
    assert!(!other.to_string().contains("facade output"));
    fixture
        .call(
            &fixture.first,
            "delete_generation_job",
            json!({
                "job_id":id,"expected_revision":completed.revision
            }),
        )
        .await
        .unwrap();
    let deleted = fixture
        .call(&fixture.first, "get_generation_jobs", json!({}))
        .await
        .unwrap();
    assert!(deleted["state"]["generation_jobs"]
        .as_object()
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn actual_handlers_require_owner_grants_and_serve_only_committed_children() {
    let mut native = NativeFixture::new().await;
    let fixture = Fixture::new(native.endpoint.clone()).await;
    assert!(fixture
        .get(&fixture.first, "generation_jobs")
        .await
        .is_err());
    let missing = fixture.request(
        SubscribeRequest {
            plugin_ids: vec![PLUGIN.into()],
            path_patterns: vec![path("generation_jobs")],
            ..Default::default()
        },
        &fixture.first,
        None,
    );
    assert_eq!(
        fixture
            .server
            .subscribe(missing)
            .await
            .err()
            .unwrap()
            .code(),
        tonic::Code::PermissionDenied
    );
    fixture.activate(&fixture.first).await;
    fixture.activate(&fixture.second).await;
    let mut public_wakes = fixture.engine.change_tx().subscribe();
    let started = fixture
        .call(
            &fixture.first,
            "start_generation",
            json!({"agent_alias":"fixture","prompt":"PRIVATE_NATIVE_PROMPT"}),
        )
        .await
        .unwrap();
    let id = started["job"]["job_id"].as_str().unwrap();
    let prompt = tokio::time::timeout(LIMIT, &mut native.prompt)
        .await
        .unwrap()
        .unwrap();
    assert!(prompt.to_string().contains("PRIVATE_NATIVE_PROMPT"));
    native.chunk("PRIVATE_CHUNK").await;
    let mut ticket = fixture.ticket().await;
    tokio::time::timeout(LIMIT, async {
        loop {
            let receipt = fixture
                .engine
                .generation_jobs()
                .unwrap()
                .refresh_subscription(&fixture.engine, &ticket)
                .await
                .unwrap();
            if receipt.state.as_ref().unwrap().generation_jobs[id].output == "PRIVATE_CHUNK" {
                let job = &receipt.state.as_ref().unwrap().generation_jobs[id];
                assert_eq!(job.status, GenerationStatus::Running);
                assert_eq!(job.committed_chunks, 1);
                break;
            }
            ticket.receiver.recv().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(
        string_value(
            fixture
                .get(&fixture.first, &format!("generation_jobs/{id}/output"))
                .await
                .unwrap()
        ),
        "PRIVATE_CHUNK"
    );
    assert_eq!(
        fixture
            .get(&fixture.second, &format!("generation_jobs/{id}"))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::NotFound
    );
    let mut forged = fixture.first.clone();
    forged.session_id = fixture.second.session_id.clone();
    assert_eq!(
        fixture
            .get(&forged, "generation_jobs")
            .await
            .unwrap_err()
            .code(),
        tonic::Code::Unauthenticated
    );
    assert!(fixture.engine.get_state(PLUGIN).await.is_none());
    let public = crate::state_stream::read_public_state_snapshot(&[PLUGIN.into()])
        .await
        .unwrap();
    assert!(!format!("{:?}", public.plugins[PLUGIN].state).contains("PRIVATE_CHUNK"));
    // Canonical mutation auditing is digest-only, even though its result is private.
    while let Ok(wake) = public_wakes.try_recv() {
        let value = simd_json::to_string(&wake.new_value).unwrap();
        assert!(!value.contains("PRIVATE_NATIVE_PROMPT"));
        assert!(!value.contains("PRIVATE_CHUNK"));
    }
    native.done("PRIVATE_CHUNK").await;
    let job = fixture.wait_status(id, GenerationStatus::Completed).await;
    assert_eq!(job.output, "PRIVATE_CHUNK");
    assert_eq!(job.reported_provider.as_deref(), Some("fixture.direct"));
    fixture
        .call(
            &fixture.first,
            "delete_generation_job",
            json!({"job_id":id,"expected_revision":job.revision}),
        )
        .await
        .unwrap();
    assert_eq!(
        fixture
            .get(&fixture.first, &format!("generation_jobs/{id}"))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::NotFound
    );
    fixture.grants(true, false);
    assert_eq!(
        fixture
            .get(&fixture.first, "generation_jobs")
            .await
            .unwrap_err()
            .code(),
        tonic::Code::PermissionDenied
    );
}

#[tokio::test]
async fn cancelling_the_caller_does_not_cancel_owned_admission_or_publication() {
    let mut native = NativeFixture::new().await;
    let fixture = Fixture::new(native.endpoint.clone()).await;
    fixture.activate(&fixture.first).await;
    let publisher = fixture.engine.generation_jobs().unwrap();
    let owner = fixture.owner(&fixture.first);
    let slot = publisher.slot(&owner).unwrap();
    let guard = slot.operation.lock().await;
    let schema = TchedRouterPlugin::new().schema().unwrap();
    let args = json!({"agent_alias":"fixture","prompt":"caller-dropped"});
    let publisher = publisher.clone();
    let engine = fixture.engine.clone();
    let identity = fixture.first.clone();
    let call = tokio::spawn(async move {
        publisher
            .dispatch(
                &engine,
                &schema,
                "start_generation",
                &args,
                &identity.principal_id,
                Some(&identity.session_id),
                Some(&identity.session_genesis),
                None,
            )
            .await
    });
    tokio::time::timeout(LIMIT, async {
        while slot.calls.available_permits() == MAX_OWNER_CALLS {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    // The semaphore proves pre-task admission finished and the accepted owned
    // call is waiting for the lock, rather than cancelling an unpolled future.
    call.abort();
    let _ = call.await;
    drop(guard);
    tokio::time::timeout(LIMIT, &mut native.prompt)
        .await
        .unwrap()
        .unwrap();
    native.chunk("survives").await;
    native.done("survives").await;
    let id = fixture
        .ticket()
        .await
        .initial
        .state
        .unwrap()
        .generation_jobs
        .into_keys()
        .next()
        .unwrap();
    assert_eq!(
        fixture
            .wait_status(&id, GenerationStatus::Completed)
            .await
            .output,
        "survives"
    );
}

#[tokio::test]
async fn cancel_closes_native_transport_and_disconnect_is_a_real_terminal_failure() {
    let mut native = NativeFixture::new().await;
    let fixture = Fixture::new(native.endpoint.clone()).await;
    fixture.activate(&fixture.first).await;
    let started = fixture
        .call(
            &fixture.first,
            "start_generation",
            json!({"agent_alias":"fixture","prompt":"cancel"}),
        )
        .await
        .unwrap();
    let id = started["job"]["job_id"].as_str().unwrap();
    tokio::time::timeout(LIMIT, &mut native.prompt)
        .await
        .unwrap()
        .unwrap();
    let job = fixture.wait_status(id, GenerationStatus::Running).await;
    fixture
        .call(
            &fixture.first,
            "cancel_generation",
            json!({"job_id":id,"expected_revision":job.revision}),
        )
        .await
        .unwrap();
    fixture.wait_status(id, GenerationStatus::Cancelled).await;
    tokio::time::timeout(LIMIT, &mut native.closed)
        .await
        .unwrap()
        .unwrap();
    // A second independent fixture exercises an actual socket close, not a fake Done.
    drop(fixture);
    drop(native);
    let mut native = NativeFixture::new().await;
    let fixture = Fixture::new(native.endpoint.clone()).await;
    fixture.activate(&fixture.first).await;
    let started = fixture
        .call(
            &fixture.first,
            "start_generation",
            json!({"agent_alias":"fixture","prompt":"disconnect"}),
        )
        .await
        .unwrap();
    let id = started["job"]["job_id"].as_str().unwrap();
    tokio::time::timeout(LIMIT, &mut native.prompt)
        .await
        .unwrap()
        .unwrap();
    native.frames.send(None).await.unwrap();
    let job = fixture.wait_status(id, GenerationStatus::Failed).await;
    assert_eq!(job.failure, Some(GenerationFailure::NativeDisconnected));
}

#[tokio::test]
async fn grant_withdrawal_stops_native_and_retains_pending_until_exact_recovery() {
    let mut native = NativeFixture::new().await;
    let fixture = Fixture::new(native.endpoint.clone()).await;
    fixture.activate(&fixture.first).await;
    let started = fixture
        .call(
            &fixture.first,
            "start_generation",
            json!({"agent_alias":"fixture","prompt":"withdraw"}),
        )
        .await
        .unwrap();
    let id = started["job"]["job_id"].as_str().unwrap();
    tokio::time::timeout(LIMIT, &mut native.prompt)
        .await
        .unwrap()
        .unwrap();
    fixture.grants(false, true);
    native.chunk("must-never-publish").await;
    tokio::time::timeout(LIMIT, &mut native.closed)
        .await
        .unwrap()
        .unwrap();
    let publisher = fixture.engine.generation_jobs().unwrap();
    let slot = publisher.slot(&fixture.owner(&fixture.first)).unwrap();
    let _guard = slot.operation.lock().await;
    assert!(!slot.activated.load(Ordering::Acquire));
    let owner = fixture
        .private
        .open_child(fixture.owner(&fixture.first).selector())
        .unwrap()
        .unwrap();
    assert!(owner.pending_document().unwrap().is_some());
    assert!(owner.read_snapshot(&[PLUGIN.into()]).is_err());
    drop(_guard);
    // Registration completion is an owned lifecycle event, not a state poll.
    tokio::time::timeout(LIMIT, async {
        loop {
            let notified = slot.worker_finished.notified();
            if slot.workers.lock().unwrap().is_empty() {
                break;
            }
            notified.await;
        }
    })
    .await
    .unwrap();
    fixture.activate(&fixture.first).await;
    let recovered = fixture
        .call(&fixture.first, "get_generation_job", json!({"job_id":id}))
        .await
        .unwrap();
    assert_eq!(recovered["job"]["status"], "interrupted");
    assert_eq!(recovered["job"]["output"], "");
}

#[tokio::test]
async fn private_pending_failure_blocks_reads_and_recovery_uses_durable_state() {
    let native = NativeFixture::new().await;
    let fixture = Fixture::new(native.endpoint.clone()).await;
    fixture.activate(&fixture.first).await;
    let owner = fixture.owner(&fixture.first);
    let directory = fixture
        .private
        .open_child(owner.selector())
        .unwrap()
        .unwrap();
    let methods = GenerationMethods::new(fixture.documents.clone());
    let handle = tokio::runtime::Handle::current();
    let hash = TchedRouterPlugin::new().schema().unwrap();
    let pins = GenerationPins::load(
        &fixture.catalog,
        Some(&hash),
        "recover_generation_jobs",
        RECOVER_CAPABILITY,
        None,
    )
    .unwrap();
    let owner_copy = owner.clone();
    let schema_hash = pins.document.schema_hash.clone();
    let result = tokio::task::spawn_blocking(move || {
        directory.commit_document_durable::<()>(PLUGIN, "fixture-uncertain", move |_| {
            handle.block_on(methods.create(
                &owner_copy,
                &StartGenerationInput {
                    agent_alias: "fixture".into(),
                    prompt: "never-replay".into(),
                },
                "interrupted",
                &schema_hash,
            ))?;
            anyhow::bail!("injected publication uncertainty")
        })
    })
    .await
    .unwrap()
    .unwrap();
    assert!(matches!(result, PrivateDocumentPublication::Pending { .. }));
    assert!(fixture
        .get(&fixture.first, "generation_jobs")
        .await
        .is_err());
    fixture.activate(&fixture.first).await;
    let recovered = fixture
        .call(
            &fixture.first,
            "get_generation_job",
            json!({"job_id":"interrupted"}),
        )
        .await
        .unwrap();
    assert_eq!(recovered["job"]["status"], "interrupted");
    assert_eq!(recovered["job"]["failure"], "process_interrupted");
    assert!(!serde_json::to_string(&recovered)
        .unwrap()
        .contains("never-replay"));
}

#[tokio::test]
async fn actual_subscription_lag_rereads_shm_and_retains_deleted_child_tombstones() {
    let mut native = NativeFixture::new().await;
    let fixture = Fixture::new(native.endpoint.clone()).await;
    fixture.activate(&fixture.first).await;
    let started = fixture
        .call(
            &fixture.first,
            "start_generation",
            json!({"agent_alias":"fixture","prompt":"lag"}),
        )
        .await
        .unwrap();
    let id = started["job"]["job_id"].as_str().unwrap();
    tokio::time::timeout(LIMIT, &mut native.prompt)
        .await
        .unwrap()
        .unwrap();
    let selected = path(&format!("generation_jobs/{id}"));
    let request = fixture.request(
        SubscribeRequest {
            plugin_ids: vec![PLUGIN.into()],
            path_patterns: vec![selected.clone()],
            include_initial_state: true,
            ..Default::default()
        },
        &fixture.first,
        Some(READ_CAPABILITY),
    );
    let mut stream = fixture
        .server
        .subscribe(request)
        .await
        .unwrap()
        .into_inner();
    let initial = tokio::time::timeout(LIMIT, stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(initial.object_path, selected);
    assert!(initial.state_generation > 0);
    // Force 40 ACTUAL publications, not merely 40 fragments which the worker
    // may correctly coalesce. The tested StateSync receiver remains unpolled;
    // this independent committed-SHM observer gates each next native fragment.
    tokio::time::timeout(Duration::from_secs(120), async {
        for count in 1..=40u32 {
            native.chunk("x").await;
            let committed = fixture.wait_committed_chunks(id, count).await;
            assert_eq!(committed.committed_chunks, count);
            assert_eq!(committed.revision, u64::from(count) + 2);
        }
    })
    .await
    .unwrap();
    native.done(&"x".repeat(40)).await;
    let complete = fixture.wait_status(id, GenerationStatus::Completed).await;
    fixture
        .call(
            &fixture.first,
            "delete_generation_job",
            json!({"job_id":id,"expected_revision":complete.revision}),
        )
        .await
        .unwrap();
    // The receiver overflowed, but the previously exposed exact child still
    // receives a tombstone from the complete committed current snapshot.
    let removed = tokio::time::timeout(LIMIT, stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(removed.object_path, selected);
    assert_eq!(
        removed.change_type,
        crate::proto::ChangeType::ObjectRemoved as i32
    );
    assert_eq!(
        removed.frame_kind,
        crate::proto::StateFrameKind::InitialState as i32
    );
    assert!(removed.new_value.is_none());
    assert!(removed.state_generation > initial.state_generation);
    fixture.grants(true, false);
    let error = tokio::time::timeout(LIMIT, stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code(), tonic::Code::PermissionDenied);
}

#[test]
fn private_generation_state_is_typed_but_never_part_of_public_router_policy() {
    let schema = TchedRouterPlugin::new().schema().unwrap();
    let value =
        simd_json::serde::to_owned_value(json!({"generation_revision":1,"generation_jobs":{}}))
            .unwrap();
    let valid = schema.validate(&value);
    assert!(valid.valid, "{:?}", valid.errors);
    assert!(valid.warnings.is_empty());
    assert!(schema
        .project_public_state(&value)
        .unwrap()
        .as_ref()
        .is_none_or(|state| {
            serde_json::to_value(state)
                .unwrap()
                .get("generation_jobs")
                .is_none()
        }));
}

#[tokio::test]
async fn queued_grant_withdrawal_rejects_before_any_durable_or_native_effect() {
    let mut native = NativeFixture::new().await;
    let fixture = Fixture::new(native.endpoint.clone()).await;
    fixture.activate(&fixture.first).await;
    let publisher = fixture.engine.generation_jobs().unwrap().clone();
    let owner = fixture.owner(&fixture.first);
    let slot = publisher.slot(&owner).unwrap();
    let guard = slot.operation.lock().await;
    let engine = fixture.engine.clone();
    let identity = fixture.first.clone();
    let call = tokio::spawn(async move {
        publisher
            .dispatch(
                &engine,
                &TchedRouterPlugin::new().schema().unwrap(),
                "start_generation",
                &json!({"agent_alias":"fixture","prompt":"queued-private"}),
                &identity.principal_id,
                Some(&identity.session_id),
                Some(&identity.session_genesis),
                None,
            )
            .await
    });
    tokio::time::timeout(LIMIT, async {
        while slot.calls.available_permits() == MAX_OWNER_CALLS {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    fixture.grants(false, true);
    drop(guard);
    assert!(tokio::time::timeout(LIMIT, call)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    assert!(fixture
        .documents
        .read(&owner.document_id())
        .await
        .unwrap()
        .is_none());
    assert!(matches!(
        native.prompt.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
    assert!(fixture
        .private
        .open_child(owner.selector())
        .unwrap()
        .unwrap()
        .pending_document()
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn queued_llm_origin_grant_and_both_original_seals_are_rechecked_before_effects() {
    for withdrawal in ["origin-grant", "origin-seal", "router-seal"] {
        let mut native = NativeFixture::new().await;
        let fixture = Fixture::new(native.endpoint.clone()).await;
        fixture.activate(&fixture.first).await;
        let publisher = fixture.engine.generation_jobs().unwrap().clone();
        let owner = fixture.owner(&fixture.first);
        let slot = publisher.slot(&owner).unwrap();
        let guard = slot.operation.lock().await;
        let engine = fixture.engine.clone();
        let identity = fixture.first.clone();
        let call = tokio::spawn(async move {
            engine
                .dispatch_method_call_with_identity(
                    "large_language_model",
                    "stream_generate",
                    &json!({"agent_alias":"fixture","prompt":"queued-facade-private"}).to_string(),
                    Some("llm.invoke"),
                    &identity.principal_id,
                    Some(&identity.session_id),
                    Some(&identity.session_genesis),
                )
                .await
        });
        tokio::time::timeout(LIMIT, async {
            while slot.calls.available_permits() == MAX_OWNER_CALLS {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        if withdrawal == "origin-grant" {
            // The nested start grant remains present; only the original facade
            // authority is withdrawn after accepted queue admission.
            std::fs::write(&fixture.grants,serde_json::to_vec(&json!({(fixture.first.principal_id.clone()):{"capabilities":[START_CAPABILITY,READ_CAPABILITY,RECOVER_CAPABILITY]}})).unwrap()).unwrap();
        } else {
            let mut schema = if withdrawal == "origin-seal" {
                LargeLanguageModelPlugin::new().schema().unwrap()
            } else {
                TchedRouterPlugin::new().schema().unwrap()
            };
            // Metadata-only reseal leaves the compiled method shape compatible,
            // so this specifically proves the original exact pin is retained.
            schema.description.push_str(" fixture reseal");
            let mut sealer = op_blob::BlobStore::open(&fixture.catalog).unwrap();
            sealer
                .write(&op_blob::blobify_plugin_schema(
                    &schema.name,
                    schema.clone(),
                ))
                .unwrap();
        }
        drop(guard);
        assert!(
            tokio::time::timeout(LIMIT, call)
                .await
                .unwrap()
                .unwrap()
                .is_err(),
            "{withdrawal}"
        );
        assert!(
            fixture
                .documents
                .read(&owner.document_id())
                .await
                .unwrap()
                .is_none(),
            "{withdrawal}"
        );
        assert!(
            matches!(
                native.prompt.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ),
            "{withdrawal}"
        );
        assert!(
            fixture
                .private
                .open_child(owner.selector())
                .unwrap()
                .unwrap()
                .pending_document()
                .unwrap()
                .is_none(),
            "{withdrawal}"
        );
    }
}

fn fragment_state() -> GenerationState {
    GenerationState {
        generation_revision: 2,
        generation_jobs: [(
            "job".into(),
            GenerationJob {
                job_id: "job".into(),
                revision: 2,
                agent_alias: "fixture".into(),
                status: GenerationStatus::Running,
                output: String::new(),
                committed_chunks: 0,
                reported_provider: None,
                reported_model: None,
                prompt_digest: "a".repeat(64),
                source_schema_hash: "b".repeat(64),
                created_at: 1,
                updated_at: 1,
                failure: None,
            },
        )]
        .into_iter()
        .collect(),
    }
}

#[test]
fn ready_batch_never_waits_and_preserves_order_across_byte_and_count_boundaries() {
    use std::collections::VecDeque;
    let state = fragment_state();
    let mut carry = None;
    let mut calls = 0;
    let first = ready_fragment_batch(&state, "job", "first".into(), &mut carry, || {
        calls += 1;
        None
    })
    .unwrap();
    assert_eq!(first, ["first"]);
    assert_eq!(calls, 1);
    assert!(carry.is_none());
    let mut queued = VecDeque::from(["b".into(), "c".into()]);
    let batch =
        ready_fragment_batch(&state, "job", "a".into(), &mut carry, || queued.pop_front()).unwrap();
    assert_eq!(batch, ["a", "b", "c"]);
    assert!(queued.is_empty());
    let mut queued = VecDeque::from(["boundary".into(), "later".into()]);
    let batch = ready_fragment_batch(
        &state,
        "job",
        "x".repeat(MAX_CHUNK_BYTES),
        &mut carry,
        || queued.pop_front(),
    )
    .unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(carry.as_deref(), Some("boundary"));
    let first = carry.take().unwrap();
    let batch =
        ready_fragment_batch(&state, "job", first, &mut carry, || queued.pop_front()).unwrap();
    assert_eq!(batch, ["boundary", "later"]);
    assert!(carry.is_none());
    let mut queued: VecDeque<String> = (1..=MAX_BATCH_FRAGMENTS)
        .map(|index| index.to_string())
        .collect();
    let batch =
        ready_fragment_batch(&state, "job", "0".into(), &mut carry, || queued.pop_front()).unwrap();
    assert_eq!(batch.len(), MAX_BATCH_FRAGMENTS);
    assert_eq!(queued.pop_front().unwrap(), MAX_BATCH_FRAGMENTS.to_string());
    let mut attempts = 0;
    let batch = ready_fragment_batch(&state, "job", "first".into(), &mut carry, || {
        attempts += 1;
        Some(String::new())
    })
    .unwrap();
    assert_eq!(batch, ["first"]);
    assert_eq!(attempts, MAX_BATCH_FRAGMENTS - 1);
}

#[test]
fn ready_batch_commits_valid_prefix_before_an_over_limit_successor() {
    let mut state = fragment_state();
    state
        .generation_jobs
        .get_mut("job")
        .unwrap()
        .committed_chunks = MAX_FRAMES - 1;
    let mut queued =
        std::collections::VecDeque::from(["over-limit".into(), "must-not-overtake".into()]);
    let mut carry = None;
    let batch = ready_fragment_batch(&state, "job", "last-valid".into(), &mut carry, || {
        queued.pop_front()
    })
    .unwrap();
    assert_eq!(batch, ["last-valid"]);
    assert_eq!(carry.as_deref(), Some("over-limit"));
    state
        .generation_jobs
        .get_mut("job")
        .unwrap()
        .committed_chunks = MAX_FRAMES;
    let first = carry.take().unwrap();
    assert!(ready_fragment_batch(&state, "job", first, &mut carry, || queued.pop_front()).is_err());
    assert_eq!(queued.front().unwrap(), "must-not-overtake");
}

#[tokio::test]
async fn native_burst_terminal_follows_all_original_fragments_in_committed_shm() {
    let mut native = NativeFixture::new().await;
    let fixture = Fixture::new(native.endpoint.clone()).await;
    fixture.activate(&fixture.first).await;
    fixture.activate(&fixture.second).await;
    let started = fixture
        .call(
            &fixture.first,
            "start_generation",
            json!({"agent_alias":"fixture","prompt":"bounded-burst"}),
        )
        .await
        .unwrap();
    let id = started["job"]["job_id"].as_str().unwrap();
    tokio::time::timeout(LIMIT, &mut native.prompt)
        .await
        .unwrap()
        .unwrap();
    // A full-size first fragment and more than one batch's fragment count
    // exercise both boundaries. Do not assume any particular scheduling or
    // number of batches: only already-queued fragments may be grouped.
    let fragments: Vec<String> = std::iter::once("x".repeat(MAX_CHUNK_BYTES))
        .chain((0..MAX_BATCH_FRAGMENTS + 2).map(|index| format!("\n{index}:λ")))
        .collect();
    let output = fragments.concat();
    for fragment in &fragments {
        native.chunk(fragment).await;
    }
    native.done(&output).await;
    let complete = fixture
        .wait_status_with_limit(id, GenerationStatus::Completed, Duration::from_secs(90))
        .await;
    assert_eq!(complete.output, output);
    assert_eq!(complete.committed_chunks as usize, fragments.len());
    assert_eq!(
        string_value(
            fixture
                .get(&fixture.first, &format!("generation_jobs/{id}/output"))
                .await
                .unwrap()
        ),
        output
    );
    assert_eq!(
        fixture
            .get(&fixture.second, &format!("generation_jobs/{id}"))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::NotFound
    );
}
