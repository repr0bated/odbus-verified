use super::*;
use handler::handler_service_server::{HandlerService, HandlerServiceServer};
use op_state::StatePlugin;
use serde_json::json;
use std::{collections::HashMap, sync::Mutex};
use tokio::task::JoinHandle;
use tonic::{Response, Status};

use commander_client::stats;
use stats::stats_service_server::{StatsService, StatsServiceServer};

#[derive(Clone, Default)]
struct MockStats(Arc<Mutex<Vec<stats::GetStatsRequest>>>);

#[tonic::async_trait]
impl StatsService for MockStats {
    async fn get_stats(
        &self,
        request: Request<stats::GetStatsRequest>,
    ) -> std::result::Result<Response<stats::GetStatsResponse>, Status> {
        assert!(request.metadata().contains_key("grpc-timeout"));
        let input = request.into_inner();
        assert!(!input.reset, "reads must never reset a native counter");
        self.0.lock().unwrap().push(input.clone());
        let stat = match input.name.as_str() {
            "inbound>>>fixture>>>traffic>>>uplink" => Some(stats::Stat {
                name: input.name,
                value: 8192,
            }),
            "wrong-counter" => Some(stats::Stat {
                name: "another-counter".into(),
                value: 0,
            }),
            "unavailable" => return Err(Status::unavailable("fixture unavailable")),
            "slow" => {
                tokio::time::sleep(Duration::from_secs(1)).await;
                None
            }
            _ => None,
        };
        Ok(Response::new(stats::GetStatsResponse { stat }))
    }
    async fn get_stats_online(
        &self,
        _: Request<stats::GetStatsRequest>,
    ) -> std::result::Result<Response<stats::GetStatsResponse>, Status> {
        Err(Status::unimplemented("outside fixture"))
    }
    async fn query_stats(
        &self,
        _: Request<stats::QueryStatsRequest>,
    ) -> std::result::Result<Response<stats::QueryStatsResponse>, Status> {
        Err(Status::unimplemented("outside fixture"))
    }
    async fn get_sys_stats(
        &self,
        _: Request<stats::SysStatsRequest>,
    ) -> std::result::Result<Response<stats::SysStatsResponse>, Status> {
        Err(Status::unimplemented("outside fixture"))
    }
    async fn get_stats_online_ip_list(
        &self,
        _: Request<stats::GetStatsRequest>,
    ) -> std::result::Result<Response<stats::GetStatsOnlineIpListResponse>, Status> {
        Err(Status::unimplemented("outside fixture"))
    }
    async fn get_all_online_users(
        &self,
        _: Request<stats::GetAllOnlineUsersRequest>,
    ) -> std::result::Result<Response<stats::GetAllOnlineUsersResponse>, Status> {
        Err(Status::unimplemented("outside fixture"))
    }
}

#[tokio::test]
async fn stats_read_exact_native_counter_without_reset_and_bound_failures() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("stats.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let stats = MockStats::default();
    let service = stats.clone();
    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(StatsServiceServer::new(service))
            .serve_with_incoming(tokio_stream::wrappers::UnixListenerStream::new(listener))
            .await
            .unwrap();
    });
    let socket = socket.to_str().unwrap();
    let name = "inbound>>>fixture>>>traffic>>>uplink";
    for _ in 0..2 {
        let output = dispatch_stats_at(socket, &json!({"name":name}), CALL_TIMEOUT)
            .await
            .unwrap();
        assert_eq!(output, json!({"name":name,"value":8192}));
    }
    assert_eq!(stats.0.lock().unwrap().len(), 2);
    for (name, expected) in [
        ("missing", "no such stat counter"),
        ("wrong-counter", "different stat counter"),
        ("unavailable", "fixture unavailable"),
    ] {
        let error = dispatch_stats_at(socket, &json!({"name":name}), CALL_TIMEOUT)
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains(expected));
    }
    let before = stats.0.lock().unwrap().len();
    for input in [
        json!({}),
        json!({"name":""}),
        json!({"name":"bad\ncounter"}),
    ] {
        assert!(
            dispatch_stats_at(socket, &input, CALL_TIMEOUT)
                .await
                .is_err()
        );
    }
    assert_eq!(stats.0.lock().unwrap().len(), before);
    assert!(
        dispatch_stats_at(socket, &json!({"name":"slow"}), Duration::from_millis(50))
            .await
            .is_err()
    );
    server.abort();
}

