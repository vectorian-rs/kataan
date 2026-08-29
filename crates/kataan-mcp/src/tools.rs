//! The MCP tool surface over a kataan vault. Reads call `kataan-core` /
//! `kataan-search` directly and return JSON (never HTML — rendering lives only
//! in kataan-server). Writes go through `kataan_core::mutate`, which guarantees
//! a well-formed vault, then reindex search so the session stays fresh.
//!
//! Every call loads the vault on demand. For a personal knowledge base that is
//! cheap and always current; there is no cache to invalidate.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};

use kataan_core::{
    id::CanonicalId,
    mutate::{self, DocumentPatch, NewDocument},
    schema::schema_response,
    vault::{LoadedVault, Vault},
};
use kataan_search::{SearchIndex, SearchQuery};

/// Rebuild the FTS index from the current vault state.
pub fn reindex_search(vault: &Path) -> Result<()> {
    let loaded = LoadedVault::load(vault)?;
    SearchIndex::open_default(vault)?.reindex_loaded(&loaded)?;
    Ok(())
}

/// The tool catalogue returned by `tools/list`, with JSON Schema for each input.
pub fn list() -> Value {
    json!([
        tool(
            "search",
            "Full-text keyword search across the vault. All filters are optional.",
            json!({
                "type": "object",
                "properties": {
                    "q": { "type": "string", "description": "Query text (BM25 keyword match)." },
                    "kind": { "type": "string", "description": "Restrict to a document kind." },
                    "type": { "type": "string", "description": "Restrict to a document type." },
                    "status": { "type": "string", "description": "Restrict to a status." },
                    "facet": { "type": "string", "description": "Restrict to a facet." },
                    "path_prefix": { "type": "string", "description": "Restrict to ids under this folder prefix." },
                    "limit": { "type": "integer", "minimum": 1 },
                    "offset": { "type": "integer", "minimum": 0 }
                }
            }),
        ),
        tool(
            "get_document",
            "Fetch one document's metadata and Markdown body by canonical id.",
            object(&[("id", "string", "Canonical id, e.g. notes/my-note.")], &["id"]),
        ),
        tool(
            "list_folders",
            "List the vault's type-to-folder mapping.",
            object(&[], &[]),
        ),
        tool(
            "get_folder",
            "List the documents and subfolders contained directly under a folder id.",
            object(&[("id", "string", "Folder id, e.g. notes.")], &["id"]),
        ),
        tool(
            "resolve",
            "Resolve a route token (alias or slug) within a type folder to a canonical id.",
            object(
                &[("type", "string", "Type folder to resolve within."), ("token", "string", "Alias or slug to resolve.")],
                &["type", "token"],
            ),
        ),
        tool(
            "resolve_path",
            "Resolve a filesystem path to a canonical document id. Accepts either file of a document pair (notes/x.md, notes/x.toml), a folder's index (resolves to the folder id), or the extensionless form. Use when you have a path from outside kataan and need an id for the other tools.",
            object(
                &[("path", "string", "Vault-relative or absolute path, e.g. notes/my-note.md.")],
                &["path"],
            ),
        ),
        tool(
            "schema",
            "Return the TOML schema for a document kind (e.g. document, ontology, index).",
            object(&[("kind", "string", "Schema kind to describe.")], &["kind"]),
        ),
        tool("vault_info", "Return the vault configuration (index).", object(&[], &[])),
        tool(
            "neighbors",
            "What a document is connected to, grouped by predicate and hydrated with each neighbor's type/title/status. Incoming edges use the ontology's inverse predicate, so this answers questions `get_document` cannot, e.g. \"who works at this organization\". Prefer this over `subgraph` for a single document.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Canonical id, e.g. organizations/bull." },
                    "predicate": { "type": "string", "description": "Restrict to one predicate; omit for all." },
                    "direction": {
                        "type": "string",
                        "enum": ["out", "in", "both"],
                        "description": "`out` = edges this document declares, `in` = edges pointing at it, `both` (default)."
                    }
                },
                "required": ["id"]
            }),
        ),
        tool(
            "subgraph",
            "Export nodes and links for the whole vault in one call. Each edge appears once, in the direction it was authored. Can be large — filter by types/predicates, and prefer `neighbors` when you only need one document's connections.",
            json!({
                "type": "object",
                "properties": {
                    "types": { "type": "array", "items": { "type": "string" }, "description": "Restrict to these document types; omit for all." },
                    "predicates": { "type": "array", "items": { "type": "string" }, "description": "Restrict to these edge predicates; omit for all." }
                }
            }),
        ),
        tool(
            "create_document",
            "Create a new document. Returns its canonical id. The vault is revalidated and reindexed.",
            json!({
                "type": "object",
                "properties": {
                    "type": { "type": "string", "description": "Document type (must be registered)." },
                    "title": { "type": "string", "description": "Human title; slugified into the id." },
                    "body": { "type": "string", "description": "Markdown body." },
                    "parent": { "type": "string", "description": "Folder id to place under; defaults to the type's folder." },
                    "aliases": { "type": "array", "items": { "type": "string" } },
                    "labels": { "type": "array", "items": { "type": "string" } },
                    "status": { "type": "string", "description": "One of the allowed status values." },
                    "fields": {
                        "type": "object",
                        "description": "Extra top-level sidecar keys to write, e.g. {\"linkedin\": \"https://...\"}. Keys kataan defines (type, status, markdown, aliases, labels, edges, ...) are rejected."
                    }
                },
                "required": ["type", "title", "body"]
            }),
        ),
        tool(
            "update_document",
            "Update a document's body and/or metadata. Omitted fields are left unchanged.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Canonical id of the document to update." },
                    "body": { "type": "string", "description": "New Markdown body (omit to keep)." },
                    "status": { "type": "string" },
                    "aliases": { "type": "array", "items": { "type": "string" } },
                    "labels": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["id"]
            }),
        ),
        tool(
            "add_edge",
            "Add an ontology-validated edge source --predicate--> target.",
            object(
                &[
                    ("source", "string", "Source document id."),
                    ("predicate", "string", "Edge predicate (must exist in the ontology)."),
                    ("target", "string", "Target document id."),
                ],
                &["source", "predicate", "target"],
            ),
        ),
    ])
}

