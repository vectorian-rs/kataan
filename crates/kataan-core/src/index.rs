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
    #[serde(default)]
    pub scan: ScanConfig,
    pub type_folders: std::collections::BTreeMap<String, String>,
}

impl VaultConfig {
    /// The type whose `type_folders` mapping points at `folder` (the inverse of
    /// the `type -> folder` map), if any.
    pub fn type_for_folder(&self, folder: &str) -> Option<&str> {
        self.type_folders
            .iter()
            .find(|(_, mapped)| mapped.as_str() == folder)
            .map(|(ty, _)| ty.as_str())
    }
}

/// Directory-scan ignore configuration (kataan.toml `[scan]`). Absent sections
/// deserialize to the defaults, so legacy vaults keep parsing.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScanConfig {
    /// Extra gitignore-style patterns, resolved relative to the vault root and
    /// added to the built-in defaults.
    #[serde(default)]
    pub ignore: Vec<String>,
    /// When false, drop the built-in defaults and honor only `ignore` (and
    /// `.kataanignore`).
    #[serde(default = "default_use_default_ignores")]
    pub use_default_ignores: bool,
}

fn default_use_default_ignores() -> bool {
    true
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            ignore: Vec::new(),
            use_default_ignores: default_use_default_ignores(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FolderIndex {
    pub name: String,
    pub description: Option<String>,
    pub default_type: Option<String>,
    pub folder_checksum: Option<String>,

    #[serde(default)]
    pub documents: Vec<FolderDocument>,
    #[serde(default)]
    pub subfolders: Vec<FolderSubfolder>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FolderDocument {
    pub slug: String,
    pub markdown: String,
    pub toml: String,
    pub markdown_checksum: String,
    pub toml_checksum: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FolderSubfolder {
    pub name: String,
    pub folder_checksum: String,
}
