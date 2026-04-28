use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{AgentError, AgentResult};
use crate::provider::{AgentProvider, ProviderRequest, ProviderResponse};
use crate::types::{Api, InputKind, Model, ProviderKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatGptCredentials {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ChatGptProvider {
    credentials: Option<ChatGptCredentials>,
}

impl ChatGptProvider {
    pub fn new(credentials: Option<ChatGptCredentials>) -> Self {
        Self { credentials }
    }

    pub fn default_model(model_id: impl Into<String>) -> Model {
        let id = model_id.into();
        Model {
            name: id.clone(),
            id,
            api: Api::OpenAiCodexResponses,
            provider: ProviderKind::ChatGpt,
            base_url: "https://chatgpt.com/backend-api/codex".to_string(),
            reasoning: true,
            input: vec![InputKind::Text],
            context_window: 192_000,
            max_tokens: 32_000,
        }
    }
}

#[async_trait]
impl AgentProvider for ChatGptProvider {
    async fn complete(&self, request: ProviderRequest) -> AgentResult<ProviderResponse> {
        let Some(_credentials) = &self.credentials else {
            return Err(AgentError::NotConfigured(
                "missing ChatGPT OAuth credentials; run `kataan agent login chatgpt`".to_string(),
            ));
        };

        Err(AgentError::Provider(format!(
            "ChatGPT subscription provider transport is scaffolded but not implemented yet for model {}",
            request.model.id
        )))
    }
}