/// Execute a tool by name. `Err` becomes an `isError` tool result upstream.
pub fn call(vault: &Path, name: &str, args: &Value) -> Result<String> {
    match name {
        "search" => search(vault, args),
        "get_document" => get_document(vault, args),
        "list_folders" => list_folders(vault),
        "get_folder" => get_folder(vault, args),
        "resolve" => resolve(vault, args),
        "resolve_path" => resolve_path(vault, args),
        "schema" => schema(vault, args),
        "vault_info" => vault_info(vault),
        "neighbors" => neighbors(vault, args),
        "subgraph" => subgraph(vault, args),
        "create_document" => create_document(vault, args),
        "update_document" => update_document(vault, args),
        "add_edge" => add_edge(vault, args),
        other => Err(anyhow!("unknown tool `{other}`")),
    }
}

fn search(vault: &Path, args: &Value) -> Result<String> {
    let query: SearchQuery =
        serde_json::from_value(args.clone()).context("invalid search arguments")?;
    let response = SearchIndex::open_default(vault)?.search(&query)?;
    to_pretty(&response)
}

fn get_document(vault: &Path, args: &Value) -> Result<String> {
    let id = parse_id(args, "id")?;
    let document = Vault::open(vault)?.load_document(&id)?;
    to_pretty(&json!({
        "id": document.id.as_str(),
        "metadata": document.metadata,
        "markdown": document.markdown,
        "ancestors": document.ancestors,
        "facets": document.facets,
    }))
}

fn list_folders(vault: &Path) -> Result<String> {
    let vault = Vault::open(vault)?;
    to_pretty(&json!({ "type_folders": vault.index.type_folders }))
}

fn get_folder(vault: &Path, args: &Value) -> Result<String> {
    let id = parse_id(args, "id")?;
    let loaded = LoadedVault::load(vault)?;
    let (mut folders, mut documents) = (Vec::new(), Vec::new());
    for child in loaded.graph.children_of(&id) {
        let is_folder = loaded
            .documents
            .get(&child)
            .is_some_and(|record| record.is_folder_index);
        if is_folder {
            folders.push(child.as_str().to_owned());
        } else {
            documents.push(child.as_str().to_owned());
        }
    }
    to_pretty(&json!({ "id": id.as_str(), "folders": folders, "documents": documents }))
}

