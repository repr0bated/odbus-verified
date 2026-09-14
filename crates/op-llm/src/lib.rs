//! op-llm: Multi-Provider LLM Integration
//!
//! ## Supported Providers & Endpoints
//!
//! | Provider | Base URL | Auth Method |
//! |----------|----------|-------------|
//! | Antigravity | Gemini API | Headless OAuth (captured from IDE) |
//! | Gemini | `https://generativelanguage.googleapis.com/v1beta` | API Key or OAuth |
//! | Anthropic | `https://api.anthropic.com/v1` | `x-api-key: {KEY}` |
//! | Perplexity | `https://api.perplexity.ai` | `Bearer {KEY}` |
//! | HuggingFace | `https://api-inference.huggingface.co` | `Bearer {HF_TOKEN}` |
//!
//! ## Authentication
//!
//! ### Option 1: Antigravity Headless (Recommended for Enterprise)
//! ```bash
//! # Start Antigravity service
//! sudo systemctl start antigravity-display antigravity-vnc
//!
//! # Connect via VNC and login once
//! vncviewer localhost:5900
//!
//! # Extract token
//! ./scripts/antigravity-extract-token.sh
//!
//! # Configure
//! export GOOGLE_AUTH_TOKEN_FILE=~/.config/antigravity/token.json
//! export LLM_PROVIDER=antigravity
//! ```
//!
//! ### Option 2: API Keys
//! ```bash
//! export GEMINI_API_KEY=xxx           # Google Gemini  
//! export ANTHROPIC_API_KEY=sk-xxx     # Anthropic Claude
//! export PERPLEXITY_API_KEY=pplx-xxx  # Perplexity
//! export HF_TOKEN=hf_xxx              # HuggingFace
//! ```

pub mod anthropic;
pub mod antigravity;
pub mod assistant;
pub mod chat;
pub mod factory;
pub mod gcloud_adc;
pub mod gemini;
pub mod gemini_cli;
pub mod headless_oauth;
pub mod huggingface;
pub mod openclaw;
pub mod perplexity;
pub mod provider;
pub mod pty_bridge;
pub mod salad;
pub mod schema;

pub use anthropic::AnthropicClient;
pub use antigravity::AntigravityProvider;
pub use assistant::AssistantProvider;
pub use factory::FactoryProvider;
pub use gcloud_adc::GCloudADCProvider;
pub use gemini::GeminiClient;
pub use headless_oauth::{HeadlessOAuthProvider, OAuthToken};
pub use huggingface::HuggingFaceClient;
pub use openclaw::OpenClawProvider;
pub use perplexity::PerplexityClient;
pub use provider::{
    ChatMessage, ChatRequest, ChatResponse, LlmProvider, ModelInfo, ProviderConfig, ProviderType,
    ToolChoice, ToolDefinition,
};
pub use salad::SaladProvider;

/// Prelude for convenient imports
pub mod prelude {
    pub use super::anthropic::AnthropicClient;
    pub use super::antigravity::AntigravityProvider;
    pub use super::assistant::AssistantProvider;
    pub use super::factory::FactoryProvider;
    pub use super::gcloud_adc::GCloudADCProvider;
    pub use super::gemini::GeminiClient;
    pub use super::headless_oauth::{HeadlessOAuthProvider, OAuthToken};
    pub use super::huggingface::HuggingFaceClient;
    pub use super::openclaw::OpenClawProvider;
    pub use super::perplexity::PerplexityClient;
    pub use super::provider::{
        ChatMessage, ChatRequest, ChatResponse, LlmProvider, ModelInfo, ProviderConfig,
        ProviderType, ToolChoice, ToolDefinition,
    };
    pub use super::salad::SaladProvider;
}
