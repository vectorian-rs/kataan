use async_trait::async_trait;

use crate::error::AgentResult;
use crate::proposal::AgentProposal;
use crate::types::{Context, Model, StreamOptions};

#[derive(Debug, Clone)]
pub struct ProviderRequest {
    pub model: Model,
    pub context: Context,
    pub options: StreamOptions,
}

#[derive(Debug, Clone)]
pub struct ProviderResponse {
    pub text: String,
    pub proposal: Option<AgentProposal>,
    pub raw_response_id: Option<String>,
}

#[async_trait]
pub trait AgentProvider: Send + Sync {
    async fn complete(&self, request: ProviderRequest) -> AgentResult<ProviderResponse>;
}