fn resolve(vault: &Path, args: &Value) -> Result<String> {
    let type_folder = str_arg(args, "type")?;
    let token = str_arg(args, "token")?;
    let loaded = LoadedVault::load(vault)?;
    let id = loaded
        .resolve_route_token(&type_folder, &token)
        .ok_or_else(|| anyhow!("`{token}` does not resolve within `{type_folder}`"))?;
    to_pretty(&json!({ "id": id.as_str() }))
}

fn resolve_path(vault: &Path, args: &Value) -> Result<String> {
    let path = str_arg(args, "path")?;
    let loaded = LoadedVault::load(vault)?;
    let id = loaded
        .resolve_path(&path)
        .ok_or_else(|| anyhow!("`{path}` does not resolve to a document in this vault"))?;
    to_pretty(&json!({
        "id": id.as_str(),
        "is_folder_index": loaded
            .documents
            .get(id)
            .is_some_and(|record| record.is_folder_index),
    }))
}

fn schema(vault: &Path, args: &Value) -> Result<String> {
    let kind = str_arg(args, "kind")?;
    let loaded = LoadedVault::load(vault).ok();
    let response = schema_response(&kind, loaded.as_ref())
        .ok_or_else(|| anyhow!("unknown schema kind `{kind}`"))?;
    to_pretty(&response)
}

fn vault_info(vault: &Path) -> Result<String> {
    to_pretty(&Vault::open(vault)?.index)
}

fn neighbors(vault: &Path, args: &Value) -> Result<String> {
    let id = parse_id(args, "id")?;
    let direction = match opt_str(args, "direction") {
        Some(value) => value.parse().map_err(|error: String| anyhow!(error))?,
        None => kataan_core::query::Direction::Both,
    };
    let loaded = LoadedVault::load(vault)?;
    let result = kataan_core::query::neighbors(
        &loaded,
        &id,
        opt_str(args, "predicate").as_deref(),
        direction,
    )?;
    to_pretty(&result)
}

fn subgraph(vault: &Path, args: &Value) -> Result<String> {
    let loaded = LoadedVault::load(vault)?;
    let graph = kataan_core::query::subgraph(
        &loaded,
        &str_vec(args, "types"),
        &str_vec(args, "predicates"),
    );
    to_pretty(&graph)
}

fn create_document(vault: &Path, args: &Value) -> Result<String> {
    let request = NewDocument {
        r#type: str_arg(args, "type")?,
        title: str_arg(args, "title")?,
        body: str_arg(args, "body")?,
        parent: opt_str(args, "parent"),
        aliases: str_vec(args, "aliases"),
        labels: str_vec(args, "labels"),
        status: opt_str(args, "status"),
        // Writes over MCP are always attributed to the agent actor.
        actor: None,
        extra: extra_fields(args, "fields"),
    };
    let id = mutate::create_document(vault, request)?;
    reindex_search(vault)?;
    to_pretty(&json!({ "id": id.as_str() }))
}

fn update_document(vault: &Path, args: &Value) -> Result<String> {
    let id = parse_id(args, "id")?;
    let patch = DocumentPatch {
        status: opt_str(args, "status"),
        aliases: opt_str_vec(args, "aliases"),
        labels: opt_str_vec(args, "labels"),
        // Writes over MCP are always attributed to the agent actor.
        actor: None,
    };
    mutate::update_document(vault, &id, opt_str(args, "body"), patch)?;
    reindex_search(vault)?;
    to_pretty(&json!({ "id": id.as_str(), "updated": true }))
}

fn add_edge(vault: &Path, args: &Value) -> Result<String> {
    let source = parse_id(args, "source")?;
    let target = parse_id(args, "target")?;
    let predicate = str_arg(args, "predicate")?;
    mutate::add_edge(vault, &source, &predicate, &target)?;
    reindex_search(vault)?;
    to_pretty(
        &json!({ "source": source.as_str(), "predicate": predicate, "target": target.as_str() }),
    )
}

// --- helpers ---------------------------------------------------------------

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({ "name": name, "description": description, "inputSchema": input_schema })
}

/// Build an object schema from `(name, json-type, description)` fields.
fn object(fields: &[(&str, &str, &str)], required: &[&str]) -> Value {
    let mut properties = serde_json::Map::new();
    for (name, ty, description) in fields {
        properties.insert(
            (*name).to_owned(),
            json!({ "type": ty, "description": description }),
        );
    }
    json!({ "type": "object", "properties": properties, "required": required })
}

