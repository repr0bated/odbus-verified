//! Private chatbot episode views on the existing StateSync transport.
//!
//! EpisodePublisher owns activation and committed private SHM reads. Wake
//! payloads contain only an opaque owner key; values are always rehydrated from
//! the committed projection.
use std::{collections::BTreeMap, pin::Pin, sync::Arc, time::Duration};

use futures::Stream;
use op_cozo_store::EpisodeScope;
use serde_json::Value;
use tonic::{Request, Status};

use crate::{
    chatbot_episode_state::{EpisodePublisher, PLUGIN, READ_CAPABILITY},
    event_chain_access,
    grpc_server::{
        simd_to_prost_struct, simd_to_prost_value, state_heartbeat_frame,
        state_snapshot_frames_with_views, DECLARED_CAPABILITY_HEADER,
    },
    interceptor::load_capability_grants,
    method_contract_admission,
    mutation_engine::MutationEngine,
    oracle_assertion::HumanPrincipalIdentity,
    proto::{GetStateRequest, GetStateResponse, StateChange, StateFrameKind, SubscribeRequest},
    state_stream::StateSnapshot,
};

const HEARTBEAT: Duration = Duration::from_secs(30);
const MAX_PATHS: usize = 32;
const MAX_PATH_BYTES: usize = 2048;
pub(crate) type EpisodeStream = Pin<Box<dyn Stream<Item = Result<StateChange, Status>> + Send>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EpisodeSelection {
    paths: Vec<String>,
}

fn private_path(path: &str) -> bool {
    let root = format!("/org/opdbus/v1/plugins/{PLUGIN}");
    [
        "episodes",
        "episode_count",
        "episode_revision",
        "episodes_truncated",
    ]
    .into_iter()
    .any(|child| {
        let child_root = format!("{root}/{child}");
        path == child_root || (child == "episodes" && path.starts_with(&(child_root + "/")))
    })
}

impl EpisodeSelection {
    pub(crate) fn from_subscription(
        plugin_ids: &[String],
        paths: &[String],
    ) -> anyhow::Result<Option<Self>> {
        if !paths.iter().any(|path| private_path(path)) {
            return Ok(None);
        }
        anyhow::ensure!(
            plugin_ids.is_empty() || plugin_ids == [PLUGIN.to_owned()],
            "private episode subscriptions require one canonical plugin"
        );
        anyhow::ensure!(
            !paths.is_empty() && paths.len() <= MAX_PATHS,
            "private episode subscriptions require 1..=32 explicit paths"
        );
        let mut selected = Vec::with_capacity(paths.len());
        for path in paths {
            anyhow::ensure!(
                private_path(path),
                "private and public state selectors cannot be mixed"
            );
            anyhow::ensure!(
                path.len() <= MAX_PATH_BYTES,
                "private episode path exceeds its bound"
            );
            anyhow::ensure!(
                !path
                    .chars()
                    .any(|c| c.is_control() || matches!(c, '*' | '?' | '\\'))
                    && path
                        .split('/')
                        .skip(1)
                        .all(|part| !part.is_empty() && part != "." && part != ".."),
                "private episode paths must name exact descendants"
            );
            if !selected.contains(path) {
                selected.push(path.clone());
            }
        }
        Ok(Some(Self { paths: selected }))
    }
    pub(crate) fn from_get_state(plugin_id: &str, path: &str) -> anyhow::Result<Option<Self>> {
        Self::from_subscription(&[plugin_id.to_owned()], &[path.to_owned()])
    }
    pub(crate) fn paths(&self) -> &[String] {
        &self.paths
    }
}

struct EpisodeAccess {
    identity: HumanPrincipalIdentity,
    schema: op_state_store::PluginSchema,
}

