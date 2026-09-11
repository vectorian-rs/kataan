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
    mutate,
    schema::schema_response,
    vault::{LoadedVault, Vault},
};
use kataan_search::{SearchIndex, SearchQuery};

mod args;
mod catalogue;

pub use catalogue::list;

use args::*;

/// Serialize a tool's result. Every read returns JSON, never HTML.
fn to_pretty<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string_pretty(value).context("failed to serialize response")
}

/// Deserialize a tool's whole argument object, naming the tool if it fails.
///
/// The `{error:#}` is deliberate: `to_string()` on an `anyhow::Error` prints
/// only the outermost context, so an agent was told "invalid arguments" without
/// being told which field or what was expected — everything serde had worked
/// out was dropped on the floor.
fn parse_args<T: serde::de::DeserializeOwned>(tool: &str, args: &Value) -> Result<T> {
    serde_json::from_value(args.clone())
        .map_err(|error| anyhow!("invalid arguments for `{tool}`: {error}"))
}

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
    if !index.refresh_document_tree(&loaded, changed)? {
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
    let query: SearchQuery = parse_args("search", args)?;
    let response = SearchIndex::open_default(vault)?.search(&query)?;
    to_pretty(&response)
}

fn get_document(vault: &Path, args: &Value) -> Result<String> {
    let id = canonical("id", &parse_args::<IdArgs>("get_document", args)?.id)?;
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
    // Deserialized straight into the core type, the way `search` already
    // consumes `SearchQuery`. The hand-assembly this replaces had to be kept in
    // step with `DocumentQuery` by hand, and had already fallen out of step.
    let query: kataan_core::query::DocumentQuery = parse_args("documents", args)?;
    let loaded = LoadedVault::load(vault)?;
    to_pretty(&kataan_core::query::documents(&loaded, &query)?)
}

fn list_folders(vault: &Path) -> Result<String> {
    let vault = Vault::open(vault)?;
    to_pretty(&json!({ "type_folders": vault.index.type_folders }))
}

fn get_folder(vault: &Path, args: &Value) -> Result<String> {
    let id = canonical("id", &parse_args::<IdArgs>("get_folder", args)?.id)?;
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
    let path = parse_args::<PathArgs>("resolve_path", args)?.path;
    let loaded = LoadedVault::load(vault)?;
    let id = loaded
        .resolve_path(&path)
        .ok_or_else(|| anyhow!("`{path}` does not resolve to a document in this vault"))?;
    // The same projection HTTP returns. These two answered the same question
    // differently until the shape moved into core: an agent got `id` and
    // `is_folder_index`, a browser also got `folder` and `type_folder`.
    to_pretty(&loaded.resolved(id))
}

fn schema(vault: &Path, args: &Value) -> Result<String> {
    let kind = parse_args::<SchemaArgs>("schema", args)?.kind;
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
    let request: NeighborsArgs = parse_args("neighbors", args)?;
    let id = canonical("id", &request.id)?;
    let loaded = LoadedVault::load(vault)?;
    let result = kataan_core::query::neighbors(
        &loaded,
        &id,
        request.predicate.as_deref(),
        request.direction,
    )?;
    to_pretty(&result)
}

/// Tighter than [`kataan_core::query::MAX_SUBGRAPH_NODES`] on purpose. The
/// whole snuffbox vault exports as 238 KB of JSON — roughly 60k tokens, spent
/// before the agent has read a word of it. An agent that genuinely wants the
/// whole graph can say so with `limit`; one that reached for `subgraph` when it
/// meant `neighbors` gets told, cheaply.
const DEFAULT_SUBGRAPH_NODES: usize = 200;

fn subgraph(vault: &Path, args: &Value) -> Result<String> {
    let request: SubgraphArgs = parse_args("subgraph", args)?;
    let loaded = LoadedVault::load(vault)?;
    let graph = kataan_core::query::subgraph(
        &loaded,
        &request.types,
        &request.predicates,
        Some(request.limit.unwrap_or(DEFAULT_SUBGRAPH_NODES)),
    )?;
    to_pretty(&graph)
}

fn create_document(vault: &Path, args: &Value) -> Result<String> {
    let mut request = parse_args::<CreateArgs>("create_document", args)?.document;
    // Writes over MCP are always attributed to the agent actor, whatever the
    // caller said.
    request.actor = None;
    let id = mutate::create_document(vault, request)?;
    refresh_search_after_write(vault, &id);
    to_pretty(&json!({ "id": id.as_str() }))
}