#[derive(Default)]
struct MockState {
    inbounds: HashMap<String, xray::core::InboundHandlerConfig>,
    users: HashMap<String, Vec<xray::common::protocol::User>>,
    altered: Vec<handler::AlterInboundRequest>,
    added: Vec<handler::AddInboundRequest>,
    deadlines: Vec<String>,
    fail: Option<tonic::Code>,
    mutation_delay: Duration,
}

#[derive(Clone, Default)]
struct MockHandler(Arc<Mutex<MockState>>);

impl MockHandler {
    fn observe_deadline<T>(&self, request: &Request<T>) {
        let deadline = request
            .metadata()
            .get("grpc-timeout")
            .expect("native request must carry a deadline")
            .to_str()
            .unwrap()
            .to_owned();
        self.0.lock().unwrap().deadlines.push(deadline);
    }
    async fn before_mutation(&self) -> std::result::Result<(), Status> {
        let (delay, fail) = {
            let state = self.0.lock().unwrap();
            (state.mutation_delay, state.fail)
        };
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        if let Some(code) = fail {
            return Err(Status::new(code, "fixture native handler rejection"));
        }
        Ok(())
    }
}

#[tonic::async_trait]
impl HandlerService for MockHandler {
    async fn add_inbound(
        &self,
        request: Request<handler::AddInboundRequest>,
    ) -> std::result::Result<Response<handler::AddInboundResponse>, Status> {
        self.observe_deadline(&request);
        self.before_mutation().await?;
        let request = request.into_inner();
        let inbound = request
            .inbound
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("missing inbound"))?;
        let mut state = self.0.lock().unwrap();
        if state.inbounds.contains_key(&inbound.tag) {
            return Err(Status::already_exists("inbound exists"));
        }
        state.inbounds.insert(inbound.tag.clone(), inbound.clone());
        state.added.push(request);
        Ok(Response::new(handler::AddInboundResponse {}))
    }

    async fn alter_inbound(
        &self,
        request: Request<handler::AlterInboundRequest>,
    ) -> std::result::Result<Response<handler::AlterInboundResponse>, Status> {
        self.observe_deadline(&request);
        self.before_mutation().await?;
        let request = request.into_inner();
        let mut state = self.0.lock().unwrap();
        if !state.inbounds.contains_key(&request.tag) {
            return Err(Status::not_found("inbound missing"));
        }
        let operation = request
            .operation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("missing operation"))?;
        match operation.r#type.as_str() {
            "xray.app.proxyman.command.AddUserOperation" => {
                let add = handler::AddUserOperation::decode(operation.value.as_slice())
                    .map_err(|_| Status::invalid_argument("invalid add protobuf"))?;
                state.users.entry(request.tag.clone()).or_default().push(
                    add.user
                        .ok_or_else(|| Status::invalid_argument("missing user"))?,
                );
            }
            "xray.app.proxyman.command.RemoveUserOperation" => {
                let remove = handler::RemoveUserOperation::decode(operation.value.as_slice())
                    .map_err(|_| Status::invalid_argument("invalid remove protobuf"))?;
                let users = state.users.entry(request.tag.clone()).or_default();
                let before = users.len();
                users.retain(|user| !user.email.eq_ignore_ascii_case(&remove.email));
                if before == users.len() {
                    return Err(Status::not_found("user missing"));
                }
            }
            _ => return Err(Status::invalid_argument("wrong operation type")),
        }
        state.altered.push(request);
        Ok(Response::new(handler::AlterInboundResponse {}))
    }

    async fn list_inbounds(
        &self,
        request: Request<handler::ListInboundsRequest>,
    ) -> std::result::Result<Response<handler::ListInboundsResponse>, Status> {
        self.observe_deadline(&request);
        assert!(!request.get_ref().is_only_tags);
        Ok(Response::new(handler::ListInboundsResponse {
            inbounds: self.0.lock().unwrap().inbounds.values().cloned().collect(),
        }))
    }

    async fn get_inbound_users(
        &self,
        request: Request<handler::GetInboundUserRequest>,
    ) -> std::result::Result<Response<handler::GetInboundUserResponse>, Status> {
        self.observe_deadline(&request);
        assert!(request.get_ref().email.is_empty());
        Ok(Response::new(handler::GetInboundUserResponse {
            users: self
                .0
                .lock()
                .unwrap()
                .users
                .get(&request.get_ref().tag)
                .cloned()
                .unwrap_or_default(),
        }))
    }

    async fn remove_inbound(
        &self,
        _: Request<handler::RemoveInboundRequest>,
    ) -> std::result::Result<Response<handler::RemoveInboundResponse>, Status> {
        Err(Status::unimplemented("outside fixture"))
    }
    async fn get_inbound_users_count(
        &self,
        _: Request<handler::GetInboundUserRequest>,
    ) -> std::result::Result<Response<handler::GetInboundUsersCountResponse>, Status> {
        Err(Status::unimplemented("outside fixture"))
    }
    async fn add_outbound(
        &self,
        _: Request<handler::AddOutboundRequest>,
    ) -> std::result::Result<Response<handler::AddOutboundResponse>, Status> {
        Err(Status::unimplemented("outside fixture"))
    }
    async fn remove_outbound(
        &self,
        _: Request<handler::RemoveOutboundRequest>,
    ) -> std::result::Result<Response<handler::RemoveOutboundResponse>, Status> {
        Err(Status::unimplemented("outside fixture"))
    }
    async fn alter_outbound(
        &self,
        _: Request<handler::AlterOutboundRequest>,
    ) -> std::result::Result<Response<handler::AlterOutboundResponse>, Status> {
        Err(Status::unimplemented("outside fixture"))
    }
    async fn list_outbounds(
        &self,
        _: Request<handler::ListOutboundsRequest>,
    ) -> std::result::Result<Response<handler::ListOutboundsResponse>, Status> {
        Err(Status::unimplemented("outside fixture"))
    }
}

