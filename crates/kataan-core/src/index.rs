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

/// Whether a `type_folders` value is a plain relative path inside the vault.
///
/// Every walker and every rebuild joins these values to the vault root, so an
/// absolute path (which `Path::join` substitutes wholesale) or a `..` segment
/// would let a cloned vault direct reads *and writes* outside its own tree.
/// Vaults are shared as git repositories, so the value is untrusted input.
pub fn is_safe_type_folder(folder: &str) -> bool {
    !folder.is_empty()
        && !std::path::Path::new(folder).is_absolute()
        && std::path::Path::new(folder)
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

/// A schema version as `(major, minor)`.
///
/// Patch is ignored: a patch release does not change the on-disk shape, so
/// refusing to open a 0.2.1 vault from a 0.2.0 build would be noise.
pub fn parse_schema_version(value: &str) -> Option<(u32, u32)> {
    let mut parts = value.split('.');
    let major = parts.next()?.trim().parse().ok()?;
    let minor = match parts.next() {
        Some(minor) => minor.trim().parse().ok()?,
        None => 0,
    };
    Some((major, minor))
}

impl VaultConfig {
    /// Whether `supported` can read a vault declaring `self.schema_version`.
    ///
    /// An older vault read by a newer build is fine and stays fine: that is the
    /// back-compat direction, and every field added since is optional. The
    /// reverse is not, because the newer vault may use shapes this build cannot
    /// deserialize at all.
    ///
    /// An unparseable version counts as unsupported. kataan writes this field
    /// itself, so a value it cannot read means the file was damaged by hand,
    /// and guessing at that is worse than saying so.
    pub fn schema_is_supported_by(&self, supported: &str) -> bool {
        match (
            parse_schema_version(&self.schema_version),
            parse_schema_version(supported),
        ) {
            (Some(found), Some(supported)) => found <= supported,
            _ => false,
        }
    }

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

    /// Types this folder declares for its own subtree, as patterns relative to
    /// this folder. Additive: a declaration here widens what is legal below it
    /// and is invisible outside it.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub type_folders: std::collections::BTreeMap<String, String>,

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