fn to_pretty<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string_pretty(value).context("failed to serialize response")
}

fn str_arg(args: &Value, key: &str) -> Result<String> {
    opt_str(args, key).ok_or_else(|| anyhow!("missing string argument `{key}`"))
}

fn opt_str(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn str_vec(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// `Some(list)` only when `key` is present, so an omitted field leaves a patch
/// field unchanged rather than clearing it.
fn opt_str_vec(args: &Value, key: &str) -> Option<Vec<String>> {
    args.get(key).map(|_| str_vec(args, key))
}

/// Extra sidecar fields from an object-valued argument. `mutate` rejects any
/// key kataan defines, so no filtering is needed here.
fn extra_fields(args: &Value, key: &str) -> std::collections::BTreeMap<String, toml::Value> {
    args.get(key)
        .and_then(Value::as_object)
        .map(|fields| {
            fields
                .iter()
                .filter_map(|(name, value)| Some((name.clone(), json_to_toml(value)?)))
                .collect()
        })
        .unwrap_or_default()
}

/// Convert a JSON tool argument to TOML. JSON null has no TOML representation,
/// so null-valued entries are dropped rather than written as something else.
fn json_to_toml(value: &Value) -> Option<toml::Value> {
    Some(match value {
        Value::Null => return None,
        Value::Bool(value) => toml::Value::Boolean(*value),
        Value::Number(number) => match number.as_i64() {
            Some(integer) => toml::Value::Integer(integer),
            None => toml::Value::Float(number.as_f64()?),
        },
        Value::String(value) => toml::Value::String(value.clone()),
        Value::Array(items) => toml::Value::Array(items.iter().filter_map(json_to_toml).collect()),
        Value::Object(fields) => toml::Value::Table(
            fields
                .iter()
                .filter_map(|(name, value)| Some((name.clone(), json_to_toml(value)?)))
                .collect(),
        ),
    })
}

fn parse_id(args: &Value, key: &str) -> Result<CanonicalId> {
    CanonicalId::parse(str_arg(args, key)?).map_err(|error| anyhow!("invalid `{key}`: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    /// A fresh initialized vault in a temp dir (auto-removed on drop).
    fn temp_vault() -> TempDir {
        let dir = TempDir::new().unwrap();
        kataan_core::init::init_vault(dir.path(), "Test").unwrap();
        dir
    }

    /// Parse a read tool's JSON string result back into a Value.
    fn json_result(vault: &Path, name: &str, args: Value) -> Value {
        serde_json::from_str(&call(vault, name, &args).unwrap()).unwrap()
    }

    #[test]
    fn create_then_get_and_search_round_trips() {
        let dir = temp_vault();
        let vault = dir.path();

        let created = json_result(
            vault,
            "create_document",
            json!({ "type": "note", "title": "Round Trip", "body": "hello world", "status": "active" }),
        );
        assert_eq!(created["id"], "notes/round-trip");

        // create_document reindexes, so the new doc is immediately searchable.
        let search = json_result(vault, "search", json!({ "q": "hello" }));
        let hits = search["results"].as_array().unwrap();
        assert!(hits.iter().any(|hit| hit["id"] == "notes/round-trip"));

        let document = json_result(vault, "get_document", json!({ "id": "notes/round-trip" }));
        assert_eq!(document["markdown"], "hello world");
        assert_eq!(document["metadata"]["type"], "note");
    }

    #[test]
    fn update_document_changes_body() {
        let dir = temp_vault();
        let vault = dir.path();
        call(
            vault,
            "create_document",
            &json!({ "type": "note", "title": "Edit Me", "body": "before" }),
        )
        .unwrap();

        call(
            vault,
            "update_document",
            &json!({ "id": "notes/edit-me", "body": "after" }),
        )
        .unwrap();

        let document = json_result(vault, "get_document", json!({ "id": "notes/edit-me" }));
        assert_eq!(document["markdown"], "after");
    }

    #[test]
    fn add_edge_accepts_legal_and_rejects_illegal() {
        let dir = temp_vault();
        let vault = dir.path();
        call(
            vault,
            "create_document",
            &json!({ "type": "note", "title": "A", "body": "a" }),
        )
        .unwrap();
        call(
            vault,
            "create_document",
            &json!({ "type": "topic", "title": "B", "body": "b" }),
        )
        .unwrap();

        // related_to is from=* to=*, so note -> topic is legal.
        assert!(call(
            vault,
            "add_edge",
            &json!({ "source": "notes/a", "predicate": "related_to", "target": "topics/b" })
        )
        .is_ok());
        // subtopic_of requires a topic source; a note source is rejected.
        assert!(call(
            vault,
            "add_edge",
            &json!({ "source": "notes/a", "predicate": "subtopic_of", "target": "topics/b" })
        )
        .is_err());
    }

    #[test]
    fn writes_keep_the_vault_valid() {
        let dir = temp_vault();
        let vault = dir.path();
        call(
            vault,
            "create_document",
            &json!({ "type": "note", "title": "Valid", "body": "x", "status": "active" }),
        )
        .unwrap();
        assert!(kataan_core::validate::validate(vault).unwrap().is_ok());
    }

    #[test]
    fn custom_fields_survive_a_create_update_edge_cycle() {
        let dir = temp_vault();
        let vault = dir.path();

        call(
            vault,
            "create_document",
            &json!({
                "type": "note", "title": "Jane", "body": "hello",
                "fields": { "linkedin": "https://example.com/in/jane", "emails": ["jane@example.com"] }
            }),
        )
        .unwrap();
        call(
            vault,
            "create_document",
            &json!({ "type": "topic", "title": "Rust", "body": "r" }),
        )
        .unwrap();

        // The fields are readable back through get_document...
        let document = json_result(vault, "get_document", json!({ "id": "notes/jane" }));
        assert_eq!(
            document["metadata"]["linkedin"],
            "https://example.com/in/jane"
        );
        assert_eq!(document["metadata"]["emails"][0], "jane@example.com");

        // ...and survive both write paths that used to drop them.
        call(
            vault,
            "update_document",
            &json!({ "id": "notes/jane", "body": "changed", "status": "active" }),
        )
        .unwrap();
        call(
            vault,
            "add_edge",
            &json!({ "source": "notes/jane", "predicate": "related_to", "target": "topics/rust" }),
        )
        .unwrap();

        let document = json_result(vault, "get_document", json!({ "id": "notes/jane" }));
        assert_eq!(
            document["metadata"]["linkedin"], "https://example.com/in/jane",
            "custom key lost across update_document/add_edge"
        );
        assert_eq!(document["metadata"]["emails"][0], "jane@example.com");
        assert!(kataan_core::validate::validate(vault).unwrap().is_ok());
    }

    #[test]
    fn create_document_rejects_reserved_custom_fields() {
        let dir = temp_vault();
        assert!(call(
            dir.path(),
            "create_document",
            &json!({
                "type": "note", "title": "Bad", "body": "x",
                "fields": { "type": "person" }
            })
        )
        .is_err());
    }

    #[test]
    fn resolve_path_maps_paths_to_ids() {
        let dir = temp_vault();
        let vault = dir.path();
        call(
            vault,
            "create_document",
            &json!({ "type": "note", "title": "Field Notes", "body": "x" }),
        )
        .unwrap();

        for spelling in [
            "notes/field-notes.md",
            "notes/field-notes.toml",
            "notes/field-notes",
        ] {
            let resolved = json_result(vault, "resolve_path", json!({ "path": spelling }));
            assert_eq!(resolved["id"], "notes/field-notes", "failed on {spelling}");
        }

        // A folder index resolves to the folder id.
        let folder = json_result(vault, "resolve_path", json!({ "path": "notes/index.toml" }));
        assert_eq!(folder["id"], "notes");
        assert_eq!(folder["is_folder_index"], true);

        // Escapes and misses are errors, not dangling ids.
        for bad in ["../secrets.md", "notes/nope.md", "/etc/passwd"] {
            assert!(
                call(vault, "resolve_path", &json!({ "path": bad })).is_err(),
                "`{bad}` must not resolve"
            );
        }
    }

    #[test]
    fn unknown_tool_errors() {
        let dir = temp_vault();
        assert!(call(dir.path(), "no_such_tool", &json!({})).is_err());
    }

    #[test]
    fn get_document_on_missing_id_errors() {
        let dir = temp_vault();
        assert!(call(dir.path(), "get_document", &json!({ "id": "notes/nope" })).is_err());
    }
}