struct Fixture {
    _directory: tempfile::TempDir,
    socket: String,
    mock: MockHandler,
    server: JoinHandle<()>,
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("xray.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let mock = MockHandler::default();
        let service = mock.clone();
        let server = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(HandlerServiceServer::new(service))
                .serve_with_incoming(tokio_stream::wrappers::UnixListenerStream::new(listener))
                .await
                .unwrap();
        });
        Self {
            _directory: directory,
            socket: socket.to_str().unwrap().to_owned(),
            mock,
            server,
        }
    }

    fn seed(&self, tag: &str, protocol: XrayHandlerProtocol) {
        self.mock.0.lock().unwrap().inbounds.insert(
            tag.to_owned(),
            xray::core::InboundHandlerConfig {
                tag: tag.to_owned(),
                proxy_settings: Some(TypedMessage {
                    r#type: protocol_config_type(protocol).to_owned(),
                    value: vec![],
                }),
                receiver_settings: Some(typed(
                    RECEIVER,
                    xray::app::proxyman::ReceiverConfig::default(),
                )),
            },
        );
    }

    async fn call(&self, method: &str, args: Value) -> Result<Value> {
        let schema = op_plugins::state_plugins::xray::XrayPlugin::new(Default::default())
            .schema()
            .unwrap();
        assert_eq!(schema.version, "4.0.0");
        let input = serde_json::to_value(&schema.methods[method].args).unwrap();
        jsonschema::Validator::new(&input)
            .unwrap()
            .validate(&args)
            .unwrap();
        let result = dispatch_at(&self.socket, CALL_TIMEOUT, method, &args).await?;
        let output =
            serde_json::to_value(schema.methods[method].returns.as_ref().unwrap()).unwrap();
        jsonschema::Validator::new(&output)
            .unwrap()
            .validate(&result)
            .unwrap();
        Ok(result)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn user(protocol: &str) -> Value {
    let account = if protocol == "trojan" {
        json!({"protocol":"trojan","password":"fixture-secret-password"})
    } else {
        json!({"protocol":protocol,"id":"12345678-1234-4567-89ab-123456789abc"})
    };
    json!({"email":"fixture@example.test","level":7,"account":account})
}

fn inbound(protocol: &str) -> Value {
    json!({"tag":format!("{protocol}-fixture"),"listen_ip":"127.0.0.1","port":12345,
        "protocol":protocol,"users":[user(protocol)],"transport":{"security":"loopback_plaintext"}})
}

fn tls_material() -> (String, String) {
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
    (certificate.cert.pem(), certificate.key_pair.serialize_pem())
}

#[tokio::test]
async fn add_and_remove_each_account_use_native_alter_inbound_envelopes() {
    let fixture = Fixture::new().await;
    for (name, protocol, account_type) in [
        ("vless", XrayHandlerProtocol::Vless, VLESS_ACCOUNT),
        ("vmess", XrayHandlerProtocol::Vmess, VMESS_ACCOUNT),
        ("trojan", XrayHandlerProtocol::Trojan, TROJAN_ACCOUNT),
    ] {
        fixture.seed(name, protocol);
        let result = fixture
            .call("add_user", json!({"inbound_tag":name,"user":user(name)}))
            .await
            .unwrap();
        assert_eq!(
            result,
            json!({"inbound_tag":name,"email":"fixture@example.test","protocol":name,"level":7})
        );
        let account = fixture.mock.0.lock().unwrap().users[name][0]
            .account
            .clone()
            .unwrap();
        assert_eq!(account.r#type, account_type);
        match protocol {
            XrayHandlerProtocol::Vless => {
                let value = xray::proxy::vless::Account::decode(account.value.as_slice()).unwrap();
                assert_eq!(value.id, "12345678-1234-4567-89ab-123456789abc");
                assert!(value.encryption.is_empty());
            }
            XrayHandlerProtocol::Vmess => {
                let value = xray::proxy::vmess::Account::decode(account.value.as_slice()).unwrap();
                assert_eq!(value.id, "12345678-1234-4567-89ab-123456789abc");
            }
            XrayHandlerProtocol::Trojan => {
                let value = xray::proxy::trojan::Account::decode(account.value.as_slice()).unwrap();
                assert_eq!(value.password, "fixture-secret-password");
            }
        }
        let removed = fixture
            .call(
                "remove_user",
                json!({"inbound_tag":name,"email":"fixture@example.test"}),
            )
            .await
            .unwrap();
        assert_eq!(
            removed,
            json!({"inbound_tag":name,"email":"fixture@example.test"})
        );
        assert!(fixture.mock.0.lock().unwrap().users[name].is_empty());
    }
    let state = fixture.mock.0.lock().unwrap();
    assert_eq!(state.altered.len(), 6);
    assert_eq!(state.deadlines.len(), 12);
}

#[tokio::test]
async fn add_inbounds_encode_real_receiver_and_protocol_configs() {
    let fixture = Fixture::new().await;
    for name in ["vless", "vmess", "trojan"] {
        let result = fixture.call("add_inbound", inbound(name)).await.unwrap();
        assert_eq!(result["protocol"], name);
        assert_eq!(result["user_count"], 1);
        assert_eq!(result["security"], "loopback_plaintext");
        assert!(!result.to_string().contains("fixture-secret"));
    }
    let state = fixture.mock.0.lock().unwrap();
    assert_eq!(state.added.len(), 3);
    for request in &state.added {
        let config = request.inbound.as_ref().unwrap();
        let receiver = config.receiver_settings.as_ref().unwrap();
        assert_eq!(receiver.r#type, RECEIVER);
        let receiver =
            xray::app::proxyman::ReceiverConfig::decode(receiver.value.as_slice()).unwrap();
        assert_eq!(
            receiver.port_list.unwrap().range,
            vec![xray::common::net::PortRange {
                from: 12345,
                to: 12345
            }]
        );
        assert_eq!(
            receiver.listen.unwrap().address,
            Some(xray::common::net::ip_or_domain::Address::Ip(vec![
                127, 0, 0, 1
            ]))
        );
        assert_eq!(receiver.stream_settings.unwrap().protocol_name, "tcp");
        let proxy = config.proxy_settings.as_ref().unwrap();
        let users = match proxy.r#type.as_str() {
            VLESS_INBOUND => {
                let value =
                    xray::proxy::vless::inbound::Config::decode(proxy.value.as_slice()).unwrap();
                assert_eq!(value.decryption, "none");
                value.clients
            }
            VMESS_INBOUND => {
                xray::proxy::vmess::inbound::Config::decode(proxy.value.as_slice())
                    .unwrap()
                    .user
            }
            TROJAN_INBOUND => {
                xray::proxy::trojan::ServerConfig::decode(proxy.value.as_slice())
                    .unwrap()
                    .users
            }
            _ => panic!("unrecognized native config type"),
        };
        assert_eq!(users[0].email, "fixture@example.test");
    }
}

#[tokio::test]
async fn tls_material_is_validated_then_encoded_inline_without_paths() {
    let fixture = Fixture::new().await;
    let (certificate, key) = tls_material();
    let mut args = inbound("vless");
    args["listen_ip"] = json!("::");
    args["users"][0]["account"]["flow"] = json!("xtls-rprx-vision");
    args["transport"] =
        json!({"security":"tls","certificate_pem":certificate,"private_key_pem":key,"alpn":["h2"]});
    let result = fixture.call("add_inbound", args).await.unwrap();
    assert_eq!(result["security"], "tls");
    assert!(!result.to_string().contains("PRIVATE KEY"));
    let state = fixture.mock.0.lock().unwrap();
    let receiver = state.added[0]
        .inbound
        .as_ref()
        .unwrap()
        .receiver_settings
        .as_ref()
        .unwrap();
    let receiver = xray::app::proxyman::ReceiverConfig::decode(receiver.value.as_slice()).unwrap();
    let stream = receiver.stream_settings.unwrap();
    assert_eq!(stream.security_type, TLS);
    assert_eq!(stream.security_settings[0].r#type, TLS);
    let tls = xray::transport::internet::tls::Config::decode(
        stream.security_settings[0].value.as_slice(),
    )
    .unwrap();
    assert_eq!(tls.min_version, "1.3");
    assert_eq!(tls.next_protocol, vec!["h2"]);
    assert_eq!(tls.certificate[0].certificate, certificate.as_bytes());
    assert_eq!(tls.certificate[0].key, key.as_bytes());
    assert!(tls.certificate[0].certificate_path.is_empty());
    assert!(tls.certificate[0].key_path.is_empty());
}

#[tokio::test]
async fn invalid_inputs_and_tls_keys_have_zero_native_requests() {
    let fixture = Fixture::new().await;
    let (certificate, _) = tls_material();
    let (_, wrong_key) = tls_material();
    let mut invalids = Vec::new();
    let mut args = inbound("vless");
    args["listen_ip"] = json!("0.0.0.0");
    invalids.push(args);
    let mut args = inbound("vless");
    args["port"] = json!(0);
    invalids.push(args);
    let mut args = inbound("vless");
    args["unknown_raw_config"] = json!({});
    invalids.push(args);
    let mut args = inbound("vless");
    args["users"][0]["account"]["id"] = json!("bad-uuid");
    invalids.push(args);
    let mut args = inbound("vless");
    args["users"][0] = user("trojan");
    invalids.push(args);
    let mut args = inbound("vless");
    args["users"][0]["account"]["flow"] = json!("xtls-rprx-vision");
    invalids.push(args);
    let mut args = inbound("vless");
    args["transport"] = json!({"security":"reality","private_key":"ignored"});
    invalids.push(args);
    let mut args = inbound("vless");
    args["transport"] =
        json!({"security":"tls","certificate_pem":certificate,"private_key_pem":wrong_key});
    invalids.push(args);
    let mut args = inbound("trojan");
    args["users"][0]["account"]["password"] = json!("");
    invalids.push(args);
    for args in invalids {
        assert!(
            dispatch_at(&fixture.socket, CALL_TIMEOUT, "add_inbound", &args)
                .await
                .is_err()
        );
    }
    let state = fixture.mock.0.lock().unwrap();
    assert!(state.deadlines.is_empty());
    assert!(state.added.is_empty());
}

#[tokio::test]
async fn mismatched_missing_and_duplicate_user_targets_never_mutate() {
    let fixture = Fixture::new().await;
    fixture.seed("target", XrayHandlerProtocol::Trojan);
    let wrong = json!({"inbound_tag":"target","user":user("vless")});
    assert!(
        dispatch_at(&fixture.socket, CALL_TIMEOUT, "add_user", &wrong)
            .await
            .is_err()
    );
    let missing = json!({"inbound_tag":"missing","user":user("trojan")});
    assert!(
        dispatch_at(&fixture.socket, CALL_TIMEOUT, "add_user", &missing)
            .await
            .is_err()
    );
    fixture
        .call(
            "add_user",
            json!({"inbound_tag":"target","user":user("trojan")}),
        )
        .await
        .unwrap();
    let mut alias = user("trojan");
    alias["email"] = json!("different@example.test");
    let duplicate = json!({"inbound_tag":"target","user":alias});
    let error = dispatch_at(&fixture.socket, CALL_TIMEOUT, "add_user", &duplicate)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("credential already exists"));
    let state = fixture.mock.0.lock().unwrap();
    assert_eq!(state.altered.len(), 1);
    assert_eq!(state.users["target"].len(), 1);
}

#[tokio::test]
async fn malformed_native_account_response_prevents_user_mutation() {
    let fixture = Fixture::new().await;
    fixture.seed("target", XrayHandlerProtocol::Vless);
    fixture.mock.0.lock().unwrap().users.insert(
        "target".into(),
        vec![xray::common::protocol::User {
            email: "existing@example.test".into(),
            level: 0,
            account: Some(TypedMessage {
                r#type: VLESS_ACCOUNT.into(),
                value: vec![0xff],
            }),
        }],
    );
    assert!(
        dispatch_at(
            &fixture.socket,
            CALL_TIMEOUT,
            "add_user",
            &json!({"inbound_tag":"target","user":user("vless")})
        )
        .await
        .is_err()
    );
    assert!(fixture.mock.0.lock().unwrap().altered.is_empty());
}

#[test]
fn credential_and_tls_bounds_are_enforced_before_transport() {
    assert!(build_tls(&"x".repeat(262_145), "key", &[]).is_err());
    assert!(build_tls("certificate", &"x".repeat(65_537), &[]).is_err());
    assert!(build_tls("certificate", "key", &vec!["h2".into(); 17]).is_err());
    let mut value = user("trojan");
    value["account"]["password"] = json!("x".repeat(4097));
    let typed: XrayHandlerUser = serde_json::from_value(value).unwrap();
    assert!(build_user(&typed).is_err());
    assert!(
        serde_json::from_value::<RemoveUserInput>(
            json!({"inbound_tag":"tag","email":"key","protocol":"trojan"})
        )
        .is_err()
    );
}

#[test]
fn vless_masked_uuid_alias_is_rejected_before_new_inbound() {
    let mut args = inbound("vless");
    let mut alias = user("vless");
    alias["email"] = json!("second@example.test");
    alias["account"]["id"] = json!("12345678-1234-ffff-89ab-123456789abc");
    args["users"].as_array_mut().unwrap().push(alias);
    let input: AddInboundInput = serde_json::from_value(args).unwrap();
    assert!(
        build_inbound(&input)
            .unwrap_err()
            .to_string()
            .contains("duplicate account credential")
    );
}

#[tokio::test]
async fn native_errors_are_preserved_and_deadline_cancels_wait() {
    let fixture = Fixture::new().await;
    fixture.seed("target", XrayHandlerProtocol::Trojan);
    fixture.mock.0.lock().unwrap().fail = Some(tonic::Code::PermissionDenied);
    let error = dispatch_at(
        &fixture.socket,
        CALL_TIMEOUT,
        "remove_user",
        &json!({"inbound_tag":"target","email":"user@example.test"}),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.downcast_ref::<Status>().unwrap().code(),
        tonic::Code::PermissionDenied
    );
    {
        let mut state = fixture.mock.0.lock().unwrap();
        state.fail = None;
        state.mutation_delay = Duration::from_secs(1);
    }
    let error = dispatch_at(
        &fixture.socket,
        Duration::from_millis(30),
        "add_inbound",
        &inbound("trojan"),
    )
    .await
    .unwrap_err();
    assert!(format!("{error:#}").contains("timed out") || error.downcast_ref::<Status>().is_some());
    assert!(fixture.mock.0.lock().unwrap().added.is_empty());
}

#[tokio::test]
async fn concurrent_duplicate_adds_are_serialized_before_native_effects() {
    let fixture = Fixture::new().await;
    fixture.seed("target", XrayHandlerProtocol::Vless);
    let first = json!({"inbound_tag":"target","user":user("vless")});
    let mut second = first.clone();
    second["user"]["email"] = json!("second@example.test");
    let (a, b) = tokio::join!(
        dispatch_at(&fixture.socket, CALL_TIMEOUT, "add_user", &first),
        dispatch_at(&fixture.socket, CALL_TIMEOUT, "add_user", &second),
    );
    assert_ne!(a.is_ok(), b.is_ok());
    assert_eq!(fixture.mock.0.lock().unwrap().altered.len(), 1);
}
