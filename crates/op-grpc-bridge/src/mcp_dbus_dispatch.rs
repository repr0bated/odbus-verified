//! MCP is an ingress to the existing canonical PluginV1 D-Bus owner.
//!
//! Each call owns a private socket pair, so no shared host/root sender can
//! substitute its identity. There is no listener, bootstrap method, credential,
//! grant or independent schema. Only an already-authenticated MCP caller can
//! construct this receiver binding; identity is never read from call arguments.
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use zbus::{Connection, Proxy};

use crate::interceptor::GhostbridgeIdentity;
use crate::mcp_frontend::AuthenticatedCaller;
use crate::mutation_engine::MutationEngine;
use crate::schema_router::{SchemaBackedInterface, SchemaRouter};

pub(crate) struct McpDbusIdentity {
    engine: Arc<MutationEngine>,
    caller: AuthenticatedCaller,
}

impl McpDbusIdentity {
    pub(crate) async fn resolve(&self) -> Result<GhostbridgeIdentity> {
        let (principal, session, genesis, origin) = self.caller.identity_claims();
        ensure!(
            !principal.is_empty() && !session.is_empty() && !genesis.is_empty(),
            "MCP receiver identity is incomplete"
        );
        if let Some(origin) = origin {
            let checked = crate::external_mcp_session::verify_connected_session(
                &self.engine,
                principal,
                session,
                Some(genesis),
            )
            .await?
            .context("external MCP receiver session is disconnected")?;
            ensure!(checked == session, "external MCP receiver session changed");
            let row = crate::external_mcp_session::lookup(session)
                .await?
                .context("external MCP receiver anchor is unavailable")?;
            ensure!(row.origin == origin, "external MCP receiver origin changed");
            Ok(GhostbridgeIdentity {
                principal_id: principal.into(),
                session_id: session.into(),
                session_genesis: genesis.into(),
            })
        } else {
            let identity = crate::dbus_identity::authoritative_session_identity(
                &self.engine,
                Some(principal),
                session,
                false,
            )
            .await?;
            ensure!(
                identity.session_genesis == genesis,
                "native MCP receiver genesis changed"
            );
            Ok(identity)
        }
    }
}

async fn connection(
    engine: Arc<MutationEngine>,
    caller: AuthenticatedCaller,
    plugin: &str,
) -> Result<(Connection, Connection, String)> {
    let identity = Arc::new(McpDbusIdentity {
        engine: engine.clone(),
        caller,
    });
    // Fail before building any projection when the authenticated term changed.
    identity.resolve().await?;
    let catalog_dir = crate::sealed_schema_reader::blob_catalog_dir_from_env();
    let plugin_id = plugin.to_owned();
    let document = tokio::task::spawn_blocking(move || {
        crate::sealed_schema_reader::read_manifest_pinned_schema_document(&catalog_dir, &plugin_id)
    })
    .await??;
    let route = SchemaRouter::build_route(plugin, &serde_json::to_value(document.schema)?);
    let path = route.dbus_path.clone();
    let interface =
        SchemaBackedInterface::with_mcp_receiver(plugin.into(), route, engine, identity);
    let (server_stream, client_stream) = tokio::net::UnixStream::pair()?;
    let server = zbus::connection::Builder::unix_stream(server_stream)
        .server(zbus::Guid::generate())?
        .p2p()
        .auth_mechanism(zbus::connection::AuthMechanism::Anonymous)
        .serve_at(path.as_str(), interface)?
        .build();
    let client = zbus::connection::Builder::unix_stream(client_stream)
        .p2p()
        .auth_mechanism(zbus::connection::AuthMechanism::Anonymous)
        .build();
    let (server, client) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::try_join!(server, client)
    })
    .await
    .context("private MCP D-Bus negotiation timed out")??;
    Ok((server, client, path))
}

