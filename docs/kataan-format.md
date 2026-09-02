# Kataan vault format

The on-disk contract: how a vault is laid out, what a document is, and the
rules a valid vault obeys. Split out of `kataan-brief.md` for length; the
brief now covers how the vault is operated.

## Core idea

Kataan treats a folder as the source of truth.

```txt
intake information → proposed structure → human review → organized knowledge
```

The system should be understandable without a database or proprietary format:

- Markdown files store readable content.
- TOML files store metadata.
- Folders provide human-friendly organization.
- Agents operate directly on the same files humans use.
- Intake input is preserved before it is transformed.
- The server keeps a metadata-only in-memory vault index; Markdown bodies are read on demand.

## Vault structure

A Kataan vault is a normal directory:

```txt
vault/
├── kataan.toml
├── ontology.toml
├── intake/
│   ├── index.md
│   ├── index.toml
│   ├── pasted-chat-about-ai-kbs.md
│   └── pasted-chat-about-ai-kbs.toml
├── people/
│   ├── index.md
│   ├── index.toml
│   └── company-x/
│       ├── index.md
│       ├── index.toml
│       ├── jane-doe.md
│       └── jane-doe.toml
├── projects/
│   ├── index.md
│   ├── index.toml
│   └── company-x/
│       ├── index.md
│       ├── index.toml
│       ├── q2-launch.md
│       └── q2-launch.toml
├── notes/
│   ├── index.md
│   ├── index.toml
│   ├── ai-compiled-knowledge-bases.md
│   └── ai-compiled-knowledge-bases.toml
├── topics/
│   ├── index.md
│   ├── index.toml
│   ├── knowledge-bases.md
│   └── knowledge-bases.toml
├── code/
│   ├── tools/
│   │   └── classify_intake.py
│   └── mcp-adapters/
│       └── README.md
└── type/
    ├── index.md
    ├── index.toml
    ├── intake.md
    ├── intake.toml
    ├── project.md
    ├── project.toml
    ├── person.md
    ├── person.toml
    ├── note.md
    ├── note.toml
    ├── topic.md
    ├── topic.toml
    ├── code.md
    ├── code.toml
    ├── type-definition.md
    └── type-definition.toml
```

Kataan knowledgebase elements are detected by Markdown/TOML pairs, not by folder name alone. A regular document is `name.md` + `name.toml`. A folder knowledgebase node is `index.md` + `index.toml`. Folders without an index pair are structural artifact folders unless they contain document pairs or indexed child folders, in which case `rebuild-indexes` may create the missing folder index pair. `code/` is therefore usually just an artifact tree, but it no longer needs a separate document-model exception.

## Vault index

The vault root contains a `kataan.toml` file with vault-level metadata and schema information.

Example `vault/kataan.toml`:

```toml
schema_version = "0.1.0"
name = "My Kataan Vault"
created_at = "2026-04-28T12:00:00Z"
updated_at = "2026-04-28T12:30:00Z"

[limits]
max_folder_depth = 4

[type_folders]
intake = "intake"
project = "projects"
person = "people"
note = "notes"
topic = "topics"
type-definition = "type"
code = "code"

# User-defined document types are first-class.
article = "articles"
presentation = "presentations"
reference = "references"
finance = "finances"
task = "tasks"
```

`schema_version` is required and identifies the vault format version. The `type_folders` table defines the authoritative type-to-folder mapping for the vault. Kataan must not hard-code the document type universe. `[limits].max_folder_depth` defaults to `4` and counts segments after the type folder, so `projects/a/b/c/foo` has depth `4`.

## File model

A first-class Kataan document exists only when a Markdown file and a matching TOML sidecar exist in the same folder with the exact same basename:

```txt
my-amazing-project.md
my-amazing-project.toml
```

This pair forms one vault node with canonical ID `.../my-amazing-project`. The TOML file is metadata and is not shown as a separate file in the UI.

The Markdown file contains the human-readable content:

```md
# My Amazing Project

This project is about building a better knowledge workspace.
```

