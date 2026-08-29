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

    /// Top-level sidecar keys kataan does not define, captured so a
    /// read-modify-write cycle preserves them and so consumers can read
    /// vault-specific fields (`linkedin`, `website`, ...) that would otherwise
    /// be invisible. Serialized flat, as the author wrote them.
    ///
    /// `toml::Value` has no `JsonSchema` impl, so the schema describes these as
    /// free-form JSON — accurate enough, since the point is that kataan does
    /// not constrain their shape.
    #[serde(flatten, default)]
    #[schemars(with = "BTreeMap<String, serde_json::Value>")]
    pub extra: BTreeMap<String, toml::Value>,
}

/// The human display name derived from a document's metadata: the first alias,
/// else the first label. Callers layer their own fallbacks (e.g. a title from
/// the id, or a markdown heading) on top of this shared precedence.
pub fn display_name(metadata: &DocumentMetadata) -> Option<String> {
    metadata
        .aliases
        .first()
        .cloned()
        .or_else(|| metadata.labels.first().cloned())
}
