use anyhow::Result;
use op_chat::{
    ChatActor,
    actor::{ChatActorConfig, RpcRequest},
};
use op_llm::chat::ChatManager;
use op_llm::provider::LlmProvider;
use simd_json::{OwnedValue as Value, prelude::*};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let provider: Arc<dyn LlmProvider> = Arc::new(ChatManager::new());

    // Create a minimal ChatActor instance just to get a handle
    let config = ChatActorConfig::default();
    let (_actor, handle) = ChatActor::new(config, provider).await?;

    // Send a ListTools request
    let response = handle
        .call(RpcRequest::ListTools {
            offset: None,
            limit: None,
        })
        .await?;

    if response.success {
        println!("Successfully listed tools:");
        if let Some(tools_value) = response.result
            && let Some(tools_array) = tools_value.get("tools").and_then(Value::as_array)
        {
            for tool_def in tools_array {
                if let Some(name) = tool_def.get("name").and_then(Value::as_str) {
                    println!("- {}", name);
                }
            }
        }
    } else {
        eprintln!(
            "Error listing tools: {}",
            response.error.unwrap_or_default()
        );
    }

    Ok(())
}
