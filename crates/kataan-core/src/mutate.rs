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
    ontology::FieldSchema,
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
    let type_registry = crate::types::TypeRegistry::load(&vault)?;
    if !type_registry.contains(&request.r#type)
        && !vault.index.type_folders.contains_key(&request.r#type)
    {
        return Err(invalid_request(format!(
            "unknown type `{}`",
            request.r#type
        )));
    }
    let base_folder = match &request.parent {
        Some(parent) => {
            // Resolved through the same module the validator uses. Two answers
            // to "may this type live here" would drift, and the way they drift
            // is a document that is created and then fails validation.
            let scopes =
                crate::scope::chain_for(root, &vault.index.type_folders, &type_registry, parent)?;
            if !crate::scope::is_claimed(&scopes, &request.r#type, parent) {
                return Err(invalid_request(format!(
                    "parent `{parent}` is not a home for type `{}`; claims: {}",
                    request.r#type,
                    crate::scope::describe_claims(&scopes, &request.r#type)
                )));
            }
            parent.clone()
        }
        // A type placed only by wildcard patterns has no single folder to
        // default to, so it needs to be told where it goes rather than have a
        // path invented for it.
        None => {
            let scopes = vec![crate::scope::TypeScope::root(
                &vault.index.type_folders,
                &type_registry,
            )];
            crate::scope::default_home(&scopes, &request.r#type).ok_or_else(|| {
                invalid_request(format!(
                    "type `{}` has no folder of its own; pass a parent naming where it goes",
                    request.r#type
                ))
            })?
        }
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

    // Built directly rather than round-tripped through TOML: serializing turns
    // a native datetime in `extra` into a table, which is exactly the value
    // `validate_quoted_dates` needs to see as a datetime to report it.
    enforce_document_schema(
        &vault,
        &crate::document::DocumentMetadata {
            r#type: request.r#type.clone(),
            status: request.status.clone(),
            markdown: format!("{}.md", id.slug()),
            markdown_checksum: None,
            aliases: request.aliases.clone(),
            labels: request.labels.clone(),
            edges: BTreeMap::new(),
            created_by: Some(actor.clone()),
            last_updated_by: Some(actor.clone()),
            occurred_at: request.occurred_at.clone(),
            created_at: Some(now.clone()),
            updated_at: Some(now.clone()),
            extra: request.extra.clone(),
        },
    )?;

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
    let vault = Vault::open(root)?;
    let record = vault.load_document_record(id)?;

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

    // The patched table is what will be on disk, so it is what gets checked —
    // including the keys this call did not touch, since a schema can require a
    // field that an unrelated edit would otherwise leave missing.
    let patched: crate::document::DocumentMetadata = sidecar
        .clone()
        .try_into()
        .map_err(|error| invalid_request(format!("patched sidecar is not valid: {error}")))?;
    enforce_document_schema(&vault, &patched)?;

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
    let source_record = validated_edge(root, source, predicate, std::slice::from_ref(target))?;

    let target_id = target.as_str().to_owned();
    edit_edges(root, &source_record, |edges| {
        let targets = targets_of(edges, predicate, &source_record.id)?;
        if !targets
            .iter()
            .any(|value| value.as_str() == Some(&target_id))
        {
            targets.push(toml::Value::String(target_id.clone()));
        }
        Ok(())
    })
}

/// Remove the forward edge `source --predicate--> target`.
///
/// Deliberately unvalidated, unlike [`add_edge`]. An edge worth removing is
/// often one that should never have been written: the ontology has since
/// narrowed and now forbids it, or the target document is gone. Requiring it to
/// be legal before it could be deleted would make exactly the states that need
/// repairing the ones that cannot be repaired. Only the source has to exist,
/// because that is the file being edited.
///
/// Idempotent: removing an edge that is not there succeeds and changes nothing,
/// so `updated_at` does not move.
pub fn remove_edge(
    root: impl AsRef<Path>,
    source: &CanonicalId,
    predicate: &str,
    target: &CanonicalId,
) -> Result<()> {
    let root = root.as_ref();
    let source_record = Vault::open(root)?.load_document_record(source)?;
    let target_id = target.as_str().to_owned();
    edit_edges(root, &source_record, |edges| {
        if let Some(targets) = existing_targets(edges, predicate, &source_record.id)? {
            targets.retain(|value| value.as_str() != Some(&target_id));
        }
        Ok(())
    })
}

/// Set the complete list of targets for one predicate on one source, replacing
/// whatever was there.
///
/// This is how a wrong edge is corrected in a single write: `add_edge` cannot
/// unsay anything, and remove-then-add leaves the document briefly in a state
/// neither the caller nor `validate` asked for.
///
/// Every incoming target is validated as [`add_edge`] would validate it — these
/// are edges being written. What is being replaced is not, for the reason
/// [`remove_edge`] gives. An empty list removes the predicate entirely.
pub fn replace_edges_for_predicate(
    root: impl AsRef<Path>,
    source: &CanonicalId,
    predicate: &str,
    targets: &[CanonicalId],
) -> Result<()> {
    let root = root.as_ref();
    let source_record = validated_edge(root, source, predicate, targets)?;

    // Written in the order given, deduplicated: an edge is identified by
    // (source, predicate, target), so a repeat in the request is one edge.
    let mut wanted: Vec<toml::Value> = Vec::new();
    for target in targets {
        let id = target.as_str();
        if !wanted.iter().any(|value| value.as_str() == Some(id)) {
            wanted.push(toml::Value::String(id.to_owned()));
        }
    }

    edit_edges(root, &source_record, |edges| {
        if wanted.is_empty() {
            edges.remove(predicate);
        } else {
            edges.insert(predicate.to_owned(), toml::Value::Array(wanted));
        }
        Ok(())
    })
}

/// The target array for `predicate`, creating it if absent.
fn targets_of<'a>(
    edges: &'a mut toml::Table,
    predicate: &str,
    source: &CanonicalId,
) -> Result<&'a mut Vec<toml::Value>> {
    edges
        .entry(predicate.to_owned())
        .or_insert_with(|| toml::Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| {
            invalid_request(format!(
                "`{source}` has a non-array `edges.{predicate}` value"
            ))
        })
}

