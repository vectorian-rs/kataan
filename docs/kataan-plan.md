# Kataan Implementation Plan

This plan prioritizes the filesystem model first. The goal is to make the vault readable, validatable, and repairable before building the agent workflow or a rich UI.

## Phase 1: Core vault primitives

Implement in `crates/kataan-core`.

### 1. Path and ID model

- Define `CanonicalId` as a newtype around a normalized vault-relative Unix path without extension, e.g. `projects/company-x/internal/q2-launch`.
- Use no leading slash and `/` separators on every platform; normalize Windows paths at load time.
- Folder index documents use the folder path directly, e.g. `projects` and `projects/company-x`.
- Regular documents use folder path plus slug, e.g. `projects/company-x/q2-launch`.
- Define path segment validation: every segment is URL-safe and may use mixed case; preserve authored case exactly and never silently lowercase filenames or canonical IDs.
- Determine document type from the top-level folder only; intermediate segments do not change type.
- Load `[limits].max_folder_depth` from `vault/kataan.toml`; default to `4`.
- Count depth as segments after the type folder, so `projects/a/b/c/foo` has depth `4`.
- Add helpers for:
  - ID → Markdown path
  - ID → TOML path
  - path → ID
  - type → top-level folder
  - ID → ancestors/path keywords
  - ID → folder depth

### 2. TOML and Markdown loading

- Load root `kataan.toml`.
- Load folder index documents only where both `index.md` and `index.toml` exist; folders without an index pair are structural/artifact folders.
- Load first-class documents only from Markdown files that have exact matching TOML sidecars in the same folder.
- Build metadata-only `DocumentRecord` values containing paths, metadata, ancestors, facets, checksums, and folder-document marker.
- Do not load full Markdown bodies into `LoadedVault`; read Markdown on demand from `markdown_path`.
- Preserve unknown TOML fields where practical or avoid rewriting document TOML until needed.

### 3. Checksums

- Compute BLAKE3 over exact raw file bytes.
- Validate `markdown_checksum` in document TOML.
- Compute `toml_checksum` for folder index entries.
- Compute recursive post-order `folder_checksum` values from sorted direct documents and sorted direct indexed-subfolder checksums; artifact-only folders do not participate.
- Ensure every write path uses atomic write semantics: tempfile in the same directory, fsync, then rename/persist.

### 4. Type registry

- Load all type-to-folder mappings from root `kataan.toml` `[type_folders]`.
- Load type definitions from `type/`.
- Do not hard-code valid document types to starter folders only; the vault registry defines the type universe.
- Validate every type definition has:
  - `.md`
  - `.toml`
  - `type = "type-definition"`
  - `name`
  - `folder`
  - optional Lucide icon ID such as `Newspaper`, `Presentation`, `BookOpen`, `ReceiptText`, `ListTodo`, or `Inbox`
- Validate root `[type_folders]` matches type definitions.
- Treat Markdown/TOML pairs as the knowledgebase boundary. A mapped folder, including `code`, is an artifact tree until files opt into the document model with `.md` + `.toml` pairs or an `index.md` + `index.toml` pair.

### 5. Ontology

After type registry loading works, load and validate `vault/ontology.toml`.

Ontology validation should check:

- `ontology.toml` exists; otherwise report `missing-ontology`.
- Every predicate name is lowercase `snake_case`.
- Every predicate has non-empty `from`, `to`, and `cardinality` fields.
- A predicate cannot have both `inverse` and `symmetric = true`.
- If `symmetric = true`, `from` and `to` must be equal.
- Cardinality is one of `one-to-one`, `one-to-many`, `many-to-one`, or `many-to-many`.
- Endpoint lists may mention custom types that are not yet used; document validation checks actual source and target document types when edges are present.

### 6. In-memory vault graph

After document and ontology loading works, build an in-memory graph from loaded TOML metadata.

The graph should include:

- All documents keyed by canonical ID in a `HashMap<CanonicalId, DocumentRecord>`.
- Folder index documents keyed by folder path directly, not by `/index` suffix.
- `DocumentRecord` holds parsed TOML metadata, Markdown/TOML paths, checksums, facets, folder marker, and computed ancestors. Full Markdown is read on demand.
- Path containment edges derived from canonical ID ancestors.
- `Document` carries `edges: HashMap<PredicateName, Vec<CanonicalId>>` parsed from `[edges]`.
- Graph builds `inverse_edges: HashMap<CanonicalId, HashMap<PredicateName, Vec<CanonicalId>>>` from ontology inverse declarations.
- Symmetric predicates populate both directions under the same predicate name.
- Graph exposes `outgoing(id, predicate)`, `incoming(id, predicate)`, and `neighbors(id, predicate)`.
- Query code never asks documents to know their inverses; inverses always come from computed graph state.

