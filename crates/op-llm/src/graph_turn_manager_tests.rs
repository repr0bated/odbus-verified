use super::*;
use crate::{
    graph_turn_reminder::{SYSTEM_GRAPH_TURN_REMINDER, with_system_graph_reminder},
    provider::ToolChoice,
};
use std::sync::Mutex;

struct RecordingProvider(Arc<Mutex<Vec<ChatRequest>>>);
#[async_trait]
impl LlmProvider for RecordingProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Custom("fixture".into())
    }
    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        Ok(vec![])
    }
    async fn search_models(&self, _query: &str, _limit: usize) -> Result<Vec<ModelInfo>> {
        Ok(vec![])
    }
    async fn get_model(&self, _model: &str) -> Result<Option<ModelInfo>> {
        Ok(None)
    }
    async fn is_model_available(&self, _model: &str) -> Result<bool> {
        Ok(true)
    }
    async fn chat(&self, model: &str, messages: Vec<ChatMessage>) -> Result<ChatResponse> {
        self.chat_with_request(model, ChatRequest::new(messages))
            .await
    }
    async fn chat_with_request(&self, model: &str, request: ChatRequest) -> Result<ChatResponse> {
        self.0.lock().unwrap().push(request);
        Ok(ChatResponse {
            message: ChatMessage::assistant("fixture response"),
            model: model.into(),
            provider: "fixture".into(),
            finish_reason: None,
            usage: None,
            tool_calls: None,
        })
    }
    async fn chat_stream(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
    ) -> Result<tokio::sync::mpsc::Receiver<Result<String>>> {
        self.chat(model, messages).await?;
        let (_sender, receiver) = tokio::sync::mpsc::channel(1);
        Ok(receiver)
    }
}

#[tokio::test]
async fn graph_turn_reminder_reaches_all_six_manager_boundaries_once_per_inference() {
    let recorded = Arc::new(Mutex::new(vec![]));
    let provider = ProviderType::Custom("fixture".into());
    let manager = ChatManager {
        providers: HashMap::from([(
            provider.clone(),
            Box::new(RecordingProvider(recorded.clone())) as BoxedProvider,
        )]),
        current_provider: Arc::new(RwLock::new(provider.clone())),
        current_model: Arc::new(RwLock::new("fixture-model".into())),
        current_model_non_sandboxed: Arc::new(RwLock::new(false)),
        model_cache: Arc::new(RwLock::new(HashMap::new())),
    };
    let messages = vec![
        ChatMessage::system("Caller instructions"),
        ChatMessage::user("Original question"),
    ];
    manager.chat(messages.clone()).await.unwrap();
    manager
        .chat_with(&provider, "fixture-model", messages.clone())
        .await
        .unwrap();
    manager
        .chat_stream_with(&provider, "fixture-model", messages.clone())
        .await
        .unwrap();
    LlmProvider::chat(&manager, "fixture-model", messages.clone())
        .await
        .unwrap();
    let mut request =
        ChatRequest::with_forced_tools(with_system_graph_reminder(messages.clone()), vec![]);
    request.max_tokens = Some(123);
    request.temperature = Some(0.25);
    LlmProvider::chat_with_request(&manager, "fixture-model", request)
        .await
        .unwrap();
    LlmProvider::chat_stream(&manager, "fixture-model", messages.clone())
        .await
        .unwrap();
    let recorded = recorded.lock().unwrap();
    assert_eq!(recorded.len(), 6);
    for request in recorded.iter() {
        assert_eq!(request.messages.len(), 2);
        assert_eq!(
            request.messages[0].content,
            format!("{SYSTEM_GRAPH_TURN_REMINDER}\nCaller instructions")
        );
        assert_eq!(
            request.messages[0]
                .content
                .matches(SYSTEM_GRAPH_TURN_REMINDER)
                .count(),
            1
        );
        assert_eq!(request.messages[1].content, "Original question");
    }
    assert!(matches!(recorded[4].tool_choice, ToolChoice::Required));
    assert_eq!(recorded[4].max_tokens, Some(123));
    assert_eq!(recorded[4].temperature, Some(0.25));
    assert_eq!(messages[0].content, "Caller instructions");
}
