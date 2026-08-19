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
    document::DocumentMetadata,
    id::CanonicalId,
    ontology, rebuild,
    title::slugify,
    vault::{LoadedVault, Vault},
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
}

/// Fields to change on an existing document. `None` leaves a field unchanged.
#[derive(Debug, Clone, Default)]
pub struct DocumentPatch {
    pub status: Option<String>,
    pub aliases: Option<Vec<String>>,
    pub labels: Option<Vec<String>>,
    pub actor: Option<String>,
}

/// Create a new document. Returns its canonical id.
pub fn create_document(root: impl AsRef<Path>, request: NewDocument) -> Result<CanonicalId> {
    let root = root.as_ref();
    let vault = Vault::open(root)?;

    let base_folder = match &request.parent {
        Some(parent) => parent.clone(),
        None => vault
            .index
            .type_folders
            .get(&request.r#type)
            .cloned()
            .ok_or_else(|| unknown(format!("unknown type `{}`", request.r#type)))?,
    };
    validate_status(request.status.as_deref())?;

    let slug = slugify(&request.title)
        .ok_or_else(|| unknown(format!("title `{}` produces no slug", request.title)))?;
    let id = CanonicalId::parse(format!("{base_folder}/{slug}"))
        .map_err(|error| unknown(format!("invalid document id: {error}")))?;

    let markdown_path = root.join(id.markdown_path());
    let toml_path = root.join(id.toml_path());
    if toml_path.exists() || markdown_path.exists() {
        return Err(unknown(format!("document `{id}` already exists")));
    }
    if let Some(parent) = markdown_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let actor = request.actor.unwrap_or_else(|| ACTOR_AGENT.to_owned());
    let sidecar = Sidecar {
        r#type: request.r#type,
        status: request.status,
        markdown: format!("{}.md", id.slug()),
        aliases: request.aliases,
        labels: request.labels,
        created_by: Some(actor.clone()),
        last_updated_by: Some(actor),
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
    let mut metadata = record.metadata;

    if let Some(status) = patch.status {
        validate_status(Some(&status))?;
        metadata.status = Some(status);
    }
    if let Some(aliases) = patch.aliases {
        metadata.aliases = aliases;
    }
    if let Some(labels) = patch.labels {
        metadata.labels = labels;
    }
    metadata.last_updated_by = Some(patch.actor.unwrap_or_else(|| ACTOR_AGENT.to_owned()));

    if let Some(body) = body {
        atomic_write_string(&record.markdown_path, &body)?;
    }
    atomic_write_string(&record.toml_path, &Sidecar::from(metadata).to_toml())?;
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
    let loaded = LoadedVault::load(root)?;

    let edge = loaded
        .ontology
        .edges
        .get(predicate)
        .ok_or_else(|| unknown(format!("unknown predicate `{predicate}`")))?;
    let source_record = loaded
        .documents
        .get(source)
        .ok_or_else(|| unknown(format!("source document `{source}` does not exist")))?;
    if !ontology::type_allowed(&edge.from, &source_record.metadata.r#type) {
        return Err(unknown(format!(
            "type `{}` cannot be the source of `{predicate}`",
            source_record.metadata.r#type
        )));
    }
    let target_record = loaded
        .documents
        .get(target)
        .ok_or_else(|| unknown(format!("target document `{target}` does not exist")))?;
    if !ontology::type_allowed(&edge.to, &target_record.metadata.r#type) {
        return Err(unknown(format!(
            "type `{}` cannot be the target of `{predicate}`",
            target_record.metadata.r#type
        )));
    }

    let mut metadata = source_record.metadata.clone();
    let targets = metadata.edges.entry(predicate.to_owned()).or_default();
    let target_id = target.as_str().to_owned();
    if !targets.contains(&target_id) {
        targets.push(target_id);
    }
    atomic_write_string(
        &record_toml(&loaded, source)?,
        &Sidecar::from(metadata).to_toml(),
    )?;
    rebuild::rebuild_indexes(root)?;
    Ok(())
}

fn record_toml(loaded: &LoadedVault, id: &CanonicalId) -> Result<std::path::PathBuf> {
    Ok(loaded
        .documents
        .get(id)
        .expect("caller verified the document exists")
        .toml_path
        .clone())
}

fn validate_status(status: Option<&str>) -> Result<()> {
    match status {
        Some(status) if !STATUS_VALUES.contains(&status) => {
            Err(unknown(format!("invalid status `{status}`")))
        }
        _ => Ok(()),
    }
}

fn unknown(message: String) -> Error {
    Error::InvalidVaultStructure(message)
}

/// A document sidecar rendered with scalars/arrays first and the `[edges]` table
/// last, so it serializes as valid TOML (unlike `DocumentMetadata`, whose field
/// order puts scalars after the table). The markdown checksum is intentionally
/// omitted — `rebuild_indexes` backfills it.
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
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    edges: BTreeMap<String, Vec<String>>,
}

impl From<DocumentMetadata> for Sidecar {
    fn from(metadata: DocumentMetadata) -> Self {
        Self {
            r#type: metadata.r#type,
            status: metadata.status,
            markdown: metadata.markdown,
            aliases: metadata.aliases,
            labels: metadata.labels,
            created_by: metadata.created_by,
            last_updated_by: metadata.last_updated_by,
            edges: metadata.edges,
        }
    }
}

impl Sidecar {
    fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("document sidecar is serializable TOML")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_vault(name: &str) -> std::path::PathBuf {
        let root = crate::test_support::unique_temp_dir(name);
        crate::init::init_vault(&root, "Test").unwrap();
        root
    }

    fn note(title: &str, body: &str) -> NewDocument {
        NewDocument {
            r#type: "note".to_owned(),
            title: title.to_owned(),
            body: body.to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn create_document_produces_a_valid_document() {
        let root = temp_vault("create");

        let id = create_document(
            &root,
            NewDocument {
                status: Some("active".to_owned()),
                ..note("My First Note!", "# My First Note\n\nhello\n")
            },
        )
        .unwrap();

        assert_eq!(id.as_str(), "notes/my-first-note");
        assert!(root.join("notes/my-first-note.md").is_file());
        assert!(root.join("notes/my-first-note.toml").is_file());
        assert!(crate::validate::validate(&root).unwrap().is_ok());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn create_document_rejects_collision_and_unknown_type() {
        let root = temp_vault("collision");

        create_document(&root, note("Dup", "x")).unwrap();
        assert!(create_document(&root, note("Dup", "x")).is_err());
        assert!(create_document(
            &root,
            NewDocument {
                r#type: "nonsense".to_owned(),
                ..note("Y", "y")
            }
        )
        .is_err());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn update_document_changes_body_and_stays_valid() {
        let root = temp_vault("update");
        let id = create_document(&root, note("Note", "old body")).unwrap();

        update_document(
            &root,
            &id,
            Some("new body".to_owned()),
            DocumentPatch {
                status: Some("archived".to_owned()),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(root.join("notes/note.md")).unwrap(),
            "new body"
        );
        assert!(crate::validate::validate(&root).unwrap().is_ok());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn add_edge_validates_against_the_ontology() {
        let root = temp_vault("edges");
        let source = create_document(&root, note("A", "a")).unwrap();
        let target = create_document(
            &root,
            NewDocument {
                r#type: "topic".to_owned(),
                ..note("B", "b")
            },
        )
        .unwrap();

        // related_to is from=* to=*, so note -> topic is legal.
        add_edge(&root, &source, "related_to", &target).unwrap();
        assert!(crate::validate::validate(&root).unwrap().is_ok());

        // subtopic_of requires a topic source; a note source is rejected.
        assert!(add_edge(&root, &source, "subtopic_of", &target).is_err());
        // An unknown predicate is rejected.
        assert!(add_edge(&root, &source, "bogus", &target).is_err());

        std::fs::remove_dir_all(root).unwrap();
    }
}