This graph will support folder/project views, backlink-style navigation, agent proposal context, and future "suggest missing links" features.

### 7. Search facets

- Derive `ancestors` from canonical IDs at load time and store them on loaded `Document`; do not store them in TOML.
- Keep explicit `labels` from TOML unchanged across moves.
- Expose a unified facet set: `union(ancestors, labels)`.
- Filtering by a value such as `company-x` returns documents where that value appears in either ancestors or labels.
- UI should present one filter list and hide whether a match came from path or labels.

## Phase 2: `kataan validate`

Expand `crates/kataan-cli` and `crates/kataan-core::validate`.

Validation should check:

- Root `kataan.toml` exists and has `schema_version`.
- Required type folders from `[type_folders]` exist.
- Every mapped type folder has a matching type definition in `type/`, and every type definition has a matching `[type_folders]` entry.
- A folder with both `index.md` and `index.toml` is a folder node. A folder with only one of them reports an incomplete folder index pair. Folders with neither are structural/artifact folders unless rebuild promotes them because they contain document pairs or indexed child folders.
- Every Markdown+TOML document pair has TOML metadata with `markdown` pointing to the matching Markdown file.
- Standalone Markdown files and standalone TOML files are regular files/artifacts, not document nodes. They should be visible in file listings but not loaded into the graph.
- `markdown_checksum` matches exact Markdown bytes.
- Document `type` is known.
- Document top-level folder matches the type-folder mapping.
- Filenames and every canonical ID segment are preserved exactly as authored; mixed-case IDs such as `projects/snappy/sows/otp-travel/HU-otp-travel-POC-SOW1-260429` are valid.
- Canonical ID depth does not exceed `[limits].max_folder_depth`; report `folder-depth-exceeded`.
- `created_by` and `last_updated_by` are one of `human`, `agent`, `system` when present.
- `status` is one of the normal lifecycle values when present; `intake` is not a valid status.
- Every predicate in a document `[edges]` table exists in `vault/ontology.toml`.
- The source document type is allowed by the predicate `from` list, or `from = ["*"]`.
- Every edge target canonical ID resolves to an existing document.
- Every target document type is allowed by the predicate `to` list, or `to = ["*"]`.
- `index.toml` document entries include only valid Markdown+TOML document pairs and document-subfolder entries match child document folders.
- Folder `markdown_checksum`, `toml_checksum`, document-subfolder checksums, and recursive `folder_checksum` values are correct for indexed folder nodes.

CLI behavior:

```sh
kataan validate <vault>
```

- Exit `0` if valid.
- Exit `1` if errors exist.
- Print diagnostics with severity, machine-readable code, message, and optional path.

Diagnostic severities:

- `error`
- `warning`
- `info`

Diagnostic codes use lowercase kebab-case and should be stable for UI filters and tooling.

Example diagnostic codes:

- `missing-root-index`
- `missing-folder-index`
- `incomplete-document-pair`
- `missing-markdown-file`
- `invalid-type`
- `type-folder-mismatch`
- `checksum-mismatch`
- `unresolved-reference`
- `index-drift`
- `folder-depth-exceeded`
- `unknown-predicate`
- `predicate-source-type-mismatch`
- `predicate-target-type-mismatch`
- `unresolved-edge-target`
- `invalid-ontology-entry`
- `missing-ontology`

## Phase 3: `kataan rebuild-indexes`

Implement repair for system-managed fields.

```sh
kataan rebuild-indexes <vault>
```

Should:

- Recompute document `markdown_checksum` fields for valid Markdown+TOML document pairs.
- Rebuild every knowledgebase folder node's direct `[[documents]]` entries from valid pairs only and `[[subfolders]]` entries from indexed child folders.
- Create `index.md` and/or `index.toml` for folders that contain document pairs or indexed child folders and are therefore part of the knowledgebase tree.
- Recompute folder `markdown_checksum`, `toml_checksum`, document-subfolder checksums, and recursive `folder_checksum` values.
- Leave purely artifact-only folders untouched, including artifact-only folders under `code/`.
- Update `updated_at` in root `kataan.toml`.
- Preserve human-authored metadata where possible.

This command makes direct filesystem edits repairable and keeps system-managed indexes canonical. Rebuild fixes checksum/index drift only; it does not auto-fix invariant violations such as missing sidecars, unresolved refs, unknown types, edge/ontology errors, or depth violations.

## Phase 4: `kataan init`

Add:

```sh
kataan init <path> --name "My Vault"
```

