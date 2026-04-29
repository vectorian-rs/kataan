use crate::prompt::KATAAN_AGENT_SYSTEM_PROMPT;
use crate::types::{Context, Message, UserContent, UserMessage};

#[derive(Debug, Clone, Default)]
pub struct AgentContextSelection {
    pub vault_summary: Option<String>,
    pub selected_document_id: Option<String>,
    pub selected_document_metadata: Option<String>,
    pub selected_document_markdown: Option<String>,
    pub graph_summary: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ContextBuilder;

impl ContextBuilder {
    pub fn build_user_context(
        prompt: impl Into<String>,
        selection: AgentContextSelection,
    ) -> Context {
        let mut content = String::new();

        push_section(&mut content, "User request", &prompt.into());

        if let Some(vault_summary) = selection.vault_summary {
            push_section(&mut content, "Vault summary", &vault_summary);
        }

        if let Some(selected_document_id) = selection.selected_document_id {
            push_section(&mut content, "Selected document", &selected_document_id);
        }

        if let Some(metadata) = selection.selected_document_metadata {
            push_section(&mut content, "Selected document metadata", &metadata);
        }

        if let Some(graph_summary) = selection.graph_summary {
            push_section(&mut content, "Relevant graph", &graph_summary);
        }

        if let Some(markdown) = selection.selected_document_markdown {
            push_section(&mut content, "Selected document markdown", &markdown);
        }

        Context {
            system_prompt: Some(KATAAN_AGENT_SYSTEM_PROMPT.to_string()),
            messages: vec![Message::User(UserMessage {
                content: UserContent::Text(content),
                timestamp: now_millis(),
            })],
            tools: Vec::new(),
        }
    }
}

fn push_section(target: &mut String, title: &str, body: &str) {
    if !target.is_empty() {
        target.push_str("\n\n");
    }
    target.push_str("## ");
    target.push_str(title);
    target.push('\n');
    target.push_str(body.trim());
    target.push('\n');
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