The TOML file contains structured metadata:

```toml
type = "project"
status = "active"

markdown = "my-amazing-project.md"
markdown_checksum = "blake3:..."

aliases = ["Amazing Project", "MAP"]
labels = ["rust", "local-first", "knowledge-workspace"]

[edges]
related_to = ["topics/knowledge-bases", "topics/ai-agents"]
derived_from = ["intake/pasted-chat-about-ai-kbs"]

created_by = "human"
last_updated_by = "agent"
```

Anything that is not a valid Markdown+TOML pair is a regular file/artifact, not a vault document. This includes JSON, spreadsheets, images, PDFs, text files, standalone TOML files, and standalone Markdown files without a sidecar.

Artifacts live next to documents when practical:

```txt
projects/
├── kataan-redesign.md
├── kataan-redesign.toml
├── kataan-redesign-sketch.png
├── kataan-redesign-brief.pdf
├── scoping-structured.json
└── allocation-matrix.xlsx
```

In folder views, documents and files are displayed separately:

```txt
Documents
---------
Kataan Redesign
Scoping Call

Files
-----
scoping-structured.json
allocation-matrix.xlsx
kataan-redesign-sketch.png
```

Matching TOML sidecars are hidden from the Files section. A standalone TOML file without a matching Markdown file is shown as a regular file/artifact.

Clicking a document opens the Markdown document with TOML metadata/properties. Clicking a file opens a file preview when supported. Raw file access and highlighted preview are separate API concerns:

```txt
GET /api/file?path=...
GET /api/file/highlight?path=...
```

`/api/file` returns source content and metadata. `/api/file/highlight` returns sanitized syntax-highlighted HTML for UI rendering when the file type is supported. The UI should prefer highlighted HTML for text-like artifacts such as JSON, TOML, Markdown, Rust, TypeScript, JavaScript, Bash, YAML, and Python, and fall back to raw text, image preview, or file metadata plus external open/download for unsupported or binary formats.

## Folder and type mapping

Kataan uses a 1:1 mapping between types and top-level folders. A document with `type = "project"` lives somewhere under `projects/`, a document with `type = "person"` lives somewhere under `people/`, and so on. Intermediate path segments are containment/search structure only; they do not change the document type. Validation reports any file whose top-level folder does not match its `type`.

Starter mappings created by the default initializer:

| Type              | Folder      |
| ----------------- | ----------- |
| `intake`          | `intake/`   |
| `project`         | `projects/` |
| `person`          | `people/`   |
| `note`            | `notes/`    |
| `topic`           | `topics/`   |
| `type-definition` | `type/`     |
| `code`            | `code/`     |

Example user-defined mappings:

| Type           | Folder           | Purpose                                  |
| -------------- | ---------------- | ---------------------------------------- |
| `article`      | `articles/`      | Original long-form writing               |
| `presentation` | `presentations/` | Talks, decks, and outlines               |
| `reference`    | `references/`    | Curated external articles and resources  |
| `finance`      | `finances/`      | Budgets, invoices, and financial plans   |
| `task`         | `tasks/`         | Todos and lightweight project management |

## Folder indexes

A folder becomes a knowledgebase folder node when it has both `index.md` and `index.toml`. The index pair is the document for that folder node. Folders without an index pair are structural folders or artifact folders. If only one of `index.md` or `index.toml` exists, validation reports an incomplete folder index pair.

`rebuild-indexes` may create `index.md` and `index.toml` for folders that contain document pairs or indexed child folders, because those folders have become part of the knowledgebase tree. It should not create indexes for purely artifact-only folders.

Each document-folder index describes that folder and lists direct child documents and direct child document subfolders. This lets Kataan assemble and render folder views quickly from stored metadata. Implementations may still walk the filesystem for validation and repair, but normal read paths should prefer loaded metadata.

Example `projects/index.toml`:

```toml
name = "Projects"
description = "Active and historical efforts with goals, owners, and outcomes."
default_type = "project"

folder_checksum = "blake3:..."

[[documents]]
slug = "kataan-redesign"
markdown = "kataan-redesign.md"
toml = "kataan-redesign.toml"
markdown_checksum = "blake3:..."
toml_checksum = "blake3:..."

[[subfolders]]
name = "company-x"
folder_checksum = "blake3:..."
```

Each `documents` entry identifies one direct non-index document in the folder. `slug` is the filename without `.md` or `.toml`, relative to the folder that owns the index. Each `subfolders` entry identifies one direct child document folder by name and records that child's recursive `folder_checksum`. A folder's own `index.md` and `index.toml` hash into that folder's checksum, not the parent's document list.

`index.toml` is system-managed. Humans may read it, but normal editing should happen through the application or agent tools so the index does not drift from the actual files. Validation should report any mismatch between `documents` and the files in the folder.

## Checksums

Each TOML sidecar that references a single Markdown file stores the Markdown filename and a BLAKE3 checksum of that Markdown file.

Example:

```toml
markdown = "my-amazing-project.md"
markdown_checksum = "blake3:9f86d081..."
```

This lets Kataan quickly detect whether the human-readable content changed since the TOML metadata or last index update.

Checksums are computed over exact file bytes. Kataan does not normalize line endings, strip BOMs, trim whitespace, or parse and re-serialize before hashing.

Each document folder `index.toml` stores a recursive Merkle-style folder checksum. The checksum is deterministic, post-order, and includes sorted direct documents plus sorted direct document-subfolder checksums. `code/` is excluded.

Conceptually:

```txt
folder_checksum = blake3(
  sorted entries of:
    "doc:{slug}:md:{md_checksum}"
    "doc:{slug}:toml:{toml_checksum}"
    "subfolder:{name}:{subfolder_checksum}"
)
```

The folder's own `index.md` and `index.toml` hash into the folder's checksum, not the parent's document list.

Kataan should include a `rebuild-indexes` command from the start. Rebuild fixes drift by recalculating document entries, subfolder entries, Markdown checksums, TOML sidecar checksums, and recursive folder checksums from the filesystem. Rebuild touches knowledgebase folders discovered from Markdown/TOML pairs and indexed child folders; purely artifact-only folders are left untouched. Rebuild does not auto-fix structural violations such as unresolved refs, unknown types, or depth violations; validation reports those.

Folder index document fields:

| Field               | Meaning                                                    |
| ------------------- | ---------------------------------------------------------- |
| `slug`              | Filename without `.md` or `.toml`, relative to the folder. |
| `markdown`          | Markdown filename for the document.                        |
| `toml`              | TOML sidecar filename for the document.                    |
| `markdown_checksum` | BLAKE3 checksum of the Markdown file.                      |
| `toml_checksum`     | BLAKE3 checksum of the document TOML sidecar.              |

Folder index subfolder fields:

| Field             | Meaning                                                        |
| ----------------- | -------------------------------------------------------------- |
| `name`            | Direct child folder name, relative to the folder owning index. |
| `folder_checksum` | Recursive checksum of that child document folder.              |

## Identity, references, and naming

Each document has a canonical ID based on its vault-relative Unix path without extension. IDs never have a leading slash, always use `/` separators even on Windows, and are normalized at load time.

Folder index documents use the folder path directly. Regular documents use `folder/slug`. Standalone files/artifacts are addressable by path for preview/download purposes, but they are not canonical document IDs and are not graph nodes.

```txt
projects
projects/company-x
projects/company-x/internal/q2-launch
topics/knowledge-bases
people/andrej-karpathy
```

Edges use canonical IDs, not bare filenames, to avoid collisions:

```toml
[edges]
related_to = ["topics/knowledge-bases"]
derived_from = ["intake/pasted-chat-about-ai-kbs"]
```

Filenames are preserved exactly as authored. Canonical IDs allow mixed-case URL-safe path segments, so externally meaningful names such as `projects/snappy/sows/otp-travel/HU-otp-travel-POC-SOW1-260429` are valid and map to files such as `HU-otp-travel-POC-SOW1-260429.md`. Kataan must not silently lowercase or rename user files. For documents, the Markdown file, TOML sidecar, and index entry all share the same slug and case.

