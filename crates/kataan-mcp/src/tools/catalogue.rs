//! What the tools are, as MCP sees them: names, descriptions and input schemas.
//!
//! Separate from the implementations because this is the part an agent
//! actually reads. A description here is not documentation — it is the only
//! thing steering a model towards `neighbors` instead of a whole-vault
//! `subgraph`, so it is worth editing as carefully as the code it describes.

use schemars::schema_for;
use serde_json::{json, Value};

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
            // Generated from `DocumentQuery` rather than written out here. The
            // hand-written copy carried thirteen property descriptions that had
            // to track the struct by hand, and had already drifted from it in
            // shape — it described `linked_to` as flat while the type nested it.
            schema_for::<kataan_core::query::DocumentQuery>(),
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
            "Export nodes and links for the whole vault in one call. Each edge appears once, in the direction it was authored. Expensive in context — a mid-sized vault is tens of thousands of tokens — so it refuses above 200 nodes rather than flooding you: filter by types/predicates, raise `limit` if you truly want the lot, and prefer `neighbors` when you only need one document's connections.",
            json!({
                "type": "object",
                "properties": {
                    "types": { "type": "array", "items": { "type": "string" }, "description": "Restrict to these document types; omit for all." },
                    "predicates": { "type": "array", "items": { "type": "string" }, "description": "Restrict to these edge predicates; omit for all." },
                    "limit": { "type": "integer", "description": "Refuse rather than return more than this many nodes. Defaults to 200; the maximum is 5000." }
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
                    "labels": { "type": "array", "items": { "type": "string" } },
                    "fields": { "type": "object", "description": "Custom sidecar keys to set. A `null` value removes the key; keys you do not mention are left alone. Reserved keys kataan defines itself are refused — they have their own arguments." },
                    "expected_updated_at": { "type": "string", "description": "The `updated_at` you last read for this document. When given, the write is refused if the document has changed since — pass it whenever you read, edit, and write back, so you cannot silently discard someone else's change." }
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

/// A tool's `inputSchema`, taken from the type the tool deserializes.
///
/// One definition instead of two: the doc comments on the struct become the
/// descriptions an agent reads, so a field cannot be added without one.
fn schema_for<T: schemars::JsonSchema>() -> Value {
    serde_json::to_value(schema_for!(T)).expect("schema serializes")
}
