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

## Vault structure

A Kataan vault is a normal directory:

```txt
vault/
├── index.toml
├── raw/
│   ├── index.toml
│   ├── pasted-chat-about-ai-kbs.md
│   └── pasted-chat-about-ai-kbs.toml
├── people/
│   ├── index.toml
│   ├── andrej-karpathy.md
│   └── andrej-karpathy.toml
├── projects/
│   ├── index.toml
│   ├── kataan-redesign.md
│   └── kataan-redesign.toml
├── notes/
│   ├── index.toml
│   ├── ai-compiled-knowledge-bases.md
│   └── ai-compiled-knowledge-bases.toml
├── topics/
│   ├── index.toml
│   ├── knowledge-bases.md
│   └── knowledge-bases.toml
└── type/
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
    ├── type-definition.md
    └── type-definition.toml
```

## Vault index

The vault root contains an `index.toml` file with vault-level metadata and schema information.

Example `vault/index.toml`:

```toml
schema_version = "0.1.0"
name = "My Kataan Vault"
created_at = "2026-04-28T12:00:00Z"
updated_at = "2026-04-28T12:30:00Z"

[type_folders]
raw = "raw"
project = "projects"
person = "people"
note = "notes"
topic = "topics"
type-definition = "type"
```

`schema_version` is required and gives Kataan a migration path as the vault format evolves. The `type_folders` table defines the authoritative type-to-folder mapping for the vault.

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
belongs_to = []
related_to = ["topics/knowledge-bases", "topics/ai-agents"]
sources = ["raw/pasted-chat-about-ai-kbs"]

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

Kataan uses a 1:1 mapping between core types and folders. A document with `type = "project"` lives in `projects/`, a document with `type = "person"` lives in `people/`, and so on. Validation should report any file whose location does not match its `type`.

Initial mappings:

| Type | Folder |
|---|---|
| `raw` | `raw/` |
| `project` | `projects/` |
| `person` | `people/` |
| `note` | `notes/` |
| `topic` | `topics/` |
| `type-definition` | `type/` |

## Folder indexes