impl EpisodeAccess {
    #[allow(clippy::result_large_err)]
    fn from_request<T>(
        request: &Request<T>,
        identity: HumanPrincipalIdentity,
    ) -> Result<Self, Status> {
        let declared: Vec<_> = request
            .metadata()
            .get_all(DECLARED_CAPABILITY_HEADER)
            .into_iter()
            .collect();
        if declared.len() != 1 || declared[0].to_str().ok() != Some(READ_CAPABILITY) {
            return Err(Status::permission_denied(
                "one exact chatbot.read capability declaration is required",
            ));
        }
        let (_, schema) = method_contract_admission::admit_compiled_invocation(
            PLUGIN,
            "list_episodes",
            Some(READ_CAPABILITY),
        )
        .map_err(|_| {
            Status::failed_precondition("compiled chatbot list_episodes contract is unavailable")
        })?;
        let access = Self { identity, schema };
        access.check_grant()?;
        Ok(access)
    }
    #[allow(clippy::result_large_err)]
    fn check_grant(&self) -> Result<(), Status> {
        if !load_capability_grants(&self.identity.principal_id)
            .iter()
            .any(|grant| grant == READ_CAPABILITY)
        {
            return Err(Status::permission_denied(
                "current exact-principal chatbot.read grant is required",
            ));
        }
        Ok(())
    }
    #[allow(clippy::result_large_err)]
    async fn revalidate(&self, engine: &MutationEngine) -> Result<(), Status> {
        event_chain_access::validate_identity(engine, &self.identity).await?;
        self.check_grant()?;
        let (_, schema) = method_contract_admission::admit_compiled_invocation(
            PLUGIN,
            "list_episodes",
            Some(READ_CAPABILITY),
        )
        .map_err(|_| {
            Status::failed_precondition("compiled chatbot list_episodes contract changed")
        })?;
        let current = serde_json::to_value(&schema)
            .map_err(|_| Status::internal("chatbot schema cannot be encoded"))?;
        let admitted = serde_json::to_value(&self.schema)
            .map_err(|_| Status::internal("admitted chatbot schema cannot be encoded"))?;
        if current != admitted {
            return Err(Status::failed_precondition(
                "chatbot episode schema changed during subscription",
            ));
        }
        Ok(())
    }
    fn scope(&self) -> EpisodeScope {
        EpisodeScope {
            principal_id: self.identity.principal_id.clone(),
            session_id: self.identity.session_id.clone(),
            session_genesis: self.identity.session_genesis.clone(),
        }
    }
}

async fn open<T>(
    engine: &MutationEngine,
    request: &Request<T>,
    identity: HumanPrincipalIdentity,
) -> Result<
    (
        Arc<EpisodePublisher>,
        EpisodeAccess,
        EpisodeScope,
        tokio::sync::broadcast::Receiver<String>,
        StateSnapshot,
    ),
    Status,
> {
    let access = EpisodeAccess::from_request(request, identity)?;
    access.revalidate(engine).await?;
    let publisher = engine
        .chatbot_episode_publisher()
        .map_err(|_| {
            Status::failed_precondition("private chatbot episode owner is not configured")
        })?
        .clone();
    let scope = access.scope();
    let receiver = publisher.subscribe();
    let initial = publisher.read(scope.clone()).await.map_err(|_| {
        Status::failed_precondition(
            "private chatbot episodes require explicit list_episodes activation",
        )
    })?;
    Ok((publisher, access, scope, receiver, initial))
}

