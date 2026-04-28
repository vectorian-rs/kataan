use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultIndex {
    pub schema_version: String,
    pub name: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub type_folders: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderIndex {
    pub name: String,
    pub description: Option<String>,
    pub default_type: Option<String>,
    pub folder_checksum: Option<String>,

    #[serde(default)]
    pub documents: Vec<FolderDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderDocument {
    pub slug: String,
    pub markdown: String,
    pub toml: String,
    pub markdown_checksum: String,
    pub toml_checksum: String,
}
