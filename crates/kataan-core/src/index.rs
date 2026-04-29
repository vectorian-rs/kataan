use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::constants::DEFAULT_MAX_FOLDER_DEPTH;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VaultLimits {
    pub max_folder_depth: Option<usize>,
}

impl Default for VaultLimits {
    fn default() -> Self {
        Self {
            max_folder_depth: Some(DEFAULT_MAX_FOLDER_DEPTH),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VaultConfig {
    pub schema_version: String,
    pub name: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    #[serde(default)]
    pub limits: VaultLimits,
    pub type_folders: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FolderIndex {
    pub name: String,
    pub description: Option<String>,
    pub default_type: Option<String>,
    pub folder_checksum: Option<String>,

    #[serde(default)]
    pub documents: Vec<FolderDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FolderDocument {
    pub slug: String,
    pub markdown: String,
    pub toml: String,
    pub markdown_checksum: String,
    pub toml_checksum: String,
}
