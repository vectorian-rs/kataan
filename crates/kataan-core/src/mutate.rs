//! Validated document mutations: create/update documents and add edges so the
//! result is always a well-formed vault (correct id, sidecar, checksum, folder
//! indexes, and ontology-legal edges). Each operation writes files atomically
//! and finishes with [`rebuild::rebuild_indexes`], which backfills the markdown
//! checksum and every folder index. This is the write surface agents use (via
//! the MCP server); direct file edits remain valid too.

use std::{collections::BTreeMap, path::Path};

use serde::Serialize;

use crate::{
    constants::{ACTOR_AGENT, STATUS_VALUES},
    id::CanonicalId,
    ontology::{self, Ontology},
    rebuild,
    title::slugify,
    vault::Vault,
    write::atomic_write_string,
    Error, Result,
};

/// A document to create. `parent` places it under a specific folder id; when
/// absent the document goes in the type's configured folder.
#[derive(Debug, Clone, Default)]
pub struct NewDocument {
    pub r#type: String,
    pub title: String,
    pub body: String,
    pub parent: Option<String>,
    pub aliases: Vec<String>,
    pub labels: Vec<String>,
    pub status: Option<String>,
    pub actor: Option<String>,
    /// When the thing this document describes happened. Validated on write, so
    /// a malformed value is rejected rather than stored.
    pub occurred_at: Option<String>,
    /// Extra top-level sidecar keys to write alongside the ones kataan defines.
    /// Rejected if they collide with a reserved key (see [`RESERVED_KEYS`]).
    pub extra: BTreeMap<String, toml::Value>,
}

/// Sidecar keys kataan models itself. `NewDocument::extra` may not contain
/// these: serializing a flattened duplicate would emit the key twice.
const RESERVED_KEYS: &[&str] = &[
    "type",
    "status",
    "markdown",
    "markdown_checksum",
    "aliases",
    "labels",
    "created_by",
    "last_updated_by",
    "occurred_at",
    "created_at",
    "updated_at",
    "edges",
];

/// Fields to change on an existing document. `None` leaves a field unchanged.
#[derive(Debug, Clone, Default)]
pub struct DocumentPatch {
    pub status: Option<String>,
    pub occurred_at: Option<String>,
    pub aliases: Option<Vec<String>>,
    pub labels: Option<Vec<String>>,
    pub actor: Option<String>,
}

