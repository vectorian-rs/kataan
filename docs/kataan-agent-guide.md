# Kataan CLI guide for agents

Kataan is a filesystem-native Markdown/TOML knowledge workspace. The filesystem is the source of truth: Markdown stores human-readable content, and TOML sidecars store metadata, relationships, and checksums.

## Core commands

```sh
kataan init <vault-path> --name "My Knowledgebase"
kataan validate <vault-path>            # add --json for a machine-readable report
kataan rebuild-indexes <vault-path>
kataan guide

kataan documents <vault-path> [--type T] [--status S] [--label L] [--id ID]
                              [--path-prefix P] [--linked-to ID] [--predicate P]
                              [--markdown] [--limit N] [--offset N]
kataan graph export <vault-path> [--type T] [--predicate P]
kataan graph neighbors <vault-path> <id> [--predicate P] [--direction out|in|both]
```

`documents` and `graph` print JSON on stdout. `graph export` is deterministic, so
its output diffs cleanly across runs and can be committed as a build artifact —
which is the intended way to rebuild a graph file from inside the vault repo,
without running a server.

Command results go to stdout (`validate` prints `valid` or the diagnostics, or a
`{ "ok", "diagnostics": [...] }` object with `--json`); logs and confirmations go
to stderr; the exit code signals success (`validate` exits non-zero when invalid).

When running from this repository without an installed binary, use:

```sh
cargo run -p kataan-cli -- <command>
```

## HTTP API

Start the API server:

```sh
kataan-server --vault <vault-path> --bind 127.0.0.1:3001
```

When running from this repository without an installed binary, use:

```sh
cargo run -p kataan-server -- --vault <vault-path> --bind 127.0.0.1:3001
```

Useful agent/read endpoints:

```txt
GET  /api/health
GET  /api/watch
GET  /api/vault
GET  /api/folders
GET  /api/folders/:folder
GET  /api/folder?id=<canonical-folder-id>
GET  /api/document?id=<canonical-document-id>
GET  /api/documents/<canonical-document-id>
GET  /api/file?path=<vault-relative-file-path>
GET  /api/file/highlight?path=<vault-relative-file-path>&theme=<theme>
GET  /api/file/raw?path=<vault-relative-file-path>
GET  /api/resolve?type=<type-folder>&token=<route-token>
GET  /api/resolve-path?path=<vault-relative-or-absolute-path>
GET  /api/documents?type=&status=&labels=&ids=&path_prefix=&linked_to=&predicate=&direction=&include=&limit=&offset=
GET  /api/graph/neighbors?id=<canonical-id>&predicate=<predicate>&direction=out|in|both
GET  /api/graph/subgraph?types=<comma-separated>&predicates=<comma-separated>
GET  /api/schema/document
GET  /api/schema/folder-index
GET  /api/schema/vault
GET  /api/schema/type-definition
GET  /api/schema/ontology
GET  /api/schema/edge-predicate
```

File previews are size-limited: `/api/file` and `/api/file/highlight` serve text content up to 10 MB, and `/api/file/raw` serves binary content (SVG, PDF) up to 50 MB. Larger files return an error.

Keyword full-text search:

```txt
GET  /api/search?q=<query>&kind=&type=&status=&facet=&path_prefix=&limit=&offset=
GET  /api/search/status
POST /api/search/reindex
```

The search index is built on demand — `POST /api/search/reindex` (re)builds it from the current vault. It is not updated automatically on edits, so reindex after changing content.

Useful repair endpoints:

```txt
POST /api/validate
POST /api/rebuild-indexes
```

Examples:

```sh
curl http://127.0.0.1:3001/api/health
curl http://127.0.0.1:3001/api/vault
curl http://127.0.0.1:3001/api/folders
curl 'http://127.0.0.1:3001/api/document?id=notes/example-note'
curl -X POST http://127.0.0.1:3001/api/rebuild-indexes
curl -X POST http://127.0.0.1:3001/api/validate
```

The HTTP API is read/repair oriented — it has no write endpoints. To create or
change content you have two options: edit the Markdown/TOML pairs directly on
disk and then call `POST /api/rebuild-indexes` and `POST /api/validate`, or use
the MCP server below, which validates each change for you.

## MCP server (read + write)

`kataan-mcp` exposes the vault to MCP clients (Claude Desktop, IDE agents) as
typed tools over stdio, and is the recommended way for an agent to **write** the
vault: every mutation goes through the validated mutation layer, so the result is
always well-formed (correct id, sidecar, checksums, folder indexes, and
ontology-legal edges), and the search index is refreshed after each write.

```sh
kataan-mcp --vault <vault-path>
# or, from this repository:
cargo run -p kataan-mcp -- --vault <vault-path>
```

Tools:

- Reads — `search`, `get_document`, `documents`, `list_folders`, `get_folder`,
  `resolve`, `resolve_path`, `neighbors`, `subgraph`, `schema`, `vault_info`
  (return JSON).
