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

/// Refresh the search index after a committed write.
///
/// The write is already durable, so a failure here must not turn a success
/// into an `isError` result — an agent that sees one retries and creates a
/// duplicate document. The index is a derived cache; log and carry on.
fn refresh_search_after_write(vault: &Path, changed: &kataan_core::id::CanonicalId) {
    if let Err(error) = refresh_search_for(vault, changed) {
        tracing::warn!(error = %error, "search refresh after write failed; index is stale");
    }
}

/// Update the one document a write touched, falling back to a full rebuild when
/// the index cannot be amended in place — it does not exist yet, or predates the
/// current schema.
fn refresh_search_for(vault: &Path, changed: &kataan_core::id::CanonicalId) -> Result<()> {
    let loaded = LoadedVault::load(vault)?;
    let index = SearchIndex::open_default(vault)?;
    if !index.refresh_document(&loaded, changed)? {
        index.reindex_loaded(&loaded)?;
    }
    Ok(())
}

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
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Results per page, default 20. Values above 100 are clamped to 100. `facets` always count the whole match set, not this page, so they do not change as you page." },
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
            "documents",
            "List or batch-fetch documents. Replaces fetching ids one at a time. All filters optional; an empty query lists the vault. Returns metadata by default — ask for markdown only when you need bodies, since each one is a file read. Matching more documents than `limit` is an error, not a truncation: narrow the query or page with `offset`.",
            json!({
                "type": "object",
                "properties": {
                    "ids": { "type": "array", "items": { "type": "string" }, "description": "Fetch these ids; order preserved, unknown ids returned in `missing`." },
                    "type": { "type": "string", "description": "Restrict to a document type." },
                    "status": { "type": "string", "description": "Restrict to a status." },
                    "labels": { "type": "array", "items": { "type": "string" }, "description": "Documents carrying every one of these labels." },
                    "path_prefix": { "type": "string", "description": "Restrict to ids under this folder." },
                    "linked_to": { "type": "string", "description": "Restrict to documents with an edge to this id." },
                    "predicate": { "type": "string", "description": "With linked_to: restrict to one predicate." },
                    "direction": { "type": "string", "enum": ["out", "in", "both"], "description": "With linked_to: which direction to follow." },
                    "after": { "type": "string", "description": "Only documents whose occurred_at is on or after this RFC 3339 bound. Inclusive, and compared at the bound's own precision — a bare 2026-08-29 covers that whole day, instants included. Documents with no occurred_at are excluded whenever a bound is given." },
                    "before": { "type": "string", "description": "Only documents whose occurred_at is on or before this bound. Same precision rule as `after`." },
                    "order": { "type": "string", "enum": ["id", "occurred_at", "created_at", "updated_at"], "description": "Sort key, default `id`. Ties always break on id, so paging is stable. Documents missing the chosen timestamp sort last in both directions." },
                    "desc": { "type": "boolean", "description": "Reverse the order. `order: updated_at` with this set answers \"what changed most recently\"." },
                    "include": { "type": "string", "enum": ["metadata", "full", "markdown"], "description": "`metadata` (default) is the summary; `full` adds each document's declared fields, timestamps and edges at no extra cost; `markdown` adds the body, which is one file read per document." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 1000, "description": "Page size. Above 1000 is an error, not a clamp; omitting it errors rather than truncating when more documents match than the default." },
                    "offset": { "type": "integer", "minimum": 0 }
                }
            }),
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
            "resolve_path",
            "Resolve a filesystem path to a canonical document id. Accepts either file of a document pair (notes/x.md, notes/x.toml), a folder's index (resolves to the folder id), or the extensionless form. Use when you have a path from outside kataan and need an id for the other tools.",
            object(
                &[("path", "string", "Vault-relative or absolute path, e.g. notes/my-note.md.")],
                &["path"],
            ),
        ),
        tool(
            "schema",
            "Describe what a document of some kind must contain. Pass one of kataan's own kinds (document, folder-index, vault, type-definition, ontology, edge-predicate) or, more usefully, one of this vault's document types (person, project, ...) — that returns the type's `[nodes.*]` declaration: which fields are required, and what type each must be. The write boundary enforces exactly this, so checking here is how you avoid a rejected write.",
            object(&[("kind", "string", "A kataan schema kind, or one of this vault's document types.")], &["kind"]),
        ),
        tool(
            "ontology",
            "The vault's whole model in one call: every document type with the fields it declares and how many documents it has, every edge predicate with its permitted endpoint types, and `links` — the type-level graph of what may connect to what. Read this once before writing, rather than discovering the rules by being rejected.",
            object(&[], &[]),
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
                    "occurred_at": { "type": "string", "description": "When the thing this document describes happened. RFC 3339, and only RFC 3339: a calendar day (2026-08-29) or a moment (2026-08-29T12:00:00Z). A bare 2026 or 2026-08 is ISO 8601 but not RFC 3339 and is rejected." },
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
                    "occurred_at": { "type": "string", "description": "When the thing this document describes happened. RFC 3339, and only RFC 3339: a calendar day (2026-08-29) or a moment (2026-08-29T12:00:00Z). A bare 2026 or 2026-08 is ISO 8601 but not RFC 3339 and is rejected." },
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
        tool(
            "remove_edge",
            "Remove the edge source --predicate--> target. Not ontology-validated: an edge worth removing is often one the ontology now forbids, or whose target is gone. Removing an edge that is not there succeeds and changes nothing.",
            object(
                &[
                    ("source", "string", "Source document id."),
                    ("predicate", "string", "Edge predicate."),
                    ("target", "string", "Target document id."),
                ],
                &["source", "predicate", "target"],
            ),
        ),
        tool(
            "replace_edges_for_predicate",
            "Set the complete list of targets for one predicate on one source, replacing whatever was there. Every new target is ontology-validated. An empty list removes the predicate. Use this to correct a wrong edge in one write.",
            json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "Source document id." },
                    "predicate": { "type": "string", "description": "Edge predicate (must exist in the ontology)." },
                    "targets": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "The complete set of target ids for this predicate. Empty removes it."
                    }
                },
                "required": ["source", "predicate", "targets"]
            }),
        ),
    ])
}