/// Create a new document. Returns its canonical id.
pub fn create_document(root: impl AsRef<Path>, request: NewDocument) -> Result<CanonicalId> {
    let root = root.as_ref();
    let vault = Vault::open(root)?;

    // The type must be registered whether or not a parent was given: placing a
    // document by hand is not a reason to skip the check that makes the result
    // a well-formed vault.
    let type_folder = vault
        .index
        .type_folders
        .get(&request.r#type)
        .cloned()
        .ok_or_else(|| invalid_request(format!("unknown type `{}`", request.r#type)))?;
    let base_folder = match &request.parent {
        Some(parent) => {
            if parent != &type_folder && !parent.starts_with(&format!("{type_folder}/")) {
                return Err(invalid_request(format!(
                    "parent `{parent}` is outside `{type_folder}`, the folder for type `{}`",
                    request.r#type
                )));
            }
            parent.clone()
        }
        None => type_folder,
    };
    validate_status(request.status.as_deref())?;
    validate_timestamp(request.occurred_at.as_deref())?;
    if let Some(reserved) = request
        .extra
        .keys()
        .find(|key| RESERVED_KEYS.contains(&key.as_str()))
    {
        return Err(invalid_request(format!(
            "`{reserved}` is a reserved sidecar key and cannot be set as an extra field"
        )));
    }

    let slug = slugify(&request.title)
        .ok_or_else(|| invalid_request(format!("title `{}` produces no slug", request.title)))?;
    let id = CanonicalId::parse(format!("{base_folder}/{slug}"))
        .map_err(|error| invalid_request(format!("invalid document id: {error}")))?;

    let markdown_path = root.join(id.markdown_path());
    let toml_path = root.join(id.toml_path());
    if toml_path.exists() || markdown_path.exists() {
        return Err(invalid_request(format!("document `{id}` already exists")));
    }
    if let Some(parent) = markdown_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let actor = request.actor.unwrap_or_else(|| ACTOR_AGENT.to_owned());
    let now = crate::time::iso8601_utc_now();
    let sidecar = Sidecar {
        r#type: request.r#type,
        status: request.status,
        markdown: format!("{}.md", id.slug()),
        aliases: request.aliases,
        labels: request.labels,
        created_by: Some(actor.clone()),
        last_updated_by: Some(actor),
        // Transaction time: when this record was written. `occurred_at` is
        // valid time and stays the author's to set.
        occurred_at: request.occurred_at,
        created_at: Some(now.clone()),
        updated_at: Some(now),
        extra: request.extra,
        edges: BTreeMap::new(),
    };

    atomic_write_string(&markdown_path, &request.body)?;
    atomic_write_string(&toml_path, &sidecar.to_toml())?;
    rebuild::rebuild_indexes(root)?;
    Ok(id)
}

/// Update a document's body and/or metadata. `body: None` leaves the markdown
/// unchanged; `patch` fields default to no change.
pub fn update_document(
    root: impl AsRef<Path>,
    id: &CanonicalId,
    body: Option<String>,
    patch: DocumentPatch,
) -> Result<()> {
    let root = root.as_ref();
    let record = Vault::open(root)?.load_document_record(id)?;

    // Edit the on-disk sidecar in place rather than re-rendering it from
    // `DocumentMetadata`: keys kataan does not define survive untouched, in
    // their original positions, instead of being dropped by the projection.
    let mut sidecar = read_sidecar_table(&record.toml_path)?;
    let before = sidecar.clone();

    if let Some(status) = patch.status {
        validate_status(Some(&status))?;
        sidecar.insert("status".to_owned(), toml::Value::String(status));
    }
    if let Some(aliases) = patch.aliases {
        sidecar.insert("aliases".to_owned(), string_array(aliases));
    }
    if let Some(labels) = patch.labels {
        sidecar.insert("labels".to_owned(), string_array(labels));
    }
    if let Some(occurred_at) = patch.occurred_at {
        validate_timestamp(Some(&occurred_at))?;
        sidecar.insert("occurred_at".to_owned(), toml::Value::String(occurred_at));
    }
    sidecar.insert(
        "last_updated_by".to_owned(),
        toml::Value::String(patch.actor.unwrap_or_else(|| ACTOR_AGENT.to_owned())),
    );

    // Only rewrite the body when it actually differs, so a caller that resends
    // the text it already has does not dirty the file.
    let body_changed = match &body {
        Some(body) => {
            std::fs::read_to_string(&record.markdown_path)
                .ok()
                .as_deref()
                != Some(body.as_str())
        }
        None => false,
    };
    if body_changed {
        atomic_write_string(&record.markdown_path, body.as_deref().unwrap_or_default())?;
    }

    // `updated_at` records when the record changed, so a call that changes
    // nothing must not move it. Otherwise a no-op update would dirty the file
    // on every invocation — the same git churn the sidecar rewrite avoids.
    if !body_changed && sidecar == before {
        return Ok(());
    }
    sidecar.insert(
        "updated_at".to_owned(),
        toml::Value::String(crate::time::iso8601_utc_now()),
    );

    write_sidecar_table(&record.toml_path, &sidecar)?;
    rebuild::rebuild_indexes(root)?;
    Ok(())
}

/// Add a forward edge `source --predicate--> target`, validated against the
/// ontology (predicate exists, and the source/target types are permitted).
/// Inverse/symmetric edges are derived at graph-build time, so only the forward
/// edge is written.
pub fn add_edge(
    root: impl AsRef<Path>,
    source: &CanonicalId,
    predicate: &str,
    target: &CanonicalId,
) -> Result<()> {
    let root = root.as_ref();
    let vault = Vault::open(root)?;

    // Validating one edge needs only the ontology and the two endpoints — not a
    // full vault walk + graph build.
    let ontology = Ontology::load(root)?;
    // Subtypes satisfy an edge rule written for their supertype, so the single
    // edge check needs the registry just as the full walk does.
    let type_registry = crate::types::TypeRegistry::load(&vault)?;
    let edge = ontology
        .edges
        .get(predicate)
        .ok_or_else(|| invalid_request(format!("unknown predicate `{predicate}`")))?;
    let source_record = vault.load_document_record(source)?;
    if !ontology::type_allowed(&edge.from, &source_record.metadata.r#type, &type_registry) {
        return Err(invalid_request(format!(
            "type `{}` cannot be the source of `{predicate}`",
            source_record.metadata.r#type
        )));
    }
    let target_type = vault.load_document_record(target)?.metadata.r#type;
    if !ontology::type_allowed(&edge.to, &target_type, &type_registry) {
        return Err(invalid_request(format!(
            "type `{target_type}` cannot be the target of `{predicate}`"
        )));
    }

    // As in `update_document`, edit the sidecar in place so sibling keys on the
    // source document are preserved.
    let mut sidecar = read_sidecar_table(&source_record.toml_path)?;
    let edges = sidecar
        .entry("edges".to_owned())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| invalid_request(format!("`{source}` has a non-table `edges` value")))?;
    let targets = edges
        .entry(predicate.to_owned())
        .or_insert_with(|| toml::Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| {
            invalid_request(format!(
                "`{source}` has a non-array `edges.{predicate}` value"
            ))
        })?;
    let target_id = target.as_str();
    if !targets
        .iter()
        .any(|value| value.as_str() == Some(target_id))
    {
        targets.push(toml::Value::String(target_id.to_owned()));
    }

    sidecar.insert(
        "last_updated_by".to_owned(),
        toml::Value::String(ACTOR_AGENT.to_owned()),
    );
    sidecar.insert(
        "updated_at".to_owned(),
        toml::Value::String(crate::time::iso8601_utc_now()),
    );

    write_sidecar_table(&source_record.toml_path, &sidecar)?;
    rebuild::rebuild_indexes(root)?;
    Ok(())
}

/// Reject a malformed timestamp at the write boundary, so `validate` never has
/// to report one kataan itself wrote.
fn validate_timestamp(value: Option<&str>) -> Result<()> {
    match value {
        Some(value) => crate::time::Timestamp::parse(value)
            .map(|_| ())
            .map_err(|error| invalid_request(error.to_string())),
        None => Ok(()),
    }
}

fn validate_status(status: Option<&str>) -> Result<()> {
    match status {
        Some(status) if !STATUS_VALUES.contains(&status) => {
            Err(invalid_request(format!("invalid status `{status}`")))
        }
        _ => Ok(()),
    }
}

fn invalid_request(message: String) -> Error {
    Error::InvalidRequest(message)
}

/// The sidecar for a brand-new document. Only [`create_document`] renders one;
/// the update paths edit the on-disk table in place so they cannot drop keys.
/// The markdown checksum is intentionally omitted — `rebuild_indexes` backfills
/// it.
#[derive(Serialize)]
struct Sidecar {
    r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    markdown: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    aliases: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    labels: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_updated_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    occurred_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_at: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, toml::Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    edges: BTreeMap<String, Vec<String>>,
}

impl Sidecar {
    fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("document sidecar is serializable TOML")
    }
}

/// Parse a sidecar into its raw TOML table. Nothing is projected through a
/// typed struct, so keys kataan does not model survive the round trip.
fn read_sidecar_table(path: &Path) -> Result<toml::Table> {
    let text = std::fs::read_to_string(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    text.parse::<toml::Table>()
        .map_err(|source| Error::TomlParse {
            path: path.to_path_buf(),
            source,
        })
}

/// Write an edited sidecar table back atomically. `toml` emits tables and
/// arrays-of-tables after scalars whatever the key order, so an edited table
/// always renders as valid TOML; the `preserve_order` feature keeps the
/// author's original key order so updates stay diff-sized.
fn write_sidecar_table(path: &Path, table: &toml::Table) -> Result<()> {
    let rendered = toml::to_string_pretty(table).expect("document sidecar is serializable TOML");
    atomic_write_string(path, &rendered)
}

fn string_array(values: Vec<String>) -> toml::Value {
    toml::Value::Array(values.into_iter().map(toml::Value::String).collect())
}

#[cfg(test)]
mod tests;
