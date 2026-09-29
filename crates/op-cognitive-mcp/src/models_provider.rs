//! Models and Providers registry.
//!
//! Maintains a queryable database of available LLM models, providers, and their configurations.
//! Updates from environment (bash_secrets) on startup and via periodic refresh.

use serde_json::{json, Value};

#[derive(Clone, Debug)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub kind: String, // "chat", "embedding", etc
    pub available: bool,
    pub identity: Option<String>, // LLM identity (e.g., "claude-haiku-4-5")
    pub provider_id: String,      // which provider this model is from
}

#[derive(Clone, Debug)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub kind: String, // "api", "local", etc
    pub models: Vec<Model>,
    pub configured: bool,
}

pub struct ModelsProvider {
    providers: Vec<Provider>,
    messaging_enabled: bool,
}

impl ModelsProvider {
    pub fn new() -> Self {
        Self {
            providers: vec![
                Provider {
                    id: "openai".to_string(),
                    name: "OpenAI".to_string(),
                    kind: "api".to_string(),
                    models: vec![
                        Model {
                            id: "gpt-4-turbo".to_string(),
                            name: "GPT-4 Turbo".to_string(),
                            kind: "chat".to_string(),
                            available: true,
                            identity: Some("gpt-4-turbo".to_string()),
                            provider_id: "openai".to_string(),
                        },
                        Model {
                            id: "gpt-3.5-turbo".to_string(),
                            name: "GPT-3.5 Turbo".to_string(),
                            kind: "chat".to_string(),
                            available: true,
                            identity: Some("gpt-3.5-turbo".to_string()),
                            provider_id: "openai".to_string(),
                        },
                    ],
                    configured: std::env::var("OPENAI_API_KEY").is_ok(),
                },
                Provider {
                    id: "openrouter".to_string(),
                    name: "OpenRouter".to_string(),
                    kind: "api".to_string(),
                    models: vec![
                        Model {
                            id: "openai/gpt-4-turbo".to_string(),
                            name: "GPT-4 Turbo".to_string(),
                            kind: "chat".to_string(),
                            available: true,
                            identity: Some("gpt-4-turbo".to_string()),
                            provider_id: "openrouter".to_string(),
                        },
                        Model {
                            id: "anthropic/claude-3-opus".to_string(),
                            name: "Claude 3 Opus".to_string(),
                            kind: "chat".to_string(),
                            available: true,
                            identity: Some("claude-3-opus".to_string()),
                            provider_id: "openrouter".to_string(),
                        },
                    ],
                    configured: std::env::var("OPENROUTER_API_KEY").is_ok(),
                },
                Provider {
                    id: "cheaper-inference".to_string(),
                    name: "Cheaper Inference".to_string(),
                    kind: "api".to_string(),
                    models: vec![Model {
                        id: "claude".to_string(),
                        name: "Claude".to_string(),
                        kind: "chat".to_string(),
                        available: true,
                        identity: Some("claude-haiku-4-5".to_string()),
                        provider_id: "cheaper-inference".to_string(),
                    }],
                    configured: std::env::var("CHEAPERINFERENE_API_KEY").is_ok(),
                },
            ],
            messaging_enabled: std::env::var("CLAUDE_CODE_MESSAGING_TOKEN").is_ok(),
        }
    }

    /// List all configured providers and their models.
    pub async fn list_providers(&self) -> Value {
        json!({
            "providers": self.providers.iter().map(|p| {
                json!({
                    "id": p.id,
                    "name": p.name,
                    "kind": p.kind,
                    "configured": p.configured,
                    "models": p.models.iter().map(|m| {
                        json!({
                            "id": m.id,
                            "name": m.name,
                            "kind": m.kind,
                            "available": m.available,
                            "identity": m.identity,
                            "provider_id": m.provider_id,
                        })
                    }).collect::<Vec<_>>(),
                })
            }).collect::<Vec<_>>(),
            "messaging": {
                "enabled": self.messaging_enabled,
            },
            "lastUpdated": chrono::Utc::now().to_rfc3339(),
        })
    }

    /// Get a specific provider.
    pub async fn get_provider(&self, id: &str) -> Option<Value> {
        self.providers.iter().find(|p| p.id == id).map(|p| {
            json!({
                "id": p.id,
                "name": p.name,
                "kind": p.kind,
                "configured": p.configured,
                "models": p.models.iter().map(|m| {
                    json!({
                        "id": m.id,
                        "name": m.name,
                        "kind": m.kind,
                        "available": m.available,
                        "identity": m.identity,
                        "provider_id": m.provider_id,
                    })
                }).collect::<Vec<_>>(),
            })
        })
    }

    /// Get all models across all providers.
    pub async fn list_models(&self) -> Value {
        let mut models = Vec::new();
        for provider in &self.providers {
            for model in &provider.models {
                models.push(json!({
                    "id": model.id,
                    "name": model.name,
                    "provider": provider.id,
                    "kind": model.kind,
                    "available": model.available,
                    "identity": model.identity,
                    "provider_id": model.provider_id,
                }));
            }
        }
        json!({
            "models": models,
        })
    }

    /// Check if provider is configured (has API key).
    pub async fn is_provider_configured(&self, id: &str) -> bool {
        self.providers
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.configured)
            .unwrap_or(false)
    }

    /// Refresh provider configuration status (check env vars).
    pub async fn refresh_configuration(&mut self) {
        for provider in &mut self.providers {
            let key_name = match provider.id.as_str() {
                "openai" => "OPENAI_API_KEY",
                "openrouter" => "OPENROUTER_API_KEY",
                "cheaper-inference" => "CHEAPERINFERENE_API_KEY",
                _ => "",
            };
            provider.configured = !key_name.is_empty() && std::env::var(key_name).is_ok();
        }
        self.messaging_enabled = std::env::var("CLAUDE_CODE_MESSAGING_TOKEN").is_ok();
    }

    /// Add a new provider.
    pub async fn add_provider(&mut self, id: String, name: String, kind: String) -> Value {
        let provider = Provider {
            id: id.clone(),
            name,
            kind,
            models: vec![],
            configured: false,
        };
        self.providers.push(provider);
        json!({
            "status": "added",
            "provider_id": id,
        })
    }

    /// Remove a provider by ID.
    pub async fn remove_provider(&mut self, id: &str) -> Value {
        self.providers.retain(|p| p.id != id);
        json!({
            "status": "removed",
            "provider_id": id,
        })
    }

    /// Add a model to a provider.
    pub async fn add_model(&mut self, provider_id: &str, model: Model) -> Value {
        if let Some(provider) = self.providers.iter_mut().find(|p| p.id == provider_id) {
            provider.models.push(model.clone());
            return json!({
                "status": "added",
                "provider_id": provider_id,
                "model_id": model.id,
            });
        }
        json!({
            "status": "error",
            "message": "provider not found",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_providers() {
        let provider = ModelsProvider::new();
        let result = provider.list_providers().await;
        assert!(result["providers"].is_array());
        assert!(result["messaging"].is_object());
    }

    #[tokio::test]
    async fn test_get_provider() {
        let provider = ModelsProvider::new();
        let openai = provider.get_provider("openai").await;
        assert!(openai.is_some());
        assert_eq!(openai.unwrap()["id"], "openai");
    }
}
