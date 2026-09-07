//! The arguments each tool accepts, as types.
//!
//! Every tool deserializes its whole argument object into one of these rather
//! than picking fields out of a `Value`. The difference is not stylistic: a
//! picker answers "what is at this key, if it is the type I expected" and has
//! to invent something when the answer is no. `str_vec` returned an empty list
//! for anything that was not an array, so `create_document` with
//! `"aliases": "a,b"` created the document and silently dropped the aliases,
//! reporting success. Deserializing says "this is not a list of strings" and
//! refuses the call.
//!
//! These types are also what the tool schemas are generated from, so the
//! catalogue an agent reads and the parser it is checked against cannot
//! disagree.

use schemars::JsonSchema;
use serde::Deserialize;

use kataan_core::{
    id::CanonicalId,
    mutate::{DocumentEdit, NewDocument},
    query::Direction,
};

/// A tool that takes one canonical id.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct IdArgs {
    /// Canonical id, e.g. `organizations/bull`.
    pub id: String,
}

/// A tool that takes one filesystem path.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct PathArgs {
    /// Vault-relative or absolute path, e.g. `notes/my-note.md`.
    pub path: String,
}

/// `schema`, which describes one kind or one vault document type.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SchemaArgs {
    /// `document`, `folder-index`, `vault`, `type-definition`, or the name of a
    /// type declared by the vault.
    pub kind: String,
}

/// `neighbors`: what one document is connected to.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct NeighborsArgs {
    /// Canonical id, e.g. `organizations/bull`.
    pub id: String,
    /// Restrict to one predicate; omit for all.
    #[serde(default)]
    pub predicate: Option<String>,
    /// `out` = edges this document declares, `in` = edges pointing at it,
    /// `both` (default).
    #[serde(default)]
    pub direction: Direction,
}

/// `subgraph`: nodes and links for the whole vault.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SubgraphArgs {
    /// Restrict to these document types; omit for all.
    #[serde(default)]
    pub types: Vec<String>,
    /// Restrict to these edge predicates; omit for all.
    #[serde(default)]
    pub predicates: Vec<String>,
    /// Refuse rather than return more than this many nodes. Defaults to 200
    /// here; the maximum is 5000.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// `create_document`. The document itself is `kataan_core`'s own
/// [`NewDocument`], so this surface cannot accept a document the HTTP API would
/// not.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct CreateArgs {
    #[serde(flatten)]
    pub document: NewDocument,
}

/// `update_document`: which document, and the edit to apply to it.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct UpdateArgs {
    /// Canonical id of the document to update.
    pub id: String,
    #[serde(flatten)]
    pub edit: DocumentEdit,
}

/// `add_edge` and `remove_edge`: one edge, named by its three parts.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct EdgeArgs {
    /// Canonical id of the document that declares the edge.
    pub source: String,
    /// Predicate name, which must exist in `ontology.toml`.
    pub predicate: String,
    /// Canonical id of the document the edge points at.
    pub target: String,
}

/// `replace_edges_for_predicate`: the whole target list for one predicate.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ReplaceEdgesArgs {
    /// Canonical id of the document that declares the edges.
    pub source: String,
    /// Predicate name, which must exist in `ontology.toml`.
    pub predicate: String,
    /// The complete new target list. An empty list removes every edge for this
    /// predicate.
    pub targets: Vec<String>,
}

/// Parse a canonical id, naming the argument it came from.
pub(super) fn canonical(field: &str, value: &str) -> anyhow::Result<CanonicalId> {
    CanonicalId::parse(value).map_err(|error| anyhow::anyhow!("invalid `{field}`: {error}"))
}