Should create:

- Root `kataan.toml` with `[limits].max_folder_depth = 4`.
- Default `ontology.toml` with the core edge vocabulary.
- Starter folders: `intake`, `projects`, `people`, `notes`, `topics`, `code`, `type`.
- Folder `index.md` and `index.toml` files for starter knowledgebase folders. `code/` may be created as an artifact folder without an index pair.
- Starter type definitions:
  - `intake`
  - `project`
  - `person`
  - `note`
  - `topic`
  - `code`
  - `type-definition`

Then run `rebuild-indexes`.

Do not add personal/workflow-specific custom types such as `article`, `presentation`, `reference`, `finance`, or `task` to every default init. They are supported through manual `kataan.toml` `[type_folders]` entries and `type/{name}.md` + `type/{name}.toml` definitions, and may later be offered through richer presets.

## Phase 5: Read API and server

Implement in `crates/kataan-server` using `axum`.

Initial endpoints:

```txt
GET  /api/health
GET  /api/vault
GET  /api/folders
GET  /api/folders/:folder
GET  /api/documents/:id
POST /api/validate
POST /api/rebuild-indexes
```

The server should be thin. Most logic stays in `kataan-core`.

Boot sequence:

1. Load `vault/kataan.toml` and read limits.
2. Walk the vault and compute BLAKE3 checksums on all files.
3. Detect drift vs. stored checksums in folder indexes.
4. Validate structure, references, depth, and type-folder mapping.
5. If errors exist, serve read-only API plus diagnostics and expose rebuild.
6. If clean, enable the full read/write API.

Concurrency and live state model:

- Server state holds `Arc<RwLock<LoadedVault>>`.
- `LoadedVault` is metadata-only and stores config, type registry, ontology, document records, facets, graph, checksums, diagnostics, and paths.
- Markdown bodies are read on demand and should not be held in the loaded vault index.
- Single writer: API writes are serialized through an mpsc command queue.
- A successful write atomically changes files, rebuilds affected indexes/checksums, and updates or reloads `LoadedVault`.
- Agent proposals carry base content hashes for every document they intend to edit; apply-time recomputes current hashes and refuses or asks the agent to re-read when a hash no longer matches.
- Filesystem watcher events are debounced and batched. Clear changes patch the minimum affected metadata; ambiguous structural changes reload the whole `LoadedVault`.
- No cross-process file lock in v1; mutating CLI commands should be avoided while the server is running unless the watcher can observe/reconcile the changes.
- The API checks depth on every write and rejects violations with `folder-depth-exceeded`.
- Edge mutations are serialized through the same queue and validate against the ontology before commit.
- Edge writes support `add_edge`, `remove_edge`, and `replace_edges_for_predicate` operations.

## Phase 6: TOML schemas and repair guidance

Expose derived schemas for TOML data models so the UI and agents can guide users when metadata is missing or invalid.

Implement in `crates/kataan-core` and `crates/kataan-server`:

- Add a schema module backed by Rust data structs where practical.
- Derive or generate JSON Schema for:
  - `DocumentMetadata`
  - `FolderIndex`
  - `VaultConfig`
  - `TypeDefinition`
  - `Ontology`
  - `EdgePredicate`
- Expose API endpoints such as:

```txt
GET /api/schema/document
GET /api/schema/folder-index
GET /api/schema/vault
GET /api/schema/type-definition
GET /api/schema/ontology
```

Each response should include:

- JSON Schema for structural fields.
- A minimal TOML template.
- Vault-aware constraints such as allowed document types for a folder, valid statuses, valid actors, ontology predicates, and pair-based folder/document detection.

Schema responses are guidance, not a replacement for validation. Repair UI and agents should use them to propose fixes, then validation remains authoritative.

## Phase 7: Astro web UI

Implement in `apps/web`.

Initial UI:

- Vault overview.
- Folder list.
- Split second column per folder into `Documents` and `Files`.
- Documents list shows Markdown+TOML pairs as first-class nodes and hides matching TOML sidecars.
- Files list shows artifacts such as JSON, PDFs, spreadsheets, images, text files, standalone TOML, standalone Markdown, and any other non-document files.
- Markdown viewer for documents.
- File preview where practical.
- Keep source file content and syntax-highlighted presentation separate:
  - `GET /api/file?path=...` returns source content/metadata.
  - `GET /api/file/highlight?path=...` returns sanitized highlighted HTML for UI preview.
- Prefer highlighted HTML for supported text-like artifacts such as JSON, TOML, Markdown, Rust, TypeScript, JavaScript, Bash, YAML, and Python.
- Fall back to raw text, image preview, or metadata/download/open externally for unknown or binary files.
- Metadata panel.
- Validation panel.
- Button to run validation.
- Button to rebuild indexes.

