# Kataan

Kataan is a simple, filesystem-native knowledge workspace where humans and agents collaborate to turn raw information into organized Markdown knowledge.

It is inspired by AI-compiled knowledge bases, but it is not AI-only. Humans can create, edit, and organize content directly. Agents help with intake, classification, restructuring, summarization, and maintenance.

## Core idea

Kataan treats a folder as the source of truth.

```txt
raw information → proposed structure → human review → organized knowledge
```

The system should be understandable without a database or proprietary format:

- Markdown files store readable content.
- TOML files store metadata.
- Folders provide human-friendly organization.
- Agents operate directly on the same files humans use.
- Raw input is preserved before it is transformed.
- The server keeps a metadata-only in-memory vault index; Markdown bodies are read on demand.

## Vault structure

A Kataan vault is a normal directory:

```txt
vault/
├── kataan.toml
├── ontology.toml
├── raw/
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
│   │   └── classify_raw.py
│   └── mcp-adapters/
│       └── README.md
└── type/
    ├── index.md
    ├── index.toml
    ├── raw.md
    ├── raw.toml
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

All folders except `code/` are document folders. Document folders use Markdown/TOML sidecars and folder index documents. `code/` is a tool/code asset tree and is intentionally exempt from document sidecar, folder index, loader, and Merkle rules.

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
raw = "raw"
project = "projects"
person = "people"
note = "notes"
topic = "topics"
code = "code"
type-definition = "type"
```

`schema_version` is required and identifies the vault format version. The `type_folders` table defines the authoritative type-to-folder mapping for the vault. `[limits].max_folder_depth` defaults to `4` and counts segments after the type folder, so `projects/a/b/c/foo` has depth `4`.

## File model

Each content item usually has two files:

```txt
my-amazing-project.md
my-amazing-project.toml
```

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
derived_from = ["raw/pasted-chat-about-ai-kbs"]

created_by = "human"
last_updated_by = "agent"
```

Attachments live next to the Markdown and TOML files when practical:

```txt
projects/
├── kataan-redesign.md
├── kataan-redesign.toml
├── kataan-redesign-sketch.png
└── kataan-redesign-brief.pdf
```

## Folder and type mapping

Kataan uses a 1:1 mapping between types and top-level folders. A document with `type = "project"` lives somewhere under `projects/`, a document with `type = "person"` lives somewhere under `people/`, and so on. Intermediate path segments are containment/search structure only; they do not change the document type. Validation reports any file whose top-level folder does not match its `type`.

Core mappings:

| Type              | Folder      |
| ----------------- | ----------- |
| `raw`             | `raw/`      |
| `project`         | `projects/` |
| `person`          | `people/`   |
| `note`            | `notes/`    |
| `topic`           | `topics/`   |
| `code`            | `code/`     |
| `type-definition` | `type/`     |

## Folder indexes

Every document folder, including intermediate document folders, has `index.md` and `index.toml`. The index pair is the document for that folder node; there are no untyped scaffolding folders in document trees. The special `code/` folder is not a document tree and does not require folder indexes.

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

Checksums are computed over exact raw file bytes. Kataan does not normalize line endings, strip BOMs, trim whitespace, or parse and re-serialize before hashing.

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

Kataan should include a `rebuild-indexes` command from the start. Rebuild fixes drift by recalculating document entries, subfolder entries, Markdown checksums, TOML sidecar checksums, and recursive folder checksums from the filesystem. Rebuild excludes `code/`. Rebuild does not auto-fix structural violations such as missing sidecars, unresolved refs, unknown types, or depth violations; validation reports those.

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

Folder index documents use the folder path directly. Regular documents use `folder/slug`.

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
derived_from = ["raw/pasted-chat-about-ai-kbs"]
```

Filenames are preserved exactly as authored. Canonical IDs allow mixed-case URL-safe path segments, so externally meaningful names such as `projects/snappy/sows/otp-travel/HU-otp-travel-POC-SOW1-260429` are valid and map to files such as `HU-otp-travel-POC-SOW1-260429.md`. Kataan must not silently lowercase or rename user files. The Markdown file, TOML sidecar, and index entry all share the same slug and case.

Because canonical IDs are path-based and case-sensitive, rename and move operations must update the Markdown file, TOML sidecar, containing folder indexes, checksums, and references to the old canonical ID. Validation should detect case-insensitive collisions for cross-platform safety.

## UI route locators

Canonical IDs remain the source-of-truth document identity. The web UI may expose shorter reloadable routes that preserve the current document without putting canonical IDs or filesystem-like paths in the browser route.

Route form:

```txt
/<type-folder>/<route-token>
/projects/8d8f2a41e5a6b07d9c948b9f7d6be2a1
```