pub(crate) async fn dispatch(
    engine: Arc<MutationEngine>,
    caller: AuthenticatedCaller,
    plugin: &str,
    method: &str,
    arguments: Value,
) -> Result<Value> {
    let (_server, client, path) = connection(engine, caller, plugin).await?;
    let proxy = Proxy::new(
        &client,
        op_plugins::canonical::BASE_SERVICE_NAME,
        path.as_str(),
        "org.opdbus.v1.PluginV1",
    )
    .await?;
    let operation = uuid::Uuid::new_v4().to_string();
    // The existing canonical owner reserves/audits this correlation before
    // effects. Dropping an HTTP request cannot cancel its admitted owner task.
    let response: String = proxy
        .call(
            "CallWithOperationId",
            &(method, serde_json::to_string(&arguments)?, operation),
        )
        .await?;
    serde_json::from_str(&response).context("canonical MCP D-Bus reply is invalid JSON")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::human_principal_dispatch::tests::{pk, temp_shm, test_engine};
    use crate::oracle_assertion::AssertionSourceBinding;
    use crate::oracle_assertion::tests::{
        signed_with_fields, test_issuer, validator_with_registered,
    };
    use base64::Engine as _;

    struct GrantsEnvironment(Option<std::ffi::OsString>);
    impl Drop for GrantsEnvironment {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => unsafe { std::env::set_var("OP_GRANTS_PATH", value) },
                None => unsafe { std::env::remove_var("OP_GRANTS_PATH") },
            }
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn real_plugin_dbus_preserves_verified_peer_and_rechecks_grants_and_session() {
        let issuer = test_issuer();
        let key = pk(211);
        let (_registry, validator) = validator_with_registered(&issuer, &key).await;
        let _shm = temp_shm();
        let engine = test_engine();
        let root = tempfile::tempdir().unwrap();
        let grants_path = root.path().join("grants.json");
        let _environment = GrantsEnvironment(std::env::var_os("OP_GRANTS_PATH"));
        unsafe { std::env::set_var("OP_GRANTS_PATH", &grants_path) };
        let principal = op_identity::session::derive_principal_id(&key);
        std::fs::write(
            &grants_path,
            serde_json::to_vec(&serde_json::json!({
                principal.clone(): {"capabilities": ["human_principal.read"]}
            }))
            .unwrap(),
        )
        .unwrap();
        let now = chrono::Utc::now().timestamp();
        let proof = signed_with_fields(
            &issuer,
            &key,
            "10.0.0.5".parse().unwrap(),
            now - 1,
            now + 300,
            [81; 16],
            None,
        );
        let validator =
            Arc::new(validator.with_source_binding(AssertionSourceBinding::TrustedDecoySignature));
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            crate::mcp_frontend::HTTP_ASSERTION_HEADER,
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(proof.to_wire())
                .parse()
                .unwrap(),
        );
        let caller =
            crate::mcp_frontend::authenticate_dbus_fixture(engine.clone(), validator, &headers)
                .await
                .unwrap();
        let (_, session, genesis, _) = caller.identity_claims();
        let session = session.to_owned();
        let genesis = genesis.to_owned();
        let receiver = McpDbusIdentity {
            engine: engine.clone(),
            caller: caller.clone(),
        };
        assert_eq!(
            receiver.resolve().await.unwrap(),
            GhostbridgeIdentity {
                principal_id: principal.clone(),
                session_id: session.clone(),
                session_genesis: genesis.clone(),
            }
        );

        // Real wire negotiation and canonical interface admission precede the
        // existing durable effect owner. This fixture installs its own owners.
        let documents = Arc::new(
            op_plugins::state_plugins::runtime_documents::RuntimeDocuments::new(
                op_cozo_store::CozoGraphShuttle::new_persistent_private(
                    root.path().join("journal"),
                )
                .unwrap(),
            ),
        );
        engine
            .configure_method_journal_documents(documents)
            .unwrap();
        engine
            .init_audit_durability_at(root.path().join("audit").to_str().unwrap())
            .await;
        let result = dispatch(
            engine.clone(),
            caller.clone(),
            "human_principal",
            "resolve_key",
            serde_json::json!({"human_pubkey": key}),
        )
        .await
        .unwrap();
        assert_eq!(result["result"]["principal"]["principal_id"], principal);
        assert_eq!(
            result["session_id"],
            Value::Null,
            "identity never comes from returned tool arguments"
        );

        let (_server, client, path) = connection(engine.clone(), caller.clone(), "human_principal")
            .await
            .unwrap();
        let proxy = Proxy::new(
            &client,
            op_plugins::canonical::BASE_SERVICE_NAME,
            path.as_str(),
            "org.opdbus.v1.PluginV1",
        )
        .await
        .unwrap();
        std::fs::write(
            &grants_path,
            serde_json::to_vec(&serde_json::json!({
                principal.clone(): {"capabilities": ["cognitive_mcp.read"]}
            }))
            .unwrap(),
        )
        .unwrap();
        let denied: zbus::Result<String> = proxy
            .call(
                "CallWithOperationId",
                &(
                    "resolve_key",
                    serde_json::json!({"human_pubkey": key}).to_string(),
                    uuid::Uuid::new_v4().to_string(),
                ),
            )
            .await;
        assert!(
            denied.unwrap_err().to_string().contains("not granted"),
            "live method grants are checked at the D-Bus owner"
        );

        let mut replaced = engine.session_context(&session).await.unwrap();
        replaced.genesis_hex = "other-session-genesis".into();
        engine.register_external_mcp_context(replaced).await;
        let rejected: zbus::Result<String> = proxy
            .call(
                "CallWithOperationId",
                &(
                    "resolve_key",
                    serde_json::json!({"human_pubkey": key}).to_string(),
                    uuid::Uuid::new_v4().to_string(),
                ),
            )
            .await;
        assert!(
            rejected
                .unwrap_err()
                .to_string()
                .contains("session binding"),
            "the peer cannot inherit a changed receiver session"
        );
        let bootstrap: zbus::Result<String> = proxy.call("Bind", &(session,)).await;
        assert!(
            bootstrap.is_err(),
            "private gateway exposes no identity bootstrap"
        );
    }
}