pub(crate) async fn subscribe(
    engine: Arc<MutationEngine>,
    request: Request<SubscribeRequest>,
    identity: HumanPrincipalIdentity,
    selection: EpisodeSelection,
) -> Result<EpisodeStream, Status> {
    if !request.get_ref().tags.is_empty() {
        return Err(Status::unimplemented(
            "chatbot episode tag selection has no committed contract",
        ));
    }
    let (publisher, access, scope, mut receiver, initial) =
        open(&engine, &request, identity).await?;
    let req = request.into_inner();
    Ok(Box::pin(async_stream::stream! {
        let mut previous = initial;
        match state_snapshot_frames_with_views(&previous, None, req.include_schema, req.include_initial_state, StateFrameKind::InitialState, selection.paths(), false, exact_views) {
            Ok(frames) => for frame in frames {
                if let Err(status) = access.revalidate(&engine).await { yield Err(status); return; }
                yield Ok(frame);
            },
            Err(status) => { yield Err(status); return; }
        }
        let owner = EpisodePublisher::key(&scope);
        let mut heartbeat = tokio::time::interval(HEARTBEAT);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        heartbeat.tick().await;
        loop {
            let force = tokio::select! {
                wake = receiver.recv() => match wake { Ok(key) if key == owner => Some(false), Ok(_) => continue, Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => Some(true), Err(tokio::sync::broadcast::error::RecvError::Closed) => return },
                _ = heartbeat.tick() => None,
            };
            if let Err(status) = access.revalidate(&engine).await {
                yield Err(status);
                return;
            }
            if let Some(force_rehydrate) = force {
                let current = match publisher.read(scope.clone()).await { Ok(snapshot) => snapshot, Err(_) => { yield Err(Status::failed_precondition("committed private chatbot episodes are unavailable")); return; } };
                if force_rehydrate || current.generation != previous.generation || current.catalog_hash != previous.catalog_hash {
                    match state_snapshot_frames_with_views(&current, Some(&previous), req.include_schema, true, if force_rehydrate { StateFrameKind::InitialState } else { StateFrameKind::Update }, selection.paths(), force_rehydrate, exact_views) {
                        Ok(frames) => for frame in frames {
                            if let Err(status) = access.revalidate(&engine).await { yield Err(status); return; }
                            yield Ok(frame);
                        },
                        Err(status) => { yield Err(status); return; }
                    }
                    previous = current;
                }
            }
            if let Err(status) = access.revalidate(&engine).await { yield Err(status); return; }
            yield Ok(state_heartbeat_frame(&previous));
        }
    }))
}

pub(crate) async fn get_state(
    engine: Arc<MutationEngine>,
    request: Request<GetStateRequest>,
    identity: HumanPrincipalIdentity,
    selection: EpisodeSelection,
) -> Result<GetStateResponse, Status> {
    let (_publisher, access, _scope, _receiver, snapshot) =
        open(&engine, &request, identity).await?;
    let state = snapshot
        .plugins
        .get(PLUGIN)
        .and_then(|plugin| plugin.state.as_ref())
        .ok_or_else(|| Status::not_found("chatbot episodes have no committed document"))?;
    let selected = value_at_path(state, &selection.paths()[0])
        .map_err(|_| Status::invalid_argument("invalid chatbot episode descendant"))?
        .ok_or_else(|| Status::not_found("chatbot episode descendant is absent"))?;
    let owned = simd_json::serde::to_owned_value(selected)
        .map_err(|_| Status::data_loss("committed chatbot episode cannot be encoded"))?;
    access.revalidate(&engine).await?;
    Ok(GetStateResponse {
        state: selected.is_object().then(|| simd_to_prost_struct(&owned)),
        value: Some(simd_to_prost_value(&owned)),
        effective_hash: String::new(),
        at_event_id: 0,
    })
}

fn value_at_path<'a>(state: &'a Value, path: &str) -> anyhow::Result<Option<&'a Value>> {
    let root = crate::state_stream::plugin_object_path(PLUGIN);
    let suffix = path.strip_prefix(&(root + "/")).unwrap_or("");
    let mut current = state;
    if suffix.is_empty() {
        return Ok(Some(current));
    }
    for segment in suffix.split('/') {
        anyhow::ensure!(
            !segment.is_empty() && segment != "." && segment != "..",
            "invalid chatbot episode path segment"
        );
        current = match current {
            Value::Object(object) => match object.get(segment) {
                Some(value) => value,
                None => return Ok(None),
            },
            Value::Array(items) => {
                let index: usize = segment
                    .parse()
                    .map_err(|_| anyhow::anyhow!("invalid chatbot episode array index"))?;
                match items.get(index) {
                    Some(value) => value,
                    None => return Ok(None),
                }
            }
            _ => return Ok(None),
        };
    }
    Ok(Some(current))
}