Because canonical IDs are path-based and case-sensitive, rename and move operations must update the Markdown file, TOML sidecar, containing folder indexes, checksums, and references to the old canonical ID. Validation should detect case-insensitive collisions for cross-platform safety.

## UI routes

The web UI's route **is** the canonical id:

```txt
/<canonical-id>
/organizations/datasentics
/companies/snappy/customers/focusedenergy/docs/responsibility-split
```

Canonical ids are already URL-safe by construction (lowercase, digits, hyphens,
and `/`), so no encoding or lookup table is needed. A URL can be read, shared,
and pasted, and it survives a reload.

This replaces an earlier scheme of `/<type-folder>/<blake3-token>`. The token
was opaque and, being derived from the id, changed whenever a document was
renamed — silently breaking every link anyone had saved. It also forced a
translation step for internal links, which is the thing that made them not work
at all (see "Links between documents" below).

Resolve a filesystem path — or a canonical id, which is its extensionless form —
with:

```txt
GET /api/resolve-path?path=companies/snappy/customers/focusedenergy/docs/responsibility-split.md
```

## Links between documents

Documents link to each other the way files do, because that is what makes them
readable in an editor and on GitHub:

```markdown
See [DataSentics](datasentics.md) and [the split](../../docs/responsibility-split.md).
```

The server rewrites these when it renders Markdown to HTML. A link that resolves
to a document becomes that document's route and is marked so the UI can select
it without a page load; a link to a non-document file becomes a raw-file URL;
and a link that resolves to nothing is left exactly as the author wrote it, so a
dead link stays visibly dead rather than silently navigating somewhere wrong.

Relative segments are resolved against the linking document's folder before
lookup, and a path that would escape the vault is never rewritten.

## Relationship ontology

Document relationships live under one `[edges]` table. Keys are predicate names defined in `vault/ontology.toml`; values are target canonical IDs.

```toml
[edges]
works_at = ["companies/acme"]
contributes_to = ["projects/kataan-redesign"]
knows = ["people/alex-smith"]
```

Edges are directional and stored only on the source document. The query layer computes inverse and symmetric adjacency at load time from the ontology; inverse edges are never stored in document TOML.

`vault/ontology.toml` defines the relationship vocabulary:

```toml
schema_version = "0.1.0"

[edges.works_at]
from = ["person"]
to = ["company"]
inverse = "employs"
cardinality = "many-to-many"
description = "Person is employed by company."

[edges.related_to]
from = ["*"]
to = ["*"]
symmetric = true
cardinality = "many-to-many"
description = "Generic lateral relationship; use a more specific edge if one fits."
```

Predicate names use lowercase `snake_case`. Endpoint type lists are polymorphic; `"*"` means any type. Cardinality is advisory in v1 and exposed to the UI, but not enforced.

A predicate cannot be both `symmetric = true` and have an `inverse`. If `symmetric = true`, `from` and `to` must match.

Every predicate used in document `[edges]` must exist in `ontology.toml`. Validation checks source type, target resolution, target type, predicate shape, and ontology presence. Edges are written only by the UI or agent through the API write queue; humans should not hand-edit edge tables in TOML.

`kataan init` creates a default ontology with common people, organization, project, authorship, provenance, topical, and lateral predicates. Users may edit `vault/ontology.toml` directly. The ontology is vault-root configuration alongside `kataan.toml` and is not checksummed into the folder Merkle tree in v1.

Path structure remains the primary containment model. Ontology predicates such as `subproject_of`, `subtopic_of`, and `member_of` cover explicit containment. `related_to` is a symmetric ontology edge. `derived_from` records provenance. Folder nodes are documents via their `index.md` and `index.toml`, but their canonical IDs are the folder paths themselves, such as `projects` or `projects/company-x`, not `projects/index`. Ancestors are derived from canonical IDs.