- Writes — `create_document` (type, title, body, optional parent/aliases/labels/
  status/occurred_at/fields), `update_document` (id, optional body/status/
  aliases/labels/occurred_at), and `add_edge` (source, predicate, target).
  Illegal requests (unknown type, id collision, ontology-forbidden edge, invalid
  status, malformed timestamp) are rejected rather than written. Writes are
  attributed to the `agent` actor.

Choosing a read tool:

- One document by id — `get_document`.
- Many documents, or every document of a type — `documents`. It takes `ids` for a
  batch fetch and filters (`type`, `status`, `labels`, `path_prefix`,
  `linked_to`) for a listing. Returns metadata only unless you ask for
  `include: "markdown"`, because each body is a separate file read. Matching more
  than `limit` is an error rather than a silent truncation, so a partial result
  can never be mistaken for a complete one.
- What one document is connected to — `neighbors`. **This is the only way to see
  incoming edges.** `get_document` returns the raw `edges` table, which is
  outgoing-only, so "who works at this organization" is unanswerable from it: the
  `works_at` edge is declared on each person, and the inverse exists only in the
  graph. `neighbors` returns both directions, grouped by predicate, with each
  neighbour's type and title already filled in.
- A whole graph — `subgraph`. Each edge appears once, in the direction it was
  authored. It can be large; filter by `types`/`predicates`.
- A filesystem path rather than an id — `resolve_path`, then `get_document`.

Because the mutation tools rebuild indexes and reindex search themselves, you do
**not** need to call `rebuild-indexes`/`validate` after an MCP write — that is
only needed after editing files directly on disk.

## Vault structure

A new vault contains:

```txt
vault/
├── kataan.toml          # root config and type-to-folder mapping
├── ontology.toml        # relationship predicates
├── intake/              # raw/source material
├── projects/            # project documents
├── people/              # person documents
├── notes/               # note documents
├── topics/              # topic documents
├── type/                # type definitions
└── code/                # file-backed folder: raw code/tool files, no index.toml
```

`code/` is a declared type folder that holds raw files instead of documents (it
has no `index.toml`). The API serves any such index-less type folder as a
"file-backed folder" — its subdirs and files are browsable via `/api/folder` and
`/api/file`, but its contents are not indexed as knowledge documents.

Read `kataan.toml` before creating files. Its `[type_folders]` table is authoritative: a document with `type = "project"` belongs under the mapped project folder, a document with `type = "topic"` belongs under the mapped topic folder, and so on.

## Ignored paths

Scans (`validate`, `rebuild-indexes`, document loading) prune build and vendor
directories so they never produce diagnostics or land in generated indexes or
`folder_checksum` values. Default pruned directory names, matched at any depth:
`node_modules`, `.git`, `.svn`, `target`, `dist`, `build`, `.astro`, `.next`,
`.cache`, `.venv`, `venv`, `__pycache__`, `.DS_Store`.

**These names are reserved: a knowledge folder must not be named `node_modules`,
`target`, `dist`, `build`, `venv`, `.venv`, `.cache`, `.next`, `.astro`, or any
other default-pruned name.** A folder matching one is skipped entirely and
silently: its documents never appear in an index and never validate. If you need
a knowledge node about one of these topics, name the folder something else (for
example `build-systems` instead of `build`), or set `use_default_ignores = false`
and manage the ignore list yourself. On case-insensitive filesystems (macOS,
Windows) matching is case-insensitive, so `Target` and `NODE_MODULES` are
reserved too.

Extend the defaults per vault with gitignore-style patterns, resolved relative
to the vault root. Patterns must match the directory to prune it (a trailing
`/**` matches only the contents, not the directory itself):

```toml
# kataan.toml
[scan]
ignore = ["vendor/", "**/*.tmp", "some/specific/dir"]
# use_default_ignores = false   # opt out of the built-in defaults entirely
```

A `.kataanignore` file at the vault root (gitignore syntax) is merged in as
well. Vaults without a `[scan]` section keep using the defaults.

## Documents

A Kataan document is a Markdown file plus a matching TOML sidecar in the same folder:

```txt
notes/example-note.md
notes/example-note.toml
```

The canonical document ID is the vault-relative path without extension:

```txt
notes/example-note
```

Use canonical IDs in relationships. Do not use bare slugs when referring to other documents.

Minimal Markdown:

```md
# Example Note

Human-readable content goes here.
```

Minimal TOML sidecar:

```toml
type = "note"
status = "draft"
markdown = "example-note.md"
labels = ["example"]
created_by = "agent"
last_updated_by = "agent"
occurred_at = "2026-08-29"

[edges]
related_to = ["topics/example-topic"]
derived_from = ["intake/source-material"]
```

Notes:

- `type` must exist in `kataan.toml` and `type/<type>.toml`.
- `markdown` must point to the paired Markdown filename.
- `status`, when used, should be `draft`, `active`, `paused`, `done`, or `archived`.
- `created_by` and `last_updated_by` should be `human`, `agent`, or `system`.
- Do not hand-compute `markdown_checksum`; run `kataan rebuild-indexes`.
- Any other top-level key is yours to use. Kataan preserves keys it does not
  model and returns them to consumers; it does not invent or delete them.