fn exact_views(
    plugin: &str,
    state: &Value,
    paths: &[String],
) -> anyhow::Result<BTreeMap<String, Value>> {
    anyhow::ensure!(plugin == PLUGIN, "invalid private chatbot episode plugin");
    let selection = EpisodeSelection::from_subscription(&[PLUGIN.to_owned()], paths)?
        .ok_or_else(|| anyhow::anyhow!("explicit chatbot episode selection is required"))?;
    let mut views = BTreeMap::new();
    let mut bytes = 0usize;
    for path in selection.paths() {
        if let Some(value) = value_at_path(state, path)? {
            bytes = bytes
                .checked_add(serde_json::to_vec(value)?.len())
                .ok_or_else(|| anyhow::anyhow!("selected-byte overflow"))?;
            anyhow::ensure!(
                bytes <= 16 * 1024 * 1024,
                "chatbot episode selection exceeds 16 MiB"
            );
            views.insert(path.clone(), value.clone());
        }
    }
    Ok(views)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        grpc_server::OperationGrpcServer,
        human_principal_dispatch::tests::{
            pk, register, temp_cozo, temp_shm_with_schemas, test_engine,
        },
        identity_sled_dispatch::{tests::write_identity, IdentitySledStore},
        proto::{state_sync_server::StateSync, GetStateRequest, SubscribeRequest},
    };
    use futures::StreamExt;
    use op_plugins::state_plugins::{
        ctl_plane_chatbot::CtlPlaneChatbotPlugin,
        ctl_plane_chatbot_methods::{build_completed_chat_episode, CompletedChatEpisode},
        human_principal::HumanPrincipalPlugin,
        identity_sled::IdentitySledPlugin,
    };
    use op_state::StatePlugin;
    use serde_json::json;
    use std::sync::Arc;

    struct Fixture {
        engine: Arc<MutationEngine>,
        server: OperationGrpcServer,
        scope: EpisodeScope,
        identity: HumanPrincipalIdentity,
        catalog: std::path::PathBuf,
        grants: std::path::PathBuf,
        schema: op_state_store::PluginSchema,
        _root: tempfile::TempDir,
        _cozo: crate::human_principal_dispatch::tests::TempCozo,
        _shm: crate::human_principal_dispatch::tests::TempShm,
        _grants: GrantsRestore,
    }

    struct GrantsRestore(Option<std::ffi::OsString>);
    impl Drop for GrantsRestore {
        fn drop(&mut self) {
            if let Some(path) = self.0.take() {
                std::env::set_var("OP_GRANTS_PATH", path);
            } else {
                std::env::remove_var("OP_GRANTS_PATH");
            }
        }
    }

    impl Fixture {
        async fn new() -> Self {
            let cozo = temp_cozo();
            let schema = CtlPlaneChatbotPlugin::new().schema().unwrap();
            let shm = temp_shm_with_schemas(&[
                schema.clone(),
                HumanPrincipalPlugin::new().schema().unwrap(),
                IdentitySledPlugin::new().schema().unwrap(),
            ]);
            let engine = test_engine();
            engine
                .install_identity_sled_store_for_test(IdentitySledStore::new(
                    op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
                ))
                .unwrap();
            let store = Arc::new(op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap());
            engine
                .configure_chatbot_episode_store(store.clone())
                .unwrap();
            let root = tempfile::tempdir().unwrap();
            let private = op_core::projection_shm::PrivateProjectionDirectory::prepare(
                &root.path().join("private"),
            )
            .unwrap();
            let catalog = crate::sealed_schema_reader::blob_catalog_dir_from_env();
            engine
                .configure_chatbot_episode_projection(private, catalog.clone())
                .unwrap();
            let key = pk(0xF1);
            register(&key, "episode-stream-fixture").await.unwrap();
            let sled = write_identity(&engine, &key).await.unwrap();
            let identity = HumanPrincipalIdentity {
                principal_id: op_identity::session::derive_principal_id(&key),
                human_pubkey: key,
                session_id: sled.session_id.clone(),
                session_genesis: sled.genesis.clone().unwrap(),
                expires_at: 0,
            };
            let scope = EpisodeScope {
                principal_id: identity.principal_id.clone(),
                session_id: identity.session_id.clone(),
                session_genesis: identity.session_genesis.clone(),
            };
            let grants = root.path().join("grants.json");
            let old = GrantsRestore(std::env::var_os("OP_GRANTS_PATH"));
            std::env::set_var("OP_GRANTS_PATH", &grants);
            let fixture = Self {
                engine: engine.clone(),
                server: OperationGrpcServer::new(engine),
                scope,
                identity,
                catalog,
                grants,
                schema,
                _root: root,
                _cozo: cozo,
                _shm: shm,
                _grants: old,
            };
            fixture.grant(true);
            fixture
        }

        fn grant(&self, enabled: bool) {
            let caps = enabled.then_some(vec![READ_CAPABILITY]).unwrap_or_default();
            std::fs::write(
                &self.grants,
                serde_json::to_vec(
                    &json!({ self.identity.principal_id.clone(): { "capabilities": caps } }),
                )
                .unwrap(),
            )
            .unwrap();
        }

        fn request<T>(&self, body: T, identity: &HumanPrincipalIdentity) -> Request<T> {
            let mut request = Request::new(body);
            request.extensions_mut().insert(identity.clone());
            request
                .metadata_mut()
                .insert(DECLARED_CAPABILITY_HEADER, READ_CAPABILITY.parse().unwrap());
            request
        }

        async fn publish(&self, answer: &str) {
            let episode = build_completed_chat_episode(CompletedChatEpisode {
                scope: self.scope.clone(),
                conversation_id: Some("stream-test".into()),
                goal_text: Some("stream goal".into()),
                decision_output: answer.into(),
                tools: vec![],
                started_at: chrono::Utc::now(),
                ended_at: chrono::Utc::now(),
            })
            .unwrap();
            self.engine.commit_chatbot_episode(episode).await.unwrap();
        }
    }
    #[test]
    fn selectors_are_exact_and_private() {
        let root = format!("/org/opdbus/v1/plugins/{PLUGIN}");
        assert!(
            EpisodeSelection::from_subscription(&[], std::slice::from_ref(&root))
                .unwrap()
                .is_none()
        );
        assert!(
            EpisodeSelection::from_subscription(&[], &[format!("{root}/config")])
                .unwrap()
                .is_none()
        );
        assert!(EpisodeSelection::from_subscription(
            &[],
            &[
                format!("{root}/episodes"),
                format!("{root}/episode_revision")
            ]
        )
        .is_ok());
        assert!(EpisodeSelection::from_subscription(
            &[],
            &[format!("{root}/episodes"), format!("{root}/config")]
        )
        .is_err());
        for path in [
            format!("{root}/episodes/../x"),
            format!("{root}/episodes/*"),
            format!("{root}/episodes//x"),
        ] {
            assert!(EpisodeSelection::from_subscription(&[], &[path]).is_err());
        }
        assert!(EpisodeSelection::from_subscription(&[], &["x".into()])
            .unwrap()
            .is_none());
        assert!(EpisodeSelection::from_subscription(
            &["other".into()],
            &[format!("{root}/episodes")]
        )
        .is_err());
    }

    #[tokio::test]
    async fn authenticated_owner_receives_initial_and_update_then_revocation_stops() {
        let fixture = Fixture::new().await;
        fixture.publish("first answer").await;
        let path = format!("/org/opdbus/v1/plugins/{PLUGIN}/episodes");
        let request = fixture.request(
            SubscribeRequest {
                plugin_ids: vec![PLUGIN.into()],
                path_patterns: vec![path],
                include_initial_state: true,
                ..Default::default()
            },
            &fixture.identity,
        );
        let mut stream = fixture
            .server
            .subscribe(request)
            .await
            .unwrap()
            .into_inner();
        let initial = stream.next().await.unwrap().unwrap();
        assert!(format!("{initial:?}").contains("first answer"));

        fixture.publish("second answer").await;
        let update = stream.next().await.unwrap().unwrap();
        assert!(format!("{update:?}").contains("second answer"));

        fixture.grant(false);
        fixture.publish("must not leak").await;
        let revoked = stream.next().await.unwrap().unwrap_err();
        assert_eq!(revoked.code(), tonic::Code::PermissionDenied);
        drop(stream);
        let stored = fixture
            .engine
            .chatbot_episode_publisher()
            .unwrap()
            .read(fixture.scope.clone())
            .await
            .unwrap();
        assert_eq!(
            stored.plugins[PLUGIN].state.as_ref().unwrap()["episode_count"],
            3
        );
    }

    #[tokio::test]
    async fn other_session_cannot_read_owner_episode_projection() {
        let fixture = Fixture::new().await;
        fixture.publish("private answer").await;
        let mut other = fixture.identity.clone();
        other.session_id.push_str("-other");
        other.session_genesis.push_str("-other");
        let path = format!("/org/opdbus/v1/plugins/{PLUGIN}/episodes");
        let request = fixture.request(
            GetStateRequest {
                plugin_id: PLUGIN.into(),
                object_path: path,
            },
            &other,
        );
        let error = fixture.server.get_state(request).await.unwrap_err();
        assert_eq!(error.code(), tonic::Code::Unauthenticated);

        // A second genuinely provisioned principal with the same read grant
        // still cannot select the first principal's private namespace.
        let key = pk(0xF2);
        register(&key, "other-episode-reader").await.unwrap();
        let sled = write_identity(&fixture.engine, &key).await.unwrap();
        let other = HumanPrincipalIdentity {
            principal_id: op_identity::session::derive_principal_id(&key),
            human_pubkey: key,
            session_id: sled.session_id,
            session_genesis: sled.genesis.unwrap(),
            expires_at: 0,
        };
        std::fs::write(
            &fixture.grants,
            serde_json::to_vec(&json!({
                other.principal_id.clone(): {"capabilities": [READ_CAPABILITY]}
            }))
            .unwrap(),
        )
        .unwrap();
        let other_scope = EpisodeScope {
            principal_id: other.principal_id.clone(),
            session_id: other.session_id.clone(),
            session_genesis: other.session_genesis.clone(),
        };
        fixture
            .engine
            .chatbot_episode_publisher()
            .unwrap()
            .recover(&fixture.engine, other_scope.clone())
            .await
            .unwrap();
        let request = fixture.request(
            GetStateRequest {
                plugin_id: PLUGIN.into(),
                object_path: format!("/org/opdbus/v1/plugins/{PLUGIN}/episodes"),
            },
            &other,
        );
        let response = fixture
            .server
            .get_state(request)
            .await
            .unwrap()
            .into_inner();
        assert!(response.state.unwrap().fields.is_empty());
        let snapshot = fixture
            .engine
            .chatbot_episode_publisher()
            .unwrap()
            .read(other_scope)
            .await
            .unwrap();
        assert_eq!(
            snapshot.plugins[PLUGIN].state.as_ref().unwrap()["episode_count"],
            0
        );
    }

    #[tokio::test]
    async fn sealed_schema_drift_terminates_episode_stream() {
        let fixture = Fixture::new().await;
        fixture.publish("before drift").await;
        let path = format!("/org/opdbus/v1/plugins/{PLUGIN}/episodes");
        let request = fixture.request(
            SubscribeRequest {
                plugin_ids: vec![PLUGIN.into()],
                path_patterns: vec![path],
                include_initial_state: true,
                ..Default::default()
            },
            &fixture.identity,
        );
        let mut stream = fixture
            .server
            .subscribe(request)
            .await
            .unwrap()
            .into_inner();
        let _ = stream.next().await.unwrap().unwrap();

        let mut changed = fixture.schema.clone();
        fixture.publish("wake before drift").await;
        changed.description.push_str(" drift");
        op_blob::BlobStore::open(&fixture.catalog)
            .unwrap()
            .write(&op_blob::blobify_plugin_schema(PLUGIN, changed))
            .unwrap();
        let error = stream.next().await.unwrap().unwrap_err();
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        op_blob::BlobStore::open(&fixture.catalog)
            .unwrap()
            .write(&op_blob::blobify_plugin_schema(
                PLUGIN,
                fixture.schema.clone(),
            ))
            .unwrap();
    }
}