`type-folder` is the visible top-level type folder, such as `projects`, `people`, or `topics`. `route-token` is derived from the canonical ID, for example the first 16 bytes of `blake3(canonical_id)` rendered as 32 lowercase hex characters. `LoadedVault` builds an in-memory lookup from `(type-folder, route-token)` to canonical ID. This lookup is only a UI locator, not a persistent identity field. Rename or move invalidates the old route; edits do not change it.

The API resolves locators explicitly, for example:

```txt
GET /api/resolve?type=projects&token=8d8f2a41e5a6b07d9c948b9f7d6be2a1
```

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

| Field               | Meaning                                                                                                                                          |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `type`              | What kind of thing this file is: `raw`, `project`, `person`, `topic`, `note`, `code`, `type-definition`, etc.                                    |
| `status`            | Optional lifecycle state, such as draft, active, paused, done, or archived. Raw documents use `type = "raw"`; they do not need `status = "raw"`. |
| `markdown`          | Markdown file associated with this TOML sidecar.                                                                                                 |
| `markdown_checksum` | BLAKE3 checksum of the associated Markdown file.                                                                                                 |
| `aliases`           | Alternative names the human or agent can use to recognize this thing.                                                                            |
| `labels`            | Lightweight tags for filtering and grouping, such as `aws`, `arm64`, `rust`, or `local-first`.                                                   |
| `edges`             | Relationship table keyed by ontology predicate name.                                                                                             |
| `created_by`        | Who created the file: human or agent.                                                                                                            |
| `last_updated_by`   | Who last changed the file: human or agent.                                                                                                       |

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

Not every document needs every state. Raw documents are identified by `type = "raw"`; they may omit `status` or use the normal lifecycle if archived.

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

The vault ships with a fixed core set:

- `raw`
- `project`
- `person`
- `note`
- `topic`
- `code`
- `type-definition`

Users may define additional types. Every valid `type` value, core or custom, must have a corresponding type definition in `type/` and a matching entry in root `[type_folders]`. Custom document types behave identically to built-in document types at runtime: path-as-containment, ancestors-as-keywords, depth limits, sidecar TOML, validation, and checksums all apply. `code` is the only core non-document type folder and is exempt from document sidecar/index/Merkle rules.

Type definitions live in `type/`.

To add a custom type, create `type/{name}.md` and `type/{name}.toml`, set `type = "type-definition"`, include `name` and `folder`, add the corresponding `[type_folders]` entry in `vault/kataan.toml`, and create the target folder.

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
folder = "projects"
icon = "rocket"

markdown = "project.md"
markdown_checksum = "blake3:..."
```

## Raw metadata and provenance

Raw files preserve the original source material before transformation. A raw file also has a TOML sidecar.

Example `raw/pasted-chat-about-ai-kbs.toml`:

```toml
type = "raw"
source = "pasted-text"
source_label = "Pasted chat about AI knowledge bases"
ingested_at = "2026-04-28T12:00:00Z"

markdown = "pasted-chat-about-ai-kbs.md"
markdown_checksum = "blake3:..."

created_by = "human"
last_updated_by = "human"
```

Initial raw `source` values:

- `pasted-text`
- `url`
- `pdf`
- `image`
- `manual`
- `file`

Optional raw source detail fields:

| Field          | Meaning                                                                      |
| -------------- | ---------------------------------------------------------------------------- |
| `source_label` | Human-readable label for the source.                                         |
| `source_url`   | Original URL for `url` sources.                                              |
| `source_file`  | Original or attached file path for `pdf`, `image`, or `file` sources.        |
| `ingested_at`  | When Kataan saved the raw file.                                              |
| `retrieved_at` | When content was fetched from its origin; only meaningful for `url` sources. |

Example URL source:

```toml
source = "url"
source_url = "https://example.com/article"
retrieved_at = "2026-04-28T11:58:00Z"
ingested_at = "2026-04-28T12:00:00Z"
```

Organized documents derived from raw input should include a provenance edge:

```toml
[edges]
derived_from = ["raw/pasted-chat-about-ai-kbs"]
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

The user pastes raw content into a UI input box.

The process:

```txt
1. User pastes raw content.
2. System saves the original into raw/.
3. Agent analyzes the content.
4. Agent proposes where it belongs.
5. Agent suggests files, folders, types, and metadata.
6. Human reviews the proposal.
7. Human accepts, edits, rejects, or saves as raw only.
8. System writes Markdown, TOML, and attachments.
```

The agent should answer questions like:

- Does this belong in an existing file?
- Should this create a new file?
- Which folder should hold it?
- What type is it?
- Which people, projects, notes, or topics are mentioned?
- What relationships should be recorded?
- Should the raw source be preserved?

## Agent runtime

