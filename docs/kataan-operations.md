# Operating a kataan vault

How agents and humans work with a vault, and how the server behaves. Split
out of `kataan-brief.md` for length; `kataan-format.md` covers the on-disk
contract.

## Agent access

Kataan is designed to be operated by agents, not just read by them. Agents
interact with a vault two ways:

- **MCP server (`kataan-mcp`).** The primary write path. It exposes the vault to
  MCP clients (Claude Desktop, IDE agents) as typed tools over stdio, backed by
  the validated mutation layer (`kataan_core::mutate`): `create_document`,
  `update_document`, and the edge writes produce guaranteed-well-formed changes, and
  ontology-illegal requests are rejected rather than written.
- **Editing files + repair.** Agents can also edit the Markdown/TOML pairs
  directly and then run `rebuild-indexes` + `validate` (or the equivalent server
  endpoints).

Either way, the guidance is the same: use the smallest useful context (vault and
folder indexes first, metadata and graph summaries next, full Markdown only when
needed), make small non-destructive changes, and preserve human-authored content
and raw intake. Removing a document from active views is a `status = "archived"`
change; deleting files is a human decision. `docs/kataan-agent-guide.md` (shipped
in the CLI via `kataan guide`) is the operational reference.

## Human and agent collaboration

Kataan is not an AI-only wiki.

Humans can:

- create files manually
- edit Markdown directly
- edit TOML metadata
- reorganize folders
- review and revert agent changes
- write original notes

Agents can:

- classify intake input
- create new files
- update existing files
- summarize long content
- extract people, projects, topics, and relationships
- restructure messy notes
- find duplicates
- suggest missing links
- run cleanup checks

The agent is a collaborator, not the owner.

Agent changes should be diff-based and non-destructive. Agents should not silently overwrite human edits: re-read a document before editing it, and preserve unknown TOML fields and human-authored Markdown.

## Concurrency and writes

Kataan uses a single-writer model in the server. Every API write takes a
process-wide write lock for the duration of the mutation, so two concurrent
requests cannot interleave a read of the old sidecar with a write of the new
one, and their index rebuilds cannot cross. The lock is held on the blocking
pool rather than an async worker, so waiting writers do not occupy the runtime.

A write refreshes the `Arc<RwLock<LoadedVault>>` metadata index and the search
index before returning, so a caller that writes and immediately reads sees its
own write. Reads take a short read lock and should avoid holding it while
reading large Markdown bodies.

MCP never needed the lock — it is one process over stdio, so its writes were
already serial. The HTTP surface is concurrent by construction.

`LoadedVault` is metadata-only: it stores config, ontology, type registry, document records, labels/facets, graph, checksums, diagnostics, and paths. It does not keep full Markdown bodies in memory. Markdown is read on demand from the file path in the document record.

Every Markdown and TOML write is atomic: write a temporary file in the same directory, fsync, then rename/persist. Rebuild operations are per-folder atomic so a crash mid-rebuild does not corrupt indexes.

External file changes are detected with filesystem notifications where available. Watcher events are debounced and batched. Kataan applies the smallest safe metadata update for clear changes, such as reparsing one TOML file or recomputing one Markdown checksum. Ambiguous structural changes fall back to reloading the whole `LoadedVault`.

There is no cross-process file lock in v1. Mutating CLI commands should be avoided while the server is running unless the watcher can observe and reconcile the changes.

## Boot and API modes

Server boot:

1. Load `vault/kataan.toml` and limits.
2. Walk the vault and compute BLAKE3 checksums on all files.
3. Detect drift vs. stored checksums in folder indexes.
4. Validate structure, references, depth, and type-folder mapping.
5. If errors exist, serve read-only API plus diagnostics and expose rebuild.
6. If clean, enable the full API.

The HTTP API is read/write. Documents and edges can be created, updated and
linked over HTTP as well as MCP:

| Route | Does |
| --- | --- |
| `POST /api/documents` | Create a document. Returns `201` and its canonical id. |
| `PATCH /api/documents/*id` | Update body and/or metadata. Omitted fields are left alone. |
| `POST /api/edges` | Add `source --predicate--> target`. |
| `DELETE /api/edges?source=&predicate=&target=` | Remove one edge. |
| `PUT /api/edges` | Replace the whole target list for one predicate. |

All five reuse `kataan_core::mutate`, exactly as the MCP tools do, so the two
surfaces cannot accept different things. All five are refused for a cross-site
request, and a write the vault's rules reject — an unknown type, a malformed
timestamp, a field violating the type's `[nodes.*]` schema — is a `400`, not a
`500`.

Read `GET /api/schema/<type>` or `GET /api/ontology` first: the write boundary
enforces node schemas, so those describe what a write must contain.

The server checks folder depth on every write and rejects violations with `folder-depth-exceeded`. Edge writes support `add_edge`, `remove_edge`, and `replace_edges_for_predicate`.

