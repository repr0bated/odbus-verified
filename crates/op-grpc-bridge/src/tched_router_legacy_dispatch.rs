//! Bridge integration for retained tched_router reads and durable role assignments.
//! Canonical admission/verified identity remains in MutationEngine.

use crate::{
    method_contract_admission,
    sealed_schema_reader::{
        blob_catalog_dir_from_env, read_manifest_pinned_schema_document,
        ManifestPinnedSchemaDocument,
    },
};
use anyhow::{Context, Result};
use op_plugins::state_plugins::{
    common::llm_projection::LlmTool,
    runtime_documents::RuntimeDocuments,
    tched_router::ObservedTchedRouterState,
    tched_router_legacy_methods::{ModelRole, TchedRouterLegacyMethods},
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub(crate) struct LegacyDispatchReceipt {
    pub(crate) output: Value,
    /// Exact runtime observation plus the exact durable assignment row used by
    /// this operation. MutationEngine commits this at the plugin root before
    /// notifying or returning the output.
    pub(crate) represented_state: Value,
    pub(crate) schema_hash: String,
}

#[derive(Clone)]
struct SealPin {
    schema_hash: String,
    schema_json: String,
}

fn seal_is_unchanged(expected: &SealPin) -> Result<()> {
    let current: ManifestPinnedSchemaDocument =
        read_manifest_pinned_schema_document(&blob_catalog_dir_from_env(), "tched_router")?;
    method_contract_admission::admit_compiled_state_contract(&current, "tched_router")?;
    anyhow::ensure!(
        current.schema_hash == expected.schema_hash
            && current.blob.schema_json == expected.schema_json,
        "tched_router plugin seal changed during retained operation"
    );
    Ok(())
}

async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(operation)
        .await
        .context("tched_router sealed-contract worker failed")?
}

fn role_for_method(method: &str) -> Option<ModelRole> {
    match method {
        "SetOvsRoutingModel" => Some(ModelRole::OvsRouting),
        "SetObfuscationModel" => Some(ModelRole::Obfuscation),
        "SetVectorizationModel" => Some(ModelRole::Vectorization),
        "SetQdrantRetrievalModel" => Some(ModelRole::QdrantRetrieval),
        "SetCozoRetrievalModel" => Some(ModelRole::CozoRetrieval),
        _ => None,
    }
}

fn role_output(role: ModelRole, model_id: &str) -> Value {
    match role {
        ModelRole::OvsRouting => json!({"ovs_routing": model_id}),
        ModelRole::Obfuscation => json!({"obfuscation": model_id}),
        ModelRole::Vectorization => json!({"vectorization": model_id}),
        ModelRole::QdrantRetrieval => json!({"qdrant_retrieval": model_id}),
        ModelRole::CozoRetrieval => json!({"cozo_retrieval": model_id}),
    }
}

fn model_id(args: &Value) -> Result<&str> {
    let object = args
        .as_object()
        .context("role assignment input must be an object")?;
    anyhow::ensure!(object.len() == 1, "role assignment accepts only model_id");
    object
        .get("model_id")
        .and_then(Value::as_str)
        .context("role assignment requires string model_id")
}

#[derive(Deserialize)]
struct NativeToolsResponse {
    tools: Vec<NativeTool>,
}

#[derive(Deserialize)]
struct NativeTool {
    name: String,
    description: String,
    parameters: Value,
    #[serde(default)]
    output: Option<Value>,
    // v0.8.4 also returns param_domains. The retained OP-DBUS LlmTool
    // contract does not declare that metadata, so it is deliberately not
    // represented rather than copied into an opaque extension.
}

async fn read_native_tools(sealed_identity: &str) -> Result<Vec<LlmTool>> {
    read_native_tools_with(
        &crate::tched_router_runtime::TchedRouterRuntimeClient::from_env()
            .with_sealed_identity(Some(sealed_identity)),
    )
    .await
}