Keep editing out of scope initially. Read-only UI first.

## Phase 8: Basic intake without agents

Before adding agent proposals, support manual intake.

```txt
paste text → save intake Markdown + TOML → rebuild indexes
```

Add endpoint/UI for:

- Pasted text.
- Source kind.
- Source label.

This creates an `intake` document with provenance metadata.

## Phase 9: Agent crate and proposal flow

Only start UI-driven proposal application after validation, rebuild, and intake work reliably. The crate structure can exist earlier so the data model, prompt, and provider boundary are explicit.

Implement in `crates/kataan-agent`.

Initial crate modules:

- `types`: provider-neutral message, content, model, usage, context, and tool types inspired by `pi-ai`, but reduced to Kataan's needs.
- `provider`: `AgentProvider` trait plus request/response structs.
- `providers/openai`: API-key OpenAI provider.
- `providers/anthropic`: API-key Anthropic provider.
- `prompt`: Kataan system prompt that instructs the model to read the smallest useful context.
- `context`: context-selection helpers that prefer vault summary, folder indexes, metadata, and graph summaries before full Markdown.
- `proposal`: structured create/update/link/archive actions for reviewable human approval.
- `event` / `loop`: evented agent loop modeled after `pi-agent`, suitable for streaming UI overlays.
- `tool`: executable tools with JSON Schema parameters and validation boundary.

Provider strategy:

- v1 ships with API-key providers: OpenAI and Anthropic.
- Keep `AgentProvider` provider-neutral so ChatGPT subscription / Codex-style OAuth can plug in later without spec changes.
- Represent tools with JSON Schema argument objects so they can be validated and serialized without TypeBox.
- MCP v1 exposes read + repair tools only: `read_document(id)`, `list_folder(path)`, `validate()`, and `rebuild_indexes()`.
- MCP v1 does not expose edge mutation tools.
- Writes and edge mutations go through proposal review or direct API calls from the UI, not direct MCP tool calls.

Initial proposal support:

- Analyze intake document.
- Analyze current selected document.
- Suggest create/update/link/archive actions.
- Show proposal to human.
- Apply accepted actions.
- Rebuild indexes after apply.

Defer hardened base-checksum conflict handling until the end-to-end flow works.

## Relationship v1 scope

Out of scope for v1:

- Edge attributes such as dates, roles, and weights. Design the parser so a future edge can be either a bare ID string or a table with `target`.
- Transitive reasoning over project/topic hierarchies.
- SPARQL/RDF integration.
- Cardinality enforcement.
- Ontology versioning beyond `schema_version`.

## Recommended build order

1. Rename root config from `index.toml` to `kataan.toml`.
2. Redesign `LoadedVault` around metadata-only `DocumentRecord` and on-demand Markdown reads.
3. Add `FacetIndex` for labels + ancestors + type/status filtering.
4. Server boot loads `LoadedVault` into `Arc<RwLock<LoadedVault>>`.
5. Add filesystem watcher with debounced minimal metadata updates and full-reload fallback.
6. `CanonicalId` and path helpers.
7. Vault and document loaders.
8. Checksum functions.
9. Type registry.
10. Load and validate `vault/ontology.toml`.
11. Full `kataan validate`, including edge and ontology checks.
12. `kataan rebuild-indexes`.
13. `kataan init`, including default `ontology.toml`.
14. In-memory vault graph, with at least one test exercising graph construction before the server depends on it.
15. Build inverse-edge adjacency map from ontology.
16. Server read API.
17. UI route locators: derive a 32-hex token from `blake3(canonical_id)`, index `(type-folder, token) -> canonical ID` in `LoadedVault`, expose `/api/resolve?type=...&token=...`, and use `/<type-folder>/<token>` for reloadable document URLs.
18. TOML schema API and repair guidance templates.
19. Read-only Astro UI.
20. Manual intake.
21. Edge mutation API.
22. `kataan-agent` crate skeleton and API-key provider boundary.
23. OpenAI/Anthropic ask commands.
24. Server `/api/agent` endpoint.
25. Global UI agent overlay.
26. MCP read/repair surface.
27. Agent proposals.

## Near-term success criteria

The first meaningful milestone is:

```sh
kataan init /tmp/my-vault --name "My Vault"
kataan validate /tmp/my-vault
kataan rebuild-indexes /tmp/my-vault
kataan validate /tmp/my-vault
```

All commands should succeed, and the resulting vault should match the spec in `docs/kataan-brief.md`.