`add_edge` validates against `ontology.toml` before commit, as does every target written by `replace_edges_for_predicate`. `remove_edge` deliberately does not: an edge worth removing is often one the ontology has since come to forbid, or whose target no longer exists, and requiring it to be legal before it could be deleted would make exactly the states that need repairing the ones that cannot be repaired. Removing an edge that is not there succeeds and changes nothing.

## MCP surface

The `kataan-mcp` crate is a Model Context Protocol server speaking JSON-RPC over
stdio (no SDK dependency). It is **read + write**:

- Reads: `search`, `get_document`, `documents`, `list_folders`, `get_folder`,
  `resolve`, `resolve_path`, `neighbors`, `subgraph`, `schema`, `vault_info` —
  returning JSON.
- Model discovery: `schema` (per kataan kind *or* per vault document type) and
  `ontology` (types, predicates, and the type-level graph) on HTTP, MCP and CLI.
- Writes: `create_document`, `update_document`, `add_edge`, `remove_edge`,
  `replace_edges_for_predicate` — routed through the
  validated mutation layer, with the search index refreshed after each write.

Graph and bulk reads are shared with the HTTP API and the CLI over one
implementation in `kataan_core::query`, so all three answer identically.
`neighbors` is the only way to read incoming edges: `get_document` returns the
raw `edges` table, which is outgoing-only, so the inverse and symmetric
predicates declared in `ontology.toml` are invisible to it.

Tool failures (unknown type, id collision, ontology-illegal edge, invalid status)
surface as MCP `isError` results rather than corrupting the vault. Reads return
JSON, never HTML — Markdown rendering lives only in `kataan-server`.

## Intake vs organized knowledge

Intake content should be preserved before transformation.

```txt
intake/        original source material
notes/      curated notes
people/     people profiles
projects/   projects and efforts
topics/     durable concepts and themes
code/       agent tools and executable helper code
```

The `code/` folder is intended for agent/tool code such as MCP adapters, TypeScript scripts, Python helpers, schemas, and executable utilities. Kataan treats it like any other folder under the pair-based model: files are artifacts by default, and only Markdown/TOML pairs become knowledgebase elements.

An intake document may later produce many organized files.

Example:

```txt
intake/karpathy-llm-knowledge-bases-article.md
        ↓
people/andrej-karpathy.md
topics/knowledge-bases.md
topics/ai-agents.md
notes/ai-compiled-knowledge-bases.md
```

## TOML schemas and repair guidance

Kataan exposes machine-readable schemas for its TOML data models so the UI, repair tools, and agents can guide users without duplicating Rust struct definitions by hand.

Schemas are derived from the Rust data structs where possible, for example `DocumentMetadata`, `FolderIndex`, `VaultConfig`, `TypeDefinition`, `Ontology`, and `EdgePredicate`. The API exposes JSON Schema plus TOML-oriented templates and vault-aware constraints.

Example endpoints:

```txt
GET /api/schema/document
GET /api/schema/folder-index
GET /api/schema/vault
GET /api/schema/type-definition
GET /api/schema/ontology
```

Example response shape:

```json
{
  "kind": "document",
  "schema": { "type": "object" },
  "constraints": {
    "allowed_status": ["draft", "active", "paused", "done", "archived"],
    "allowed_actors": ["human", "agent", "system"],
    "allowed_types": ["project"],
    "allowed_edge_predicates": ["related_to", "derived_from"]
  },
  "toml_template": "type = \"project\"\nmarkdown = \"example.md\"\n"
}
```

JSON Schema describes the structural data model: required fields, optional fields, arrays, maps, and nested tables. Kataan-specific rules that depend on the current vault are returned as separate constraints, such as allowed types, allowed ontology predicates, folder/type mapping, valid status values, actor values, and pair-based folder/document detection.

Repair UI should use this endpoint to show the minimum valid TOML shape for a broken file and to constrain LLM repair proposals. The LLM may suggest a patch, but human review and base-content-hash checks are required before applying it.

## Diagnostics

Validation and repair commands should emit structured diagnostics with a severity, machine-readable code, message, and optional path.

Severities:

- `error`: must be fixed for the vault to be considered valid.
- `warning`: should be reviewed, but does not make the vault invalid.
- `info`: informational note or successful repair detail.

Example diagnostic:

```toml
severity = "error"
code = "invalid-toml"
path = "projects/kataan-redesign.toml"
message = "TOML metadata is invalid for a Markdown+TOML document pair."
```

Diagnostic codes use lowercase kebab-case and should be stable enough for tools and UI filters.

Edge and ontology diagnostic codes include:

- `unknown-predicate`
- `predicate-source-type-mismatch`
- `predicate-target-type-mismatch`
- `unresolved-edge-target`
- `invalid-ontology-entry`
- `missing-ontology`

## Initial technical direction

Kataan should start small:

- Rust backend
- Astro frontend
- local vault folder
- Markdown reader/writer
- TOML metadata reader/writer
- validated mutation layer for agent writes
- full-text keyword search
- intake input box
- simple file browser
- MCP read/write server
- no complex database unless proven necessary

## Guiding principle

Intake input is preserved. Organized knowledge is curated. Humans and agents both participate. The filesystem remains the source of truth.
