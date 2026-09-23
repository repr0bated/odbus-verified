//! gRPC server implementation

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use super::auth::{self, CredentialVerifier, FailClosedVerifier};
use super::proto::service_manager_server::ServiceManager as ServiceManagerTrait;
use super::proto::*;
use crate::manager::ServiceManager;
use crate::schema::{self, ServiceName};

/// An empty selection watches all services; otherwise names are exact.
fn matches_watch_filter(filter: &HashSet<ServiceName>, name: &ServiceName) -> bool {
    filter.is_empty() || filter.contains(name)
}

pub struct GrpcServer {
    manager: Arc<ServiceManager>,
    verifier: Arc<dyn CredentialVerifier>,
}

impl GrpcServer {
    pub fn new(manager: Arc<ServiceManager>) -> Self {
        Self {
            manager,
            verifier: Arc::new(FailClosedVerifier),
        }
    }

    async fn authorize<T>(&self, request: &Request<T>, capability: &str) -> Result<(), Status> {
        auth::authorize(request, capability, &self.verifier).await
    }
}

#[tonic::async_trait]
impl ServiceManagerTrait for GrpcServer {
    async fn start(&self, req: Request<StartRequest>) -> Result<Response<StartResponse>, Status> {
        self.authorize(&req, "service_manager.start").await?;
        let name = ServiceName::new(&req.get_ref().name)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        let status = self
            .manager
            .start(&name)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(StartResponse {
            status: Some(status.into()),
        }))
    }

    async fn stop(&self, req: Request<StopRequest>) -> Result<Response<StopResponse>, Status> {
        self.authorize(&req, "service_manager.stop").await?;
        let name = ServiceName::new(&req.get_ref().name)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        let status = self
            .manager
            .stop(&name)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(StopResponse {
            status: Some(status.into()),
        }))
    }

    async fn restart(
        &self,
        req: Request<RestartRequest>,
    ) -> Result<Response<RestartResponse>, Status> {
        self.authorize(&req, "service_manager.restart").await?;
        let name = ServiceName::new(&req.get_ref().name)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        let status = self
            .manager
            .restart(&name)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(RestartResponse {
            status: Some(status.into()),
        }))
    }

    async fn reload(
        &self,
        req: Request<ReloadRequest>,
    ) -> Result<Response<ReloadResponse>, Status> {
        self.authorize(&req, "service_manager.reload").await?;
        let name = ServiceName::new(&req.get_ref().name)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        // Runit has no generic application reload verb, so use restart.
        let status = self
            .manager
            .restart(&name)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(ReloadResponse {
            status: Some(status.into()),
        }))
    }

    async fn create(
        &self,
        req: Request<CreateRequest>,
    ) -> Result<Response<CreateResponse>, Status> {
        self.authorize(&req, "service_manager.create").await?;
        let proto_def = req
            .get_ref()
            .service
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("missing service definition"))?;

        let service_def =
            proto_to_schema_def(proto_def).map_err(|e| Status::invalid_argument(e.to_string()))?;

        // Persist the service definition in the store
        self.manager
            .create(&service_def)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(CreateResponse {
            service: Some(service_def.into()),
        }))
    }

    async fn delete(
        &self,
        req: Request<DeleteRequest>,
    ) -> Result<Response<DeleteResponse>, Status> {
        self.authorize(&req, "service_manager.delete").await?;
        let name = ServiceName::new(&req.get_ref().name)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        self.manager
            .delete(&name)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(DeleteResponse {}))
    }

    async fn get(&self, req: Request<GetRequest>) -> Result<Response<GetResponse>, Status> {
        self.authorize(&req, "service_manager.get").await?;
        let name = ServiceName::new(&req.get_ref().name)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        let service_def = self
            .manager
            .get(&name)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found(format!("service not found: {}", name)))?;

        let status = self
            .manager
            .get_status(&name)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(GetResponse {
            service: Some(service_def.into()),
            status: Some(status.into()),
        }))
    }

    async fn list(&self, req: Request<ListRequest>) -> Result<Response<ListResponse>, Status> {
        self.authorize(&req, "service_manager.list").await?;
        let services = self
            .manager
            .list()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(ListResponse {
            services: services.into_iter().map(Into::into).collect(),
        }))
    }

    async fn enable(
        &self,
        req: Request<EnableRequest>,
    ) -> Result<Response<EnableResponse>, Status> {
        self.authorize(&req, "service_manager.enable").await?;
        let name = ServiceName::new(&req.get_ref().name)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        self.manager
            .set_enabled(&name, true)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(EnableResponse {}))
    }

    async fn disable(
        &self,
        req: Request<DisableRequest>,
    ) -> Result<Response<DisableResponse>, Status> {
        self.authorize(&req, "service_manager.disable").await?;
        let name = ServiceName::new(&req.get_ref().name)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        self.manager
            .set_enabled(&name, false)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(DisableResponse {}))
    }

    type WatchStatusStream = ReceiverStream<Result<ServiceEvent, Status>>;

    async fn watch_status(
        &self,
        req: Request<WatchRequest>,
    ) -> Result<Response<Self::WatchStatusStream>, Status> {
        self.authorize(&req, "service_manager.watch_status").await?;
        let filter: HashSet<ServiceName> = req
            .get_ref()
            .names
            .iter()
            .map(ServiceName::new)
            .collect::<Result<_, _>>()
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let (tx, rx) = mpsc::channel(128);
        let mut sub = self.manager.subscribe();

        tokio::spawn(async move {
            loop {
                match sub.recv().await {
                    Ok(event) => {
                        if matches_watch_filter(&filter, &event.name)
                            && tx.send(Ok(event.into())).await.is_err()
                        {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

/// Convert a proto ServiceDef into the internal schema ServiceDef.
fn proto_to_schema_def(proto: &ServiceDef) -> anyhow::Result<schema::ServiceDef> {
    let name = ServiceName::new(&proto.name)?;

    let exec = proto
        .exec
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing exec config"))?;

    let exec_start =
        schema::ExecCommand::new(PathBuf::from(&exec.start_program), exec.start_args.clone())?;

    let exec_stop = match &exec.stop_program {
        Some(prog) if !prog.is_empty() => Some(schema::ExecCommand::new(
            PathBuf::from(prog),
            exec.stop_args.clone(),
        )?),
        _ => None,
    };

    let working_dir = exec.working_dir.as_deref().map(PathBuf::from);
    let user = exec.user.clone();
    let group = exec.group.clone();

    let service_type = match proto.r#type {
        t if t == ServiceType::Simple as i32 => schema::ServiceType::Simple,
        t if t == ServiceType::Forking as i32 => schema::ServiceType::Forking { pid_file: None },
        t if t == ServiceType::Oneshot as i32 => schema::ServiceType::Oneshot,
        t if t == ServiceType::Notify as i32 => schema::ServiceType::Notify,
        _ => schema::ServiceType::Simple,
    };

    let restart = match &proto.restart {
        Some(rp) => {
            let condition = match rp.condition {
                c if c == RestartCondition::RestartAlways as i32 => {
                    schema::RestartCondition::Always
                }
                c if c == RestartCondition::RestartOnFailure as i32 => {
                    schema::RestartCondition::OnFailure
                }
                _ => schema::RestartCondition::Never,
            };
            let delay_secs = rp.delay.as_ref().map(|d| d.seconds as u64).unwrap_or(1);
            schema::RestartPolicy {
                condition,
                delay_secs,
                max_retries: rp.max_retries,
            }
        }
        None => schema::RestartPolicy::default(),
    };

    let depends_on: Vec<ServiceName> = proto
        .depends_on
        .iter()
        .map(ServiceName::new)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(schema::ServiceDef {
        name,
        service_type,
        exec_start,
        exec_stop,
        working_dir,
        user,
        group,
        depends_on,
        waits_for: Vec::new(),
        restart,
        environment: proto.environment.clone(),
        env_file: None,
        resources: None,
        log_type: schema::LogType::default(),
        ready_notification: schema::ReadyNotification::default(),
        chain_to: None,
        smooth_recovery: false,
        enabled: proto.enabled,
    })
}

// Conversions
impl From<crate::schema::ServiceStatus> for ServiceStatus {
    fn from(s: crate::schema::ServiceStatus) -> Self {
        Self {
            name: s.name.to_string(),
            state: match s.state {
                crate::schema::ManagerState::Stopped => ServiceState::StateStopped as i32,
                crate::schema::ManagerState::Starting => ServiceState::StateStarting as i32,
                crate::schema::ManagerState::Running => ServiceState::StateRunning as i32,
                crate::schema::ManagerState::Stopping => ServiceState::StateStopping as i32,
                crate::schema::ManagerState::Failed => ServiceState::StateFailed as i32,
            },
            pid: s.pid,
            error: s.error,
            started_at: s.started_at.map(|t| prost_types::Timestamp {
                seconds: t.timestamp(),
                nanos: t.timestamp_subsec_nanos() as i32,
            }),
        }
    }
}

impl From<crate::schema::ServiceDef> for ServiceDef {
    fn from(s: crate::schema::ServiceDef) -> Self {
        Self {
            name: s.name.to_string(),
            r#type: match s.service_type {
                crate::schema::ServiceType::Simple => ServiceType::Simple as i32,
                crate::schema::ServiceType::Forking { .. } => ServiceType::Forking as i32,
                crate::schema::ServiceType::Oneshot => ServiceType::Oneshot as i32,
                crate::schema::ServiceType::Notify => ServiceType::Notify as i32,
            },
            exec: Some(ExecConfig {
                start_program: s.exec_start.program.to_string_lossy().to_string(),
                start_args: s.exec_start.args,
                stop_program: s
                    .exec_stop
                    .as_ref()
                    .map(|c| c.program.to_string_lossy().to_string()),
                stop_args: s.exec_stop.map(|c| c.args).unwrap_or_default(),
                working_dir: s.working_dir.map(|p| p.to_string_lossy().to_string()),
                user: s.user,
                group: s.group,
            }),
            depends_on: s.depends_on.into_iter().map(|n| n.to_string()).collect(),
            restart: Some(RestartPolicy {
                condition: match s.restart.condition {
                    crate::schema::RestartCondition::Never => RestartCondition::RestartNever as i32,
                    crate::schema::RestartCondition::Always => {
                        RestartCondition::RestartAlways as i32
                    }
                    crate::schema::RestartCondition::OnFailure => {
                        RestartCondition::RestartOnFailure as i32
                    }
                },
                delay: Some(prost_types::Duration {
                    seconds: s.restart.delay_secs as i64,
                    nanos: 0,
                }),
                max_retries: s.restart.max_retries,
            }),
            environment: s.environment,
            resources: None,
            health_check: None,
            enabled: s.enabled,
        }
    }
}

impl From<crate::manager::ServiceEvent> for ServiceEvent {
    fn from(e: crate::manager::ServiceEvent) -> Self {
        Self {
            name: e.name.to_string(),
            old_state: match e.old_state {
                crate::schema::ManagerState::Stopped => ServiceState::StateStopped as i32,
                crate::schema::ManagerState::Starting => ServiceState::StateStarting as i32,
                crate::schema::ManagerState::Running => ServiceState::StateRunning as i32,
                crate::schema::ManagerState::Stopping => ServiceState::StateStopping as i32,
                crate::schema::ManagerState::Failed => ServiceState::StateFailed as i32,
            },
            new_state: match e.new_state {
                crate::schema::ManagerState::Stopped => ServiceState::StateStopped as i32,
                crate::schema::ManagerState::Starting => ServiceState::StateStarting as i32,
                crate::schema::ManagerState::Running => ServiceState::StateRunning as i32,
                crate::schema::ManagerState::Stopping => ServiceState::StateStopping as i32,
                crate::schema::ManagerState::Failed => ServiceState::StateFailed as i32,
            },
            timestamp: Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::Code;

    struct FixtureVerifier;

    #[tonic::async_trait]
    impl CredentialVerifier for FixtureVerifier {
        async fn verify(&self, wire: &[u8], capability: &str) -> Result<(), Status> {
            if wire != b"fixture-credential" {
                return Err(Status::unauthenticated("invalid fixture credential"));
            }
            if capability != "service_manager.list" {
                return Err(Status::permission_denied("capability not granted"));
            }
            Ok(())
        }
    }

    async fn server() -> GrpcServer {
        let path = std::env::temp_dir().join(format!(
            "op-services-auth-test-{}-{}.json",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let store = Arc::new(crate::store::Store::new(path).await.unwrap());
        GrpcServer {
            manager: Arc::new(ServiceManager::for_test(store)),
            verifier: Arc::new(FixtureVerifier),
        }
    }

    fn credential_request<T>(value: T) -> Request<T> {
        let mut request = Request::new(value);
        request.metadata_mut().insert_bin(
            auth::SEALED_ID_METADATA_KEY,
            tonic::metadata::MetadataValue::from_bytes(b"fixture-credential"),
        );
        request
    }

    #[tokio::test]
    async fn credential_free_mutations_are_rejected_before_manager_calls() {
        let server = server().await;
        let create = server
            .create(Request::new(CreateRequest { service: None }))
            .await;
        assert_eq!(create.unwrap_err().code(), Code::Unauthenticated);
        let delete = server
            .delete(Request::new(DeleteRequest {
                name: "demo".into(),
            }))
            .await;
        assert_eq!(delete.unwrap_err().code(), Code::Unauthenticated);
        let stop = server
            .stop(Request::new(StopRequest {
                name: "demo".into(),
            }))
            .await;
        assert_eq!(stop.unwrap_err().code(), Code::Unauthenticated);
    }

    #[tokio::test]
    async fn verified_exact_grant_reaches_manager_and_wrong_action_is_denied() {
        let server = server().await;
        let listed = server
            .list(credential_request(ListRequest { filter: None }))
            .await;
        assert!(listed.unwrap().into_inner().services.is_empty());
        let stop = server
            .stop(credential_request(StopRequest {
                name: "demo".into(),
            }))
            .await;
        assert_eq!(stop.unwrap_err().code(), Code::PermissionDenied);
    }

    #[test]
    fn watch_filter_selects_exact_names_and_empty_means_all() {
        let a = ServiceName::new("svc-a").unwrap();
        let b = ServiceName::new("svc-b").unwrap();
        let filter = HashSet::from([a.clone()]);
        assert!(matches_watch_filter(&filter, &a));
        assert!(!matches_watch_filter(&filter, &b));
        assert!(matches_watch_filter(&HashSet::new(), &b));
    }
}
