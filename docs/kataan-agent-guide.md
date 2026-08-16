# Kataan CLI guide for agents

Kataan is a filesystem-native Markdown/TOML knowledge workspace. The filesystem is the source of truth: Markdown stores human-readable content, and TOML sidecars store metadata, relationships, and checksums.

## Core commands

```sh
kataan init <vault-path> --name "My Knowledgebase"
kataan validate <vault-path>
kataan rebuild-indexes <vault-path>
kataan guide
```

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
GET  /api/schema/document
GET  /api/schema/folder-index
GET  /api/schema/vault
GET  /api/schema/type-definition
GET  /api/schema/ontology
GET  /api/schema/edge-predicate
```

File previews are size-limited: `/api/file` and `/api/file/highlight` serve text content up to 10 MB, and `/api/file/raw` serves binary content (SVG, PDF) up to 50 MB. Larger files return an error.

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

The current HTTP API is primarily read/repair oriented. Create and update document files directly on disk or through a reviewed application workflow, then call `POST /api/rebuild-indexes` and `POST /api/validate`.

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
└── code/                # artifacts/tools; usually not indexed as knowledge
```

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
and manage the ignore list yourself.

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

## Agent safety rules

- Prefer proposals and small diffs over large rewrites.
- Do not overwrite human content blindly.
- Treat TOML relationships and `ontology.toml` as authoritative.
- Use `belongs_to` only for explicit containment relationships if present in the ontology; path ancestry is already derived from canonical IDs.
- Treat `related_to` as an undirected/symmetric relationship when querying.
- Keep raw intake/source material instead of replacing it with summaries.
- Ask for clarification when the destination type, folder, or relationship is uncertain.