### Time

Three optional time fields are validated:

- `occurred_at` — when the thing the document describes happened. **Yours to
  set.**
- `created_at`, `updated_at` — when the record was written and last changed.
  Stamped automatically by the mutation layer; do not hand-write them.

**Never widen precision.** Record only what you know:

| You know | Write |
| --- | --- |
| the year | `2006` |
| the month | `2006-05` |
| the day | `2006-05-18` |
| the moment | `2026-08-29T12:00:00Z` |

Writing `2006-01-01` for a source that said `2006` asserts a day nobody knows,
so kataan keeps each value exactly as written. Bare Unix epochs, datetimes
without a timezone, and impossible dates like `2026-02-30` are rejected.

## Folder knowledge nodes

A folder becomes a knowledgebase folder node when it contains both:

```txt
some-folder/index.md
some-folder/index.toml
```

Minimal folder index Markdown:

```md
# Folder Name

Short description of this area.
```

Minimal folder index TOML:

```toml
type = "project"
markdown = "index.md"
name = "Folder Name"
default_type = "project"
```

`index.toml` is system-managed after creation. `rebuild-indexes` fills in `folder_checksum`, `[[documents]]`, and `[[subfolders]]` entries.

If you create document pairs inside a folder and skip the folder index, `kataan rebuild-indexes` can create the missing folder index pair for knowledge folders. Pure artifact-only folders are left alone.

## Creating or updating content as an agent

Recommended workflow:

1. Inspect `kataan.toml`, `ontology.toml`, relevant `index.toml` files, and existing sidecars before reading large Markdown bodies.
2. Choose the correct type folder from `[type_folders]`.
3. Create or edit small Markdown/TOML pairs. Preserve human-authored content and unknown TOML fields when possible.
4. Use canonical IDs for edges, for example `projects/my-project` or `topics/rust`.
5. Record provenance with `derived_from` edges when content comes from intake/source documents.
6. Run:

   ```sh
   kataan rebuild-indexes <vault-path>
   kataan validate <vault-path>
   ```

7. If validation reports errors, repair the files and repeat.

## Re-indexing

Run this after creating, moving, or editing documents:

```sh
kataan rebuild-indexes <vault-path>
```

It updates:

- document `markdown_checksum` fields
- folder `[[documents]]` entries
- folder `[[subfolders]]` entries
- recursive `folder_checksum` fields
- missing folder index pairs for folders that have become knowledge folders
- root `updated_at`

Then verify:

```sh
kataan validate <vault-path>
```

`rebuild-indexes` fixes checksum and index drift. It does not fix invalid types, unresolved edge targets, unknown ontology predicates, or folder-depth violations.

## Custom types

To add a custom type such as `article`:

1. Add a mapping to `kataan.toml`:

   ```toml
   [type_folders]
   article = "articles"
   ```

2. Create `type/article.md`.
3. Create `type/article.toml`:

   ```toml
   type = "type-definition"
   name = "article"
   folder = "articles"
   icon = "Newspaper"
   markdown = "article.md"
   created_by = "agent"
   last_updated_by = "agent"
   ```

4. Create the `articles/` folder.
5. Run `kataan rebuild-indexes <vault-path>` and `kataan validate <vault-path>`.

## Field schemas

`ontology.toml` can describe what documents of a type carry, alongside the
`[edges.*]` definitions it already holds:

```toml
[nodes.person]
required = ["name"]

[nodes.person.fields]
name       = { type = "string" }
emails     = { type = "array", items = "string" }
born       = { type = "date" }
employment = { type = "array", items = "interval" }
mentor     = { type = "reference", to = ["person"] }
```

Types: `string`, `integer`, `number`, `boolean`, `date`, `instant`, `interval`,
`reference`, `array`, `table`.

- `date` accepts any precision; `instant` requires a full timestamp.
- `interval` is a table with `from` and an optional `to`. **Leaving `to` out is
  legal** — an open interval means "still true", not missing data.
- `reference` is another document's canonical id, optionally restricted by `to`.

Two rules worth remembering:

- **Schemas constrain what they declare, never what they do not.** An undeclared
  key still validates. A type with no schema is entirely unconstrained, so a
  vault can adopt schemas one type at a time.
- **Only top-level keys can be constrained.** A value nested inside a table
  (`[rate_card]` → `effective_date`) can be required and checked as a `table`,
  but its interior is not reachable.

Schemas live in the vault, not in kataan, so they version in the same git
timeline as the documents they describe.

## Agent safety rules

- Prefer proposals and small diffs over large rewrites.
- Do not overwrite human content blindly.
- Treat TOML relationships and `ontology.toml` as authoritative.
- Use `belongs_to` only for explicit containment relationships if present in the ontology; path ancestry is already derived from canonical IDs.
- Treat `related_to` as an undirected/symmetric relationship when querying.
- Keep raw intake/source material instead of replacing it with summaries.
- Ask for clarification when the destination type, folder, or relationship is uncertain.
