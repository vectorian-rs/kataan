use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DocumentMetadata {
    pub r#type: String,
    pub status: Option<String>,
    pub markdown: String,
    pub markdown_checksum: Option<String>,

    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub edges: BTreeMap<String, Vec<String>>,

    pub created_by: Option<String>,
    pub last_updated_by: Option<String>,
}