Each folder has an `index.toml` file describing the purpose of the folder and listing the documents in that folder. This lets Kataan assemble and render folder views quickly by loading the folder index instead of scanning every file.

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
```

Each `documents` entry identifies one document in the folder. `slug` is the filename without `.md` or `.toml`, relative to the folder that owns the index.

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

Each folder `index.toml` stores a Merkle-tree-like folder checksum. The folder checksum is computed from the sorted list of child document checksums in that folder. For each document, both the Markdown file and its TOML sidecar contribute to the folder checksum.

Conceptually:

```txt
folder_checksum = blake3(
  "kataan-redesign:md:" + markdown_checksum + "\n" +
  "kataan-redesign:toml:" + toml_checksum + "\n" +
  "other-document:md:" + markdown_checksum + "\n" +
  "other-document:toml:" + toml_checksum + "\n"
)
```

`index.toml` itself does not contribute to `folder_checksum`; it stores the computed folder state. If subfolders are included later, their folder checksums can be included as child hashes using the same sorted, deterministic approach.

Kataan should include a `rebuild-indexes` command from the start. It recalculates document entries, Markdown checksums, TOML sidecar checksums, and folder checksums from the filesystem. On startup or scan, Kataan should detect checksum mismatches caused by direct human edits and refresh system-managed checksum and index fields.

Folder index document fields:

| Field | Meaning |
|---|---|
| `slug` | Filename without `.md` or `.toml`, relative to the folder. |
| `markdown` | Markdown filename for the document. |
| `toml` | TOML sidecar filename for the document. |
| `markdown_checksum` | BLAKE3 checksum of the Markdown file. |
| `toml_checksum` | BLAKE3 checksum of the document TOML sidecar. |

## Identity, references, and naming

Each document has a canonical ID based on its vault-relative path without extension:

```txt
projects/kataan-redesign
topics/knowledge-bases
people/andrej-karpathy
```

Relationship fields use canonical IDs, not bare filenames, to avoid collisions:

```toml
related_to = ["topics/knowledge-bases"]
sources = ["raw/pasted-chat-about-ai-kbs"]
```

Filenames and IDs use lowercase kebab-case. The Markdown file, TOML sidecar, and index entry all share the same slug.

Because canonical IDs are path-based, rename and move operations must update the Markdown file, TOML sidecar, containing folder indexes, checksums, and references to the old canonical ID.

## Relationship semantics

Relationship fields are directional and store canonical IDs. TOML relationships are authoritative for graph queries.

| Field | Direction | Meaning |
|---|---|---|
| `belongs_to` | child → parent | This document belongs to a broader document or container. |
| `related_to` | source → target | This document is laterally related to another document. |
| `sources` | derived → source | This document was derived from or informed by another document, often in `raw/`. |

`related_to` is stored on one document, but graph queries treat it as an undirected edge. If A lists B in `related_to`, queries for B's related documents should include A even if B does not list A.

Containment uses only `belongs_to`. There is no `has` field. Parent or container views are computed by finding documents whose `belongs_to` includes the parent ID.

Example:

```toml
belongs_to = ["projects/kataan-redesign"]
```

## Core metadata fields

| Field | Meaning |
|---|---|
| `type` | What kind of thing this file is: `raw`, `project`, `person`, `topic`, `note`, `type-definition`, etc. |
| `status` | Lifecycle state, such as active, paused, done, archived, draft, or raw. |
| `markdown` | Markdown file associated with this TOML sidecar. |
| `markdown_checksum` | BLAKE3 checksum of the associated Markdown file. |
| `aliases` | Alternative names the human or agent can use to recognize this thing. |
| `labels` | Lightweight tags for filtering and grouping, such as `aws`, `arm64`, `rust`, or `local-first`. |
| `belongs_to` | Parent relationship. |
| `related_to` | Lateral relationship. |
| `sources` | Raw or organized documents this document was derived from, using canonical IDs. |
| `created_by` | Who created the file: human or agent. |
| `last_updated_by` | Who last changed the file: human or agent. |

## Enums and lifecycle

Controlled values use lowercase kebab-case. Timestamps use RFC3339 UTC strings, for example `2026-04-28T12:00:00Z`.

Initial `status` values:

- `raw`
- `draft`
- `active`
- `paused`
- `done`
- `archived`

Typical lifecycle:

```txt
raw → draft → active → done → archived
```

Not every document needs every state. For example, a raw intake file may stay `raw`, and a topic may move directly from `draft` to `active`.

Initial actor values for `created_by` and `last_updated_by`:

- `human`
- `agent`
- `system`

## Labels

Labels are lightweight tags that can be attached to any document.

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

## Types

The initial `type` enum includes:

- `raw`
- `project`
- `person`
- `note`
- `topic`
- `type-definition`

The `type` enum includes both content types and system types. Every valid `type` must have a corresponding type definition in `type/`.

Type definitions live in `type/`.

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
status = "raw"
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

| Field | Meaning |
|---|---|
| `source_label` | Human-readable label for the source. |
| `source_url` | Original URL for `url` sources. |
| `source_file` | Original or attached file path for `pdf`, `image`, or `file` sources. |
| `ingested_at` | When Kataan saved the raw file. |
| `retrieved_at` | When content was fetched from its origin; only meaningful for `url` sources. |

Example URL source:

```toml
source = "url"
source_url = "https://example.com/article"
retrieved_at = "2026-04-28T11:58:00Z"
ingested_at = "2026-04-28T12:00:00Z"
```

Organized documents derived from raw input should include `sources`:

```toml
sources = ["raw/pasted-chat-about-ai-kbs"]
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
relationship = "related_to"
```

Initial action kinds:

- `create`: create a new Markdown/TOML document pair.
- `update`: change an existing document, such as appending a summary or editing metadata.
- `merge`: combine one document into another and update references.
- `link`: add a relationship between documents.
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

Agent changes should be diff-based and non-destructive. Agents should not silently overwrite human edits. If a file changed since the agent analyzed it, the proposal should be regenerated or shown as a conflict for human review.

## Raw vs organized knowledge

Raw content should be preserved before transformation.

```txt
raw/        original source material
notes/      curated notes
people/     people profiles
projects/   projects and efforts
topics/     durable concepts and themes
```

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
- no MCP requirement for now
- no complex database unless proven necessary

## Guiding principle

Raw input is preserved. Organized knowledge is curated. Humans and agents both participate. The filesystem remains the source of truth.