## Core metadata fields

| Field               | Meaning                                                                                                                                                   |
| ------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `type`              | What kind of thing this file is: `intake`, `project`, `person`, `topic`, `note`, `code`, `type-definition`, etc.                                          |
| `status`            | Optional lifecycle state, such as draft, active, paused, done, or archived. Intake documents use `type = "intake"`; they do not need `status = "intake"`. |
| `markdown`          | Markdown file associated with this TOML sidecar.                                                                                                          |
| `markdown_checksum` | BLAKE3 checksum of the associated Markdown file.                                                                                                          |
| `aliases`           | Alternative names the human or agent can use to recognize this thing.                                                                                     |
| `labels`            | Lightweight tags for filtering and grouping, such as `aws`, `arm64`, `rust`, or `local-first`.                                                            |
| `edges`             | Relationship table keyed by ontology predicate name.                                                                                                      |
| `created_by`        | Who created the file: human or agent.                                                                                                                     |
| `last_updated_by`   | Who last changed the file: human or agent.                                                                                                                |

## Enums and lifecycle

Controlled values use lowercase kebab-case. Timestamps use RFC3339 UTC strings, for example `2026-04-28T12:00:00Z`.

Initial `status` values:

- `draft`
- `active`
- `paused`
- `done`
- `archived`

Typical lifecycle:

```txt
draft → active → done → archived
```

Not every document needs every state. Intake documents are identified by `type = "intake"`; they may omit `status` or use the normal lifecycle if archived.

Initial actor values for `created_by` and `last_updated_by`:

- `human`
- `agent`
- `system`

## Labels

Labels are lightweight tags that can be attached to any document. Path ancestors are also first-class searchable keywords from the user's perspective.

Example:

```toml
labels = ["aws", "arm64", "rust"]
```

Labels are different from types and topics:

- `type` controls what kind of document something is and which folder it belongs in.
- `topic` is a durable knowledge object with its own Markdown and TOML files.
- `label` is a lightweight marker used for filtering, grouping, search, and UI facets.

Label conventions:

- Labels use lowercase kebab-case.
- Labels are global across the vault.
- Labels do not control where a document lives.
- Labels may later be promoted into topics if they become important enough to deserve their own page.
- Ancestors are derived from the canonical ID at load time and are not stored in TOML.
- The query layer exposes a unified facet: `union(ancestors, labels)`.
- Filtering by `company-x` returns documents where `company-x` appears in either path ancestors or explicit labels.

## Types

Kataan has no fixed document type universe. The root `[type_folders]` table plus `type/` definition documents define the vault's type registry.

The default initializer may create starter types:

- `intake`
- `project`
- `person`
- `note`
- `topic`
- `type-definition`
- `code`

Users may define additional types such as `article`, `presentation`, `reference`, `finance`, and `task`. Every valid `type` value, starter or custom, must have a corresponding type definition in `type/` and a matching entry in root `[type_folders]`. Custom document types behave identically to starter document types at runtime: path-as-containment, ancestors-as-keywords, depth limits, sidecar TOML, validation, rebuild-indexes, and checksums all apply. `code` can be configured as a normal type, but folders/files under `code/` remain artifacts unless they opt into the knowledgebase model with Markdown/TOML pairs.

Type definitions live in `type/`.

To add a custom type, create `type/{name}.md` and `type/{name}.toml`, set `type = "type-definition"`, include `name` and `folders`, and optionally `icon`, add the corresponding `[type_folders]` entry in `vault/kataan.toml`, and create the target folder.

A type may claim more than one location. `folders` is a list of vault-root-relative path patterns, where `*` matches exactly one path segment and never crosses a `/`; there is no `**`. `folder = "projects"` remains accepted as an alias for `folders = ["projects"]`, so type definitions written before this existed parse unchanged.

```toml
folders = ["presentations", "companies/*/decks/*"]
```