fn update_document(vault: &Path, args: &Value) -> Result<String> {
    let request: UpdateArgs = parse_args("update_document", args)?;
    let id = canonical("id", &request.id)?;
    let mut patch = request.edit.patch;
    // Writes over MCP are always attributed to the agent actor.
    patch.actor = None;
    mutate::update_document(vault, &id, request.edit.body, patch)?;
    refresh_search_after_write(vault, &id);
    to_pretty(&json!({ "id": id.as_str(), "updated": true }))
}

fn remove_edge(vault: &Path, args: &Value) -> Result<String> {
    let request: EdgeArgs = parse_args("remove_edge", args)?;
    let (source, target) = (
        canonical("source", &request.source)?,
        canonical("target", &request.target)?,
    );
    let predicate = request.predicate;
    mutate::remove_edge(vault, &source, &predicate, &target)?;
    refresh_search_after_write(vault, &source);
    to_pretty(
        &json!({ "source": source.as_str(), "predicate": predicate, "target": target.as_str(), "removed": true }),
    )
}

fn replace_edges_for_predicate(vault: &Path, args: &Value) -> Result<String> {
    let request: ReplaceEdgesArgs = parse_args("replace_edges_for_predicate", args)?;
    let source = canonical("source", &request.source)?;
    let predicate = request.predicate;
    let targets = request
        .targets
        .iter()
        .map(|target| canonical("targets", target))
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
    let request: EdgeArgs = parse_args("add_edge", args)?;
    let (source, target) = (
        canonical("source", &request.source)?,
        canonical("target", &request.target)?,
    );
    let predicate = request.predicate;
    mutate::add_edge(vault, &source, &predicate, &target)?;
    refresh_search_after_write(vault, &source);
    to_pretty(
        &json!({ "source": source.as_str(), "predicate": predicate, "target": target.as_str() }),
    )
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
    fn a_wrong_typed_argument_is_refused_rather_than_defaulted() {
        // The failure this replaced: every tool picked its arguments out of a
        // `Value` field by field, and a picker has to invent something when the
        // value is not the type it wanted. `str_vec` returned an empty list for
        // anything that was not an array, so this call created the document,
        // dropped the aliases, and reported success — a write silently
        // discarding what the caller asked for.
        let dir = temp_vault();
        let vault = dir.path();

        let refused = call(
            vault,
            "create_document",
            &json!({
                "type": "note",
                "title": "Alpha",
                "body": "a",
                "aliases": "A,a1"
            }),
        )
        .expect_err("a string is not a list of aliases");

        // The message has to name what was wrong, or an agent cannot correct
        // itself: `to_string()` on the error chain would say only "invalid
        // arguments for `create_document`".
        let message = format!("{refused:#}");
        assert!(message.contains("create_document"), "{message}");
        assert!(message.contains("expected a sequence"), "{message}");

        // And nothing was written.
        assert!(
            !vault.join("notes/alpha.toml").exists(),
            "document created anyway"
        );
    }

    #[test]
    fn a_defaulted_argument_cannot_be_silently_wrong() {
        // Same class, read side: `limit` went through `as_u64().unwrap_or(200)`,
        // so a string limit silently became the default ceiling and the caller
        // was answered as though it had asked for that.
        let dir = temp_vault();
        let message = format!(
            "{:#}",
            call(dir.path(), "subgraph", &json!({ "limit": "500" }))
                .expect_err("a string is not a node count")
        );
        assert!(message.contains("expected usize"), "{message}");
    }

    #[test]
    fn resolve_path_answers_an_agent_what_it_answers_a_browser() {
        // These two surfaces returned different shapes for the same question:
        // MCP gave `id` and `is_folder_index`, HTTP also gave `folder` and
        // `type_folder`, because each built its own projection. Both now
        // serialize `LoadedVault::resolved`, and this asserts the tool emits
        // that whole shape rather than a subset of it.
        let dir = temp_vault();
        let vault = dir.path();

        let resolved = json_result(vault, "resolve_path", json!({ "path": "type/note.md" }));
        let expected = serde_json::to_value(
            LoadedVault::load(vault)
                .unwrap()
                .resolved(&kataan_core::id::CanonicalId::parse("type/note").unwrap()),
        )
        .unwrap();

        assert_eq!(resolved, expected);
        // Named individually so a field silently dropped from the projection
        // fails here rather than passing because both sides lost it.
        for field in ["id", "folder", "type_folder", "is_folder_index"] {
            assert!(
                resolved.get(field).is_some(),
                "`{field}` missing: {resolved}"
            );
        }
        assert_eq!(resolved["type_folder"], "type");
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