async fn read_native_tools_with(
    client: &crate::tched_router_runtime::TchedRouterRuntimeClient,
) -> Result<Vec<LlmTool>> {
    use crate::tched_router_runtime::{GatewayJsonRequest, GatewayMethod, GatewayProfile};
    let response: NativeToolsResponse = client
        .send_gateway_json(GatewayJsonRequest {
            method: GatewayMethod::Get,
            relative_path: "/api/tools",
            query: &[],
            body: None,
            operation: "tched_router tools",
            profile: GatewayProfile::Config,
            mutation: false,
        })
        .await?;
    map_native_tools(response)
}

fn map_native_tools(response: NativeToolsResponse) -> Result<Vec<LlmTool>> {
    anyhow::ensure!(
        response.tools.len() <= 512,
        "native tool catalog exceeds 512 entries"
    );
    let mut names = std::collections::BTreeSet::new();
    response
        .tools
        .into_iter()
        .map(|tool| {
            anyhow::ensure!(
                !tool.name.trim().is_empty()
                    && tool.name.len() <= 256
                    && names.insert(tool.name.clone()),
                "native tool catalog contains an empty, oversized, or duplicate name"
            );
            anyhow::ensure!(
                tool.description.len() <= 16 * 1024,
                "native tool description exceeds bound"
            );
            anyhow::ensure!(
                tool.parameters.is_object(),
                "native tool parameters are not a JSON Schema object"
            );
            anyhow::ensure!(
                tool.output.as_ref().is_none_or(Value::is_object),
                "native tool output is not a JSON Schema object"
            );
            Ok(LlmTool {
                name: tool.name,
                description: tool.description,
                parameters: tool.parameters,
                response_schema: tool.output.unwrap_or(Value::Null),
            })
        })
        .collect()
}

async fn ensure_runtime_unchanged(
    pinned: &ManifestPinnedSchemaDocument,
    expected: &Value,
    sealed_identity: &str,
) -> Result<()> {
    let (_, current) =
        crate::tched_config_engine::coherent_observation(pinned, sealed_identity).await?;
    anyhow::ensure!(
        &current == expected,
        "native tched_router config/selection changed during retained operation"
    );
    Ok(())
}

pub(crate) async fn dispatch(
    pinned: &ManifestPinnedSchemaDocument,
    method: &str,
    args: &Value,
    documents: Option<Arc<RuntimeDocuments>>,
    sealed_identity: &str,
) -> Result<LegacyDispatchReceipt> {
    // The engine retains final admission and the operation guard for this exact pin.
    let pin = SealPin {
        schema_hash: pinned.schema_hash.clone(),
        schema_json: pinned.blob.schema_json.clone(),
    };
    let (native, complete_native) =
        crate::tched_config_engine::coherent_observation(pinned, sealed_identity).await?;
    if method == "GetTools" {
        let object = args
            .as_object()
            .context("GetTools input must be an object")?;
        anyhow::ensure!(object.is_empty(), "GetTools does not accept fields");
        let tools = read_native_tools(sealed_identity).await?;
        let tools_value = serde_json::to_value(&tools)?;
        let expected = pin.clone();
        blocking(move || seal_is_unchanged(&expected)).await?;
        return Ok(LegacyDispatchReceipt {
            output: json!({"tools": tools_value.clone()}),
            represented_state: {
                let mut fields = complete_native
                    .as_object()
                    .cloned()
                    .context("native router fields must be an object")?;
                fields.insert("tools".into(), tools_value);
                Value::Object(fields)
            },
            schema_hash: pin.schema_hash.clone(),
        });
    }
    let methods = TchedRouterLegacyMethods::new(
        documents.context("retained tched_router method requires the configured durable owner")?,
    );
    let observed = ObservedTchedRouterState::from(&native);
    if matches!(
        method,
        "GetModelRoutes" | "GetProviderCatalog" | "ResolveRoute"
    ) {
        let receipt = methods.observe(method, args, &observed).await?;
        ensure_runtime_unchanged(pinned, &complete_native, sealed_identity).await?;
        let expected = pin.clone();
        blocking(move || seal_is_unchanged(&expected)).await?;
        return Ok(LegacyDispatchReceipt {
            output: receipt.output,
            represented_state: with_native_config(receipt.represented_state, &complete_native)?,
            schema_hash: pin.schema_hash.clone(),
        });
    }
    if let Some(role) = role_for_method(method) {
        let requested = model_id(args)?;
        let (receipt, represented_state) = methods
            .assign(role, requested, &observed)
            .await
            .with_context(|| format!("{method} durable assignment failed"))?;
        ensure_runtime_unchanged(pinned, &complete_native, sealed_identity)
            .await
            .with_context(|| {
                format!(
                "{method} assignment committed but the native catalog changed before publication"
            )
            })?;
        let expected = pin.clone();
        blocking(move || seal_is_unchanged(&expected)).await.with_context(|| {
            format!("{method} durable assignment committed but its sealed publication contract changed")
        })?;
        return Ok(LegacyDispatchReceipt {
            output: role_output(role, &receipt.model_id),
            represented_state: with_native_config(represented_state, &complete_native)?,
            schema_hash: pin.schema_hash.clone(),
        });
    }
    anyhow::bail!("retained tched_router method has no native/durable adapter: {method}")
}