A type may also extend another with `extends`, which forms the subtype relation. Wherever a type is matched against a set of permitted types — an edge's `from`/`to`, a `--type` query filter, a `subgraph` type filter — the match walks the `extends` chain, so a `customer` satisfies a rule written for `company`. A cycle in the chain is a validation error (`type-extends-cycle`).

Types do not have to be declared at the root. A folder's `index.toml` may carry its own `[type_folders]` table, whose keys are type names and whose values are patterns relative to **the declaring folder** (`"."` means that folder and its descendants). The declaration is additive for that subtree and invisible outside it, which is how the ontology grows at depth without the root config gaining an entry for every tree in the vault. A declaration that resolves outside its own subtree is rejected (`type-scope-escapes`), on the same reasoning as `type_folders` at the root: vaults are shared as git repositories, so these values are untrusted.

A document's type is legal at its path if *any* claim in scope matches — legality is a union, because a deck genuinely does belong in more than one place. The *default* type for a folder is the narrower question, and is taken from the nearest declaring scope. See `docs/kataan-type-scopes.md` for the full resolution rules.

`icon` is a Lucide icon export name such as `Inbox`, `Rocket`, `Newspaper`, `Presentation`, `BookOpen`, `ReceiptText`, or `ListTodo`. The UI should use a safe icon allowlist/map and fall back to a generic folder icon when the configured icon ID is unknown.

A type definition has a Markdown explanation and a TOML metadata file:

```txt
type/project.md
type/project.toml
```

Example `type/project.md`:

```md
# Project

A project is a time-bound effort with an intended outcome.
```

Example `type/project.toml`:

```toml
type = "type-definition"
name = "project"
folders = ["projects"]
icon = "Rocket"

markdown = "project.md"
markdown_checksum = "blake3:..."
```

## Intake metadata and provenance

Intake documents preserve original source material before transformation. An intake document is a normal document in the configured `intake/` folder and has a TOML sidecar.

Example `intake/pasted-chat-about-ai-kbs.toml`:

```toml
type = "intake"
source = "pasted-text"
source_label = "Pasted chat about AI knowledge bases"
ingested_at = "2026-04-28T12:00:00Z"

markdown = "pasted-chat-about-ai-kbs.md"
markdown_checksum = "blake3:..."

created_by = "human"
last_updated_by = "human"
```

Initial intake `source` values:

- `pasted-text`
- `url`
- `pdf`
- `image`
- `manual`
- `file`

Optional intake source detail fields:

| Field          | Meaning                                                                      |
| -------------- | ---------------------------------------------------------------------------- |
| `source_label` | Human-readable label for the source.                                         |
| `source_url`   | Original URL for `url` sources.                                              |
| `source_file`  | Original or attached file path for `pdf`, `image`, or `file` sources.        |
| `ingested_at`  | When Kataan saved the intake document.                                       |
| `retrieved_at` | When content was fetched from its origin; only meaningful for `url` sources. |

Example URL source:

```toml
source = "url"
source_url = "https://example.com/article"
retrieved_at = "2026-04-28T11:58:00Z"
ingested_at = "2026-04-28T12:00:00Z"
```

Organized documents derived from intake input should include a provenance edge:

```toml
[edges]
derived_from = ["intake/pasted-chat-about-ai-kbs"]
```

## Intake process

The most important workflow is new information intake.

Inputs may be:

- copied chats
- HTML pages
- articles
- research notes
- PDFs
- images
- random pasted text
- manually written notes

The user pastes intake content into a UI input box.

The process:

```txt
1. User pastes intake content.
2. System saves the original into intake/.
3. Agent analyzes the content.
4. Agent proposes where it belongs.
5. Agent suggests files, folders, types, and metadata.
6. Human reviews the proposal.
7. Human accepts, edits, rejects, or saves as intake only.
8. System writes Markdown, TOML, and attachments.
```

The agent should answer questions like:

- Does this belong in an existing file?
- Should this create a new file?
- Which folder should hold it?
- What type is it?
- Which people, projects, notes, or topics are mentioned?
- What relationships should be recorded?
- Should the intake source be preserved?
