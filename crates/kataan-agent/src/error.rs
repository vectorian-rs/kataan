use thiserror::Error;

pub type AgentResult<T> = Result<T, AgentError>;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("agent provider is not configured: {0}")]
    NotConfigured(String),

    #[error("agent provider request failed: {0}")]
    Provider(String),

    #[error("agent authentication failed: {0}")]
    Auth(String),

    #[error("agent response could not be parsed: {0}")]
    Parse(String),
}
