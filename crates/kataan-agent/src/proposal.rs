use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProposal {
    pub rationale: String,
    pub confidence: f32,
    #[serde(default)]
    pub context_used: Vec<String>,
    #[serde(default)]
    pub actions: Vec<AgentAction>,
    #[serde(default)]
    pub expected_changed_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AgentAction {
    CreateDocument {
        id: String,
        markdown: String,
        metadata: Value,
    },
    UpdateDocument {
        id: String,
        markdown: Option<String>,
        metadata_patch: Value,
    },
    Link {
        from: String,
        to: String,
        relationship: String,
    },
    Archive {
        id: String,
        reason: String,
    },
}
