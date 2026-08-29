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
    "edges",
];

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
            .ok_or_else(|| invalid_request(format!("unknown type `{}`", request.r#type)))?,
    };
    validate_status(request.status.as_deref())?;
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
    let sidecar = Sidecar {
        r#type: request.r#type,
        status: request.status,
        markdown: format!("{}.md", id.slug()),
        aliases: request.aliases,
        labels: request.labels,
        created_by: Some(actor.clone()),
        last_updated_by: Some(actor),
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
    sidecar.insert(
        "last_updated_by".to_owned(),
        toml::Value::String(patch.actor.unwrap_or_else(|| ACTOR_AGENT.to_owned())),
    );

    if let Some(body) = body {
        atomic_write_string(&record.markdown_path, &body)?;
    }
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
    let edge = ontology
        .edges
        .get(predicate)
        .ok_or_else(|| invalid_request(format!("unknown predicate `{predicate}`")))?;
    let source_record = vault.load_document_record(source)?;
    if !ontology::type_allowed(&edge.from, &source_record.metadata.r#type) {
        return Err(invalid_request(format!(
            "type `{}` cannot be the source of `{predicate}`",
            source_record.metadata.r#type
        )));
    }
    let target_type = vault.load_document_record(target)?.metadata.r#type;
    if !ontology::type_allowed(&edge.to, &target_type) {
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

    write_sidecar_table(&source_record.toml_path, &sidecar)?;
    rebuild::rebuild_indexes(root)?;
    Ok(())
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

    /// A sidecar carrying the three shapes an author can write that kataan does
    /// not model: a custom scalar, a custom array, and a custom array-of-tables.
    fn write_custom_keys(root: &std::path::Path, id: &CanonicalId) {
        let path = root.join(id.toml_path());
        let mut table = read_sidecar_table(&path).unwrap();
        table.insert(
            "linkedin".to_owned(),
            toml::Value::String("https://example.com/in/jane".to_owned()),
        );
        table.insert(
            "emails".to_owned(),
            string_array(vec!["jane@example.com".to_owned()]),
        );
        let mut employment = toml::Table::new();
        employment.insert(
            "from".to_owned(),
            toml::Value::String("2020-01-01".to_owned()),
        );
        table.insert(
            "employment".to_owned(),
            toml::Value::Array(vec![toml::Value::Table(employment)]),
        );
        write_sidecar_table(&path, &table).unwrap();
    }

    fn assert_custom_keys_intact(root: &std::path::Path, id: &CanonicalId) {
        let table = read_sidecar_table(&root.join(id.toml_path())).unwrap();
        assert_eq!(
            table["linkedin"].as_str(),
            Some("https://example.com/in/jane"),
            "custom scalar was dropped"
        );
        assert_eq!(
            table["emails"].as_array().unwrap()[0].as_str(),
            Some("jane@example.com"),
            "custom array was dropped"
        );
        assert_eq!(
            table["employment"].as_array().unwrap()[0]["from"].as_str(),
            Some("2020-01-01"),
            "custom array-of-tables was dropped"
        );
    }

    #[test]
    fn update_document_preserves_unknown_sidecar_keys() {
        let root = temp_vault("preserve-update");
        let id = create_document(&root, note("Jane", "hello")).unwrap();
        write_custom_keys(&root, &id);

        update_document(
            &root,
            &id,
            Some("changed".to_owned()),
            DocumentPatch {
                status: Some("active".to_owned()),
                ..Default::default()
            },
        )
        .unwrap();

        assert_custom_keys_intact(&root, &id);
        let table = read_sidecar_table(&root.join(id.toml_path())).unwrap();
        assert_eq!(table["status"].as_str(), Some("active"));
        assert!(crate::validate::validate(&root).unwrap().is_ok());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn add_edge_preserves_sibling_keys() {
        let root = temp_vault("preserve-edge");
        let source = create_document(&root, note("Jane", "a")).unwrap();
        let target = create_document(
            &root,
            NewDocument {
                r#type: "topic".to_owned(),
                ..note("B", "b")
            },
        )
        .unwrap();
        write_custom_keys(&root, &source);

        add_edge(&root, &source, "related_to", &target).unwrap();

        assert_custom_keys_intact(&root, &source);
        let table = read_sidecar_table(&root.join(source.toml_path())).unwrap();
        assert_eq!(
            table["edges"]["related_to"].as_array().unwrap()[0].as_str(),
            Some("topics/b")
        );
        assert!(crate::validate::validate(&root).unwrap().is_ok());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_update_touches_only_the_keys_it_changes() {
        let root = temp_vault("minimal-diff");
        let id = create_document(&root, note("Jane", "hello")).unwrap();
        write_custom_keys(&root, &id);
        let path = root.join(id.toml_path());
        let before = std::fs::read_to_string(&path).unwrap();

        update_document(&root, &id, None, DocumentPatch::default()).unwrap();
        let after = std::fs::read_to_string(&path).unwrap();

        // `last_updated_by` is already `agent`, so a no-op patch must leave the
        // file byte-identical — key order included.
        assert_eq!(before, after);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn create_document_writes_and_rejects_extra_fields() {
        let root = temp_vault("create-extra");

        let id = create_document(
            &root,
            NewDocument {
                extra: BTreeMap::from([(
                    "linkedin".to_owned(),
                    toml::Value::String("https://example.com/in/jane".to_owned()),
                )]),
                ..note("Jane", "hello")
            },
        )
        .unwrap();

        let table = read_sidecar_table(&root.join(id.toml_path())).unwrap();
        assert_eq!(
            table["linkedin"].as_str(),
            Some("https://example.com/in/jane")
        );
        assert!(crate::validate::validate(&root).unwrap().is_ok());

        // A reserved key would serialize twice and produce invalid TOML.
        let reserved = create_document(
            &root,
            NewDocument {
                extra: BTreeMap::from([(
                    "type".to_owned(),
                    toml::Value::String("person".to_owned()),
                )]),
                ..note("Reserved", "x")
            },
        );
        assert!(reserved.is_err());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_keys_are_readable_through_document_metadata() {
        let root = temp_vault("expose-extra");
        let id = create_document(&root, note("Jane", "hello")).unwrap();
        write_custom_keys(&root, &id);

        let record = Vault::open(&root)
            .unwrap()
            .load_document_record(&id)
            .unwrap();

        assert_eq!(
            record.metadata.extra["linkedin"].as_str(),
            Some("https://example.com/in/jane")
        );
        assert!(record.metadata.extra.contains_key("employment"));
        // Keys kataan models must not leak into `extra`.
        for reserved in RESERVED_KEYS {
            assert!(
                !record.metadata.extra.contains_key(*reserved),
                "`{reserved}` leaked into extra"
            );
        }

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rebuild_indexes_is_idempotent_over_custom_keys() {
        let root = temp_vault("rebuild-extra");
        let id = create_document(&root, note("Jane", "hello")).unwrap();
        write_custom_keys(&root, &id);

        crate::rebuild::rebuild_indexes(&root).unwrap();
        let once = std::fs::read_to_string(root.join(id.toml_path())).unwrap();
        crate::rebuild::rebuild_indexes(&root).unwrap();
        let twice = std::fs::read_to_string(root.join(id.toml_path())).unwrap();

        assert_eq!(once, twice);
        assert_custom_keys_intact(&root, &id);
        assert!(crate::validate::validate(&root).unwrap().is_ok());

        std::fs::remove_dir_all(root).unwrap();
    }
}