/// The target array for `predicate`, or `None` when the predicate is absent —
/// so a removal does not create the key it is about to empty.
fn existing_targets<'a>(
    edges: &'a mut toml::Table,
    predicate: &str,
    source: &CanonicalId,
) -> Result<Option<&'a mut Vec<toml::Value>>> {
    match edges.get_mut(predicate) {
        None => Ok(None),
        Some(toml::Value::Array(targets)) => Ok(Some(targets)),
        Some(_) => Err(invalid_request(format!(
            "`{source}` has a non-array `edges.{predicate}` value"
        ))),
    }
}

/// Apply `edit` to the source document's `edges` table and write the sidecar
/// back — but only if something actually changed.
///
/// As in `update_document`, the on-disk table is edited in place so sibling
/// keys survive, and a call that changes nothing leaves `updated_at` alone
/// rather than dirtying the file for git to notice.
fn edit_edges(
    root: &Path,
    source_record: &crate::vault::DocumentRecord,
    edit: impl FnOnce(&mut toml::Table) -> Result<()>,
) -> Result<()> {
    let mut sidecar = read_sidecar_table(&source_record.toml_path)?;
    let before = sidecar.clone();

    let mut edges = match sidecar.get("edges") {
        Some(toml::Value::Table(table)) => table.clone(),
        Some(_) => {
            return Err(invalid_request(format!(
                "`{}` has a non-table `edges` value",
                source_record.id
            )))
        }
        None => toml::Table::new(),
    };
    edit(&mut edges)?;

    // A predicate with no targets left says nothing; keeping the empty array
    // would make a removal look different on disk from never having added it.
    edges.retain(|_, targets| targets.as_array().is_none_or(|list| !list.is_empty()));
    if edges.is_empty() {
        sidecar.remove("edges");
    } else {
        sidecar.insert("edges".to_owned(), toml::Value::Table(edges));
    }

    if sidecar == before {
        return Ok(());
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
/// Check that `predicate` exists and that the source and every target are types
/// the ontology permits at those ends. Returns the source's record, which every
/// caller needs next.
///
/// Shared so that `add_edge` and `replace_edges_for_predicate` cannot drift on
/// what a legal edge is: a rule added to one would otherwise silently not apply
/// to the other, and both write the same edges.
fn validated_edge(
    root: &Path,
    source: &CanonicalId,
    predicate: &str,
    targets: &[CanonicalId],
) -> Result<crate::vault::DocumentRecord> {
    let vault = Vault::open(root)?;
    // Validating an edge needs only the ontology and the endpoints — not a full
    // vault walk + graph build. Subtypes satisfy a rule written for their
    // supertype, so it needs the registry just as the full walk does.
    let ontology = Ontology::load(root)?;
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
    for target in targets {
        let target_type = vault.load_document_record(target)?.metadata.r#type;
        if !ontology::type_allowed(&edge.to, &target_type, &type_registry) {
            return Err(invalid_request(format!(
                "type `{target_type}` cannot be the target of `{predicate}`"
            )));
        }
    }
    Ok(source_record)
}

/// Refuse a write that `kataan validate` would reject on its next run.
///
/// This module's stated invariant is that a malformed value never reaches disk,
/// so `validate` never has to report one kataan itself wrote. Two hand-written
/// guards enforced that for `status` and for timestamp *syntax*. Everything a
/// vault declares in `[nodes.*]` — required fields, field types, interval
/// bounds, reference targets, `instant` precision on `occurred_at` — was
/// checked only by `validate`, after the value was already stored. The general
/// validator lived one module away.
///
/// A vault with no ontology, or one whose ontology does not parse, writes
/// exactly as it did before: `validate` is the tool that reports a broken
/// ontology, and refusing every write because of one would be worse than the
/// problem.
fn enforce_document_schema(
    vault: &Vault,
    metadata: &crate::document::DocumentMetadata,
) -> Result<()> {
    let Ok(ontology) = Ontology::load(&vault.root) else {
        return Ok(());
    };
    let registry = crate::types::TypeRegistry::load(vault)?;

    // Only a `reference` field needs to know what else exists, and resolving
    // that means walking the vault. Most types declare none, so the walk is
    // paid for only when a schema can actually use it.
    let known_document_types = if declares_a_reference(&ontology, &metadata.r#type) {
        vault
            .load_documents()?
            .into_iter()
            .map(|document| (document.id.as_str().to_owned(), document.metadata.r#type))
            .collect()
    } else {
        BTreeMap::new()
    };

    let mut diagnostics = crate::ontology::validate_quoted_dates(metadata);
    diagnostics.extend(crate::ontology::validate_node_fields(
        &ontology,
        metadata,
        &known_document_types,
        &registry,
    ));

    // Warnings describe the schema, not the write — an array declared without
    // `items` is the vault author's problem, and blocking an unrelated document
    // on it would be wrong.
    match diagnostics
        .into_iter()
        .find(|diagnostic| diagnostic.severity == crate::diagnostic::Severity::Error)
    {
        Some(error) => Err(invalid_request(format!(
            "{} [{}]",
            error.message, error.code
        ))),
        None => Ok(()),
    }
}

/// Whether `type_name`'s schema has any field, or array element, that is a
/// reference — the only thing that makes the document index necessary.
fn declares_a_reference(ontology: &Ontology, type_name: &str) -> bool {
    ontology.nodes.get(type_name).is_some_and(|schema| {
        schema
            .fields
            .values()
            .any(FieldSchema::declares_a_reference)
    })
}

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