/// Execute a tool by name. `Err` becomes an `isError` tool result upstream.
pub fn call(vault: &Path, name: &str, args: &Value) -> Result<String> {
    match name {
        "search" => search(vault, args),
        "get_document" => get_document(vault, args),
        "documents" => documents(vault, args),
        "list_folders" => list_folders(vault),
        "get_folder" => get_folder(vault, args),
        "resolve_path" => resolve_path(vault, args),
        "schema" => schema(vault, args),
        "ontology" => ontology(vault),
        "vault_info" => vault_info(vault),
        "neighbors" => neighbors(vault, args),
        "subgraph" => subgraph(vault, args),
        "create_document" => create_document(vault, args),
        "update_document" => update_document(vault, args),
        "add_edge" => add_edge(vault, args),
        "remove_edge" => remove_edge(vault, args),
        "replace_edges_for_predicate" => replace_edges_for_predicate(vault, args),
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

fn documents(vault: &Path, args: &Value) -> Result<String> {
    let direction = opt_direction(args)?;
    // `Include` already derives Deserialize with the lowercase names.
    let include = match args.get("include") {
        Some(value) => serde_json::from_value(value.clone()).context("invalid `include`")?,
        None => kataan_core::query::Include::default(),
    };
    let query = kataan_core::query::DocumentQuery {
        ids: str_vec(args, "ids"),
        r#type: opt_str(args, "type"),
        status: opt_str(args, "status"),
        labels: str_vec(args, "labels"),
        path_prefix: opt_str(args, "path_prefix"),
        linked_to: opt_str(args, "linked_to").map(|id| kataan_core::query::LinkedTo {
            id,
            predicate: opt_str(args, "predicate"),
            direction,
        }),
        after: opt_str(args, "after"),
        before: opt_str(args, "before"),
        order: match args.get("order") {
            Some(value) => serde_json::from_value(value.clone()).context("invalid `order`")?,
            None => kataan_core::query::Order::default(),
        },
        desc: args
            .get("desc")
            .and_then(Value::as_bool)
            .unwrap_or_default(),
        include,
        limit: args
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| n as usize),
        offset: args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize,
    };
    let loaded = LoadedVault::load(vault)?;
    to_pretty(&kataan_core::query::documents(&loaded, &query)?)
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

fn ontology(vault: &Path) -> Result<String> {
    let loaded = LoadedVault::load(vault)?;
    to_pretty(&kataan_core::schema::ontology_response(&loaded))
}

fn vault_info(vault: &Path) -> Result<String> {
    to_pretty(&Vault::open(vault)?.index)
}

fn neighbors(vault: &Path, args: &Value) -> Result<String> {
    let id = parse_id(args, "id")?;
    let direction = opt_direction(args)?;
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
        occurred_at: opt_str(args, "occurred_at"),
        extra: extra_fields(args, "fields"),
    };
    let id = mutate::create_document(vault, request)?;
    refresh_search_after_write(vault, &id);
    to_pretty(&json!({ "id": id.as_str() }))
}

fn update_document(vault: &Path, args: &Value) -> Result<String> {
    let id = parse_id(args, "id")?;
    let patch = DocumentPatch {
        status: opt_str(args, "status"),
        occurred_at: opt_str(args, "occurred_at"),
        aliases: opt_str_vec(args, "aliases"),
        labels: opt_str_vec(args, "labels"),
        // Writes over MCP are always attributed to the agent actor.
        actor: None,
    };
    mutate::update_document(vault, &id, opt_str(args, "body"), patch)?;
    refresh_search_after_write(vault, &id);
    to_pretty(&json!({ "id": id.as_str(), "updated": true }))
}

fn remove_edge(vault: &Path, args: &Value) -> Result<String> {
    let source = parse_id(args, "source")?;
    let target = parse_id(args, "target")?;
    let predicate = str_arg(args, "predicate")?;
    mutate::remove_edge(vault, &source, &predicate, &target)?;
    refresh_search_after_write(vault, &source);
    to_pretty(
        &json!({ "source": source.as_str(), "predicate": predicate, "target": target.as_str(), "removed": true }),
    )
}

fn replace_edges_for_predicate(vault: &Path, args: &Value) -> Result<String> {
    let source = parse_id(args, "source")?;
    let predicate = str_arg(args, "predicate")?;
    let targets = str_vec(args, "targets")
        .into_iter()
        .map(|target| {
            kataan_core::id::CanonicalId::parse(&target)
                .map_err(|error| anyhow!("invalid target `{target}`: {error}"))
        })
        .collect::<Result<Vec<_>>>()?;
    mutate::replace_edges_for_predicate(vault, &source, &predicate, &targets)?;
    refresh_search_after_write(vault, &source);
    to_pretty(&json!({
        "source": source.as_str(),
        "predicate": predicate,
        "targets": targets.iter().map(|t| t.as_str()).collect::<Vec<_>>(),
    }))
}

fn add_edge(vault: &Path, args: &Value) -> Result<String> {
    let source = parse_id(args, "source")?;
    let target = parse_id(args, "target")?;
    let predicate = str_arg(args, "predicate")?;
    mutate::add_edge(vault, &source, &predicate, &target)?;
    refresh_search_after_write(vault, &source);
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
    match args.get(key).and_then(json_to_toml) {
        Some(toml::Value::Table(fields)) => fields.into_iter().collect(),
        _ => Default::default(),
    }
}

/// `direction` defaults to the enum's own default rather than restating it.
fn opt_direction(args: &Value) -> Result<kataan_core::query::Direction> {
    match opt_str(args, "direction") {
        Some(value) => value.parse().map_err(|error: String| anyhow!(error)),
        None => Ok(kataan_core::query::Direction::default()),
    }
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

    /// Edges were append-only through every surface: a wrong one could only be
    /// corrected by hand-editing TOML, which bypasses ontology validation.
    /// An agent has to be able to learn a type's rules before writing, now
    /// that the write boundary enforces them.
    #[test]
    fn schema_and_ontology_expose_the_vaults_own_model() {
        let dir = temp_vault();
        let vault = dir.path();
        let path = vault.join("ontology.toml");
        let existing = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            &path,
            format!(
                "{existing}\n[nodes.person]\nrequired = [\"email\"]\n\n\
                 [nodes.person.fields]\nemail = {{ type = \"string\" }}\n"
            ),
        )
        .unwrap();

        // Asking for a vault type returns that type's declaration, not the
        // generic document struct.
        let person = json_result(vault, "schema", json!({ "kind": "person" }));
        assert_eq!(person["node_schema"]["required"][0], "email");
        assert!(person["toml_template"]
            .as_str()
            .unwrap()
            .contains("email = \"\""));

        // And the whole model arrives in one call.
        let ontology = json_result(vault, "ontology", json!({}));
        let types = ontology["types"].as_array().unwrap();
        assert!(types.iter().any(|ty| ty["name"] == "person"));
        assert!(ontology["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["predicate"] == "related_to"));
        assert!(!ontology["links"].as_array().unwrap().is_empty());

        // Writing blind is refused; writing what the schema described is not.
        assert!(call(
            vault,
            "create_document",
            &json!({ "type": "person", "title": "Blind", "body": "x" })
        )
        .is_err());
        call(
            vault,
            "create_document",
            &json!({ "type": "person", "title": "Informed", "body": "x",
                     "fields": { "email": "informed@example.com" } }),
        )
        .unwrap();
    }

    #[test]
    fn edges_can_be_removed_and_replaced_over_mcp() {
        let dir = temp_vault();
        let vault = dir.path();

        for (title, ty) in [("Src", "note"), ("First", "topic"), ("Second", "topic")] {
            call(
                vault,
                "create_document",
                &json!({ "type": ty, "title": title, "body": "x" }),
            )
            .unwrap();
        }
        let src = "notes/src";
        let first = "topics/first";
        let second = "topics/second";

        let neighbors = |vault: &Path| {
            json_result(vault, "neighbors", json!({ "id": src }))["out"]["related_to"]
                .as_array()
                .map(|targets| {
                    targets
                        .iter()
                        .map(|node| node["id"].as_str().unwrap().to_owned())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };

        call(
            vault,
            "add_edge",
            &json!({ "source": src, "predicate": "related_to", "target": first }),
        )
        .unwrap();
        assert_eq!(neighbors(vault), vec![first.to_owned()]);

        // Replace: the wrong target out and the right one in, in one write.
        call(
            vault,
            "replace_edges_for_predicate",
            &json!({ "source": src, "predicate": "related_to", "targets": [second] }),
        )
        .unwrap();
        assert_eq!(neighbors(vault), vec![second.to_owned()]);

        // Remove, and then remove again — the second call is not an error.
        call(
            vault,
            "remove_edge",
            &json!({ "source": src, "predicate": "related_to", "target": second }),
        )
        .unwrap();
        call(
            vault,
            "remove_edge",
            &json!({ "source": src, "predicate": "related_to", "target": second }),
        )
        .unwrap();
        assert!(neighbors(vault).is_empty());

        // A replacement is still ontology-validated, unlike a removal.
        assert!(call(
            vault,
            "replace_edges_for_predicate",
            &json!({ "source": src, "predicate": "no_such_predicate", "targets": [second] })
        )
        .is_err());
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
    fn occurred_at_is_settable_and_validated() {
        let dir = temp_vault();
        let vault = dir.path();

        // Both RFC 3339 productions are accepted and stored verbatim.
        for (title, value) in [("Day", "2006-05-18"), ("Exact", "2026-08-29T12:00:00Z")] {
            call(
                vault,
                "create_document",
                &json!({ "type": "note", "title": title, "body": "x", "occurred_at": value }),
            )
            .unwrap();
        }
        let doc = json_result(vault, "get_document", json!({ "id": "notes/day" }));
        assert_eq!(doc["metadata"]["occurred_at"], "2006-05-18");
        // Transaction time is stamped for us, in ISO-8601.
        assert!(doc["metadata"]["created_at"]
            .as_str()
            .unwrap()
            .ends_with('Z'));

        // A Unix epoch, and ISO 8601 reduced precision, are both refused at the
        // write boundary rather than stored and reported later by validate.
        assert!(call(
            vault,
            "create_document",
            &json!({ "type": "note", "title": "Year", "body": "x", "occurred_at": "2026" })
        )
        .is_err());
        assert!(call(
            vault,
            "create_document",
            &json!({ "type": "note", "title": "Bad", "body": "x", "occurred_at": "1788013953" })
        )
        .is_err());
        assert!(call(
            vault,
            "update_document",
            &json!({ "id": "notes/day", "occurred_at": "2026-08-29T12:00:00" })
        )
        .is_err());

        assert!(kataan_core::validate::validate(vault).unwrap().is_ok());
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