Kataan includes a Rust-native `kataan-agent` crate for AI-assisted vault work. The first provider targets are API-key providers such as OpenAI and Anthropic. ChatGPT subscription / Codex-style OAuth is deferred behind the same provider-neutral trait.

The agent should use the smallest useful context: vault and folder indexes first, metadata and graph summaries next, full Markdown only when needed. Tool calls are represented as provider-neutral JSON-schema-described actions, but v1 agent output remains proposal-first and human-reviewed.

## Agent proposals

Agents should propose changes before writing organized knowledge. A proposal is a reviewable set of actions.

Example proposal shape:

```toml
source = "raw/pasted-chat-about-ai-kbs"
confidence = 0.82
rationale = "The input describes a project and mentions related knowledge-base topics."

[[actions]]
kind = "create"
target = "projects/kataan-redesign"
type = "project"

[[actions]]
kind = "update"
target = "topics/knowledge-bases"
operation = "append-summary"

[[actions]]
kind = "link"
from = "projects/kataan-redesign"
to = "topics/knowledge-bases"
predicate = "related_to"
```

Initial action kinds:

- `create`: create a new Markdown/TOML document pair.
- `update`: change an existing document, such as appending a summary or editing metadata.
- `merge`: combine one document into another and update references.
- `link`: add an ontology-backed edge between documents.
- `delete`: remove files from disk.

Prefer `status = "archived"` for normal removal from active views. The `delete` action is reserved for actual file removal and requires explicit human approval. Destructive actions such as `delete`, `merge`, or large rewrites must be reviewable before execution.

Future hardened proposal actions should include base checksums for each target so stale proposals can be detected before applying changes.

## Human and agent collaboration

Kataan is not an AI-only wiki.

Humans can:

- create files manually
- edit Markdown directly
- edit TOML metadata
- reorganize folders
- approve or reject agent proposals
- write original notes

Agents can:

- classify raw input
- create new files
- update existing files
- summarize long content
- extract people, projects, topics, and relationships
- restructure messy notes
- find duplicates
- suggest missing links
- run cleanup checks

The agent is a collaborator, not the owner.

Agent changes should be diff-based and non-destructive. Agents should not silently overwrite human edits. Agent proposals include base content hashes for each edited document; if the current hash no longer matches, the proposal is stale and the agent must re-read or present a conflict for human review.

## Concurrency and writes

Kataan uses a single-writer model in the server. API writes are serialized through a command queue and then update an `Arc<RwLock<LoadedVault>>` metadata index. Reads take a short read lock and should avoid holding it while reading large Markdown bodies.

`LoadedVault` is metadata-only: it stores config, ontology, type registry, document records, labels/facets, graph, checksums, diagnostics, and paths. It does not keep full Markdown bodies in memory. Markdown is read on demand from the file path in the document record.

Conflict detection is content-hash based. In-flight agent proposals carry the base hashes of documents they intend to edit. Apply-time recomputes current hashes and refuses or asks for re-read/review when a hash no longer matches.

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
6. If clean, enable the full read/write API.

The server checks folder depth on every write and rejects violations with `folder-depth-exceeded`. Edge writes support `add_edge`, `remove_edge`, and `replace_edges_for_predicate`; each mutation validates against `ontology.toml` before commit.

## MCP surface

MCP v1 is read + repair only:

- `read_document(id)`
- `list_folder(path)`
- `validate()`
- `rebuild_indexes()`

Writes and edge mutations go through proposal review or direct API calls from the UI, not direct MCP tool calls.

## Raw vs organized knowledge

Raw content should be preserved before transformation.

```txt
raw/        original source material
notes/      curated notes
people/     people profiles
projects/   projects and efforts
topics/     durable concepts and themes
code/       agent tools and executable helper code
```

The `code/` folder is a special typed folder for agent/tool code such as MCP adapters, TypeScript scripts, Python helpers, schemas, and executable utilities. It is not a Markdown/TOML document folder: Kataan does not require `.md`/`.toml` sidecars, does not require `index.md`/`index.toml`, does not load files in `code/` as documents, and excludes `code/` from folder Merkle checksums.

A raw file may later produce many organized files.

Example:

```txt
raw/karpathy-llm-knowledge-bases-article.md
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

JSON Schema describes the structural data model: required fields, optional fields, arrays, maps, and nested tables. Kataan-specific rules that depend on the current vault are returned as separate constraints, such as allowed types, allowed ontology predicates, folder/type mapping, valid status values, actor values, and the `code/` exemption.

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
code = "missing-toml-sidecar"
path = "projects/kataan-redesign.md"
message = "Markdown file is missing a matching TOML sidecar."
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
- intake input box
- agent proposal flow
- simple file browser
- MCP read/repair surface
- no complex database unless proven necessary

## Guiding principle

Raw input is preserved. Organized knowledge is curated. Humans and agents both participate. The filesystem remains the source of truth.