fn with_native_config(mut fields: Value, native: &Value) -> Result<Value> {
    let config = native
        .get("native_config")
        .context("coherent router observation omitted configuration")?
        .clone();
    fields
        .as_object_mut()
        .context("retained router fields must be an object")?
        .insert("native_config".into(), config);
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tools_adapter_reads_the_real_v084_http_envelope() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::with_capacity(4096);
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                assert!(request.len() < 4096, "fixture request exceeded bound");
                let mut chunk = [0_u8; 512];
                let read = socket.read(&mut chunk).await.unwrap();
                assert!(read != 0, "fixture request ended before headers");
                request.extend_from_slice(&chunk[..read]);
            }
            let request = String::from_utf8_lossy(&request);
            assert!(request.starts_with("GET /api/tools HTTP/1.1\r\n"));
            let body = r#"{"tools":[{"name":"state_get","description":"Read state","parameters":{"type":"object","additionalProperties":false},"output":{"type":"object"},"param_domains":{"key":["public"]}}]}"#;
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        body.len(), body
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let client = crate::tched_router_runtime::TchedRouterRuntimeClient::new(
            format!("http://{address}"),
            "fixture".into(),
            None,
        );
        let tools = read_native_tools_with(&client).await.unwrap();
        server.await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "state_get");
        assert_eq!(tools[0].parameters["additionalProperties"], false);
        assert_eq!(tools[0].response_schema["type"], "object");
    }

    #[test]
    fn native_tool_mapping_preserves_parameter_and_output_schemas() {
        let tools = map_native_tools(NativeToolsResponse {
            tools: vec![NativeTool {
                name: "state_get".into(),
                description: "Read state".into(),
                parameters: json!({"type":"object","additionalProperties":false}),
                output: Some(json!({"type":"object","required":["value"]})),
            }],
        })
        .unwrap();
        assert_eq!(tools[0].parameters["additionalProperties"], false);
        assert_eq!(tools[0].response_schema["required"], json!(["value"]));
    }

    #[test]
    fn malformed_or_duplicate_native_tools_are_rejected() {
        let duplicate = || NativeTool {
            name: "same".into(),
            description: String::new(),
            parameters: json!({"type":"object"}),
            output: None,
        };
        assert!(map_native_tools(NativeToolsResponse {
            tools: vec![duplicate(), duplicate()],
        })
        .is_err());
        assert!(map_native_tools(NativeToolsResponse {
            tools: vec![NativeTool {
                parameters: Value::String("not-schema".into()),
                ..duplicate()
            }],
        })
        .is_err());
    }
}
