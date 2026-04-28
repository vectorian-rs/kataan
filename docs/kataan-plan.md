# Kataan Implementation Plan

This plan prioritizes the filesystem model first. The goal is to make the vault readable, validatable, and repairable before building the agent workflow or a rich UI.

## Phase 1: Core vault primitives

Implement in `crates/kataan-core`.

### 1. Path and ID model

- Define `CanonicalId` as a newtype around a normalized vault-relative Unix path without extension, e.g. `projects/company-x/internal/q2-launch`.
- Use no leading slash and `/` separators on every platform; normalize Windows paths at load time.
- Folder index documents use the folder path directly, e.g. `projects` and `projects/company-x`.
- Regular documents use folder path plus slug, e.g. `projects/company-x/q2-launch`.
- Define path segment validation: every segment is lowercase kebab-case.
- Determine document type from the top-level folder only; intermediate segments do not change type.
- Load `[limits].max_folder_depth` from `vault/index.toml`; default to `4`.
- Count depth as segments after the type folder, so `projects/a/b/c/foo` has depth `4`.
- Add helpers for:
  - ID → Markdown path
  - ID → TOML path
  - path → ID
  - type → top-level folder
  - ID → ancestors/path keywords
  - ID → folder depth

### 2. TOML and Markdown loading

- Load root `index.toml`.
- Load every folder's `index.md` and `index.toml` files, including intermediate folders.
- Load document TOML sidecars.
- Read associated Markdown files.
- Preserve unknown TOML fields where practical or avoid rewriting document TOML until needed.

### 3. Checksums

- Compute BLAKE3 over exact raw file bytes.
- Validate `markdown_checksum` in document TOML.
- Compute `toml_checksum` for folder index entries.
- Compute recursive post-order `folder_checksum` values from sorted direct documents and sorted direct subfolder checksums.
- Ensure every write path uses atomic write semantics: tempfile in the same directory, fsync, then rename/persist.

### 4. Type registry

- Load type definitions from `type/`.
- Validate every type has:
  - `.md`
  - `.toml`
  - `type = "type-definition"`
  - `name`
  - `folder`
- Validate root `[type_folders]` matches type definitions.

### 5. In-memory vault graph

After document loading works, build an in-memory graph from loaded TOML metadata.

The graph should include:

- All documents keyed by canonical ID in a `HashMap<CanonicalId, Document>`.
- Folder index documents keyed by folder path directly, not by `/index` suffix.
- `Document` holds parsed TOML metadata, Markdown content or lazy handle, and computed ancestors.
- Path containment edges derived from canonical ID ancestors.
- Optional `belongs_to` edges as explicit broader-parent relationships.
- Computed children views from path containment and, where requested, reversed `belongs_to` edges.
- `related_to` edges treated as undirected for traversal.
- `sources` edges as derived → source provenance edges.

This graph will support folder/project views, backlink-style navigation, agent proposal context, and future "suggest missing links" features.

### 6. Search facets

- Derive `ancestors` from canonical IDs at load time and store them on loaded `Document`; do not store them in TOML.
- Keep explicit `labels` from TOML unchanged across moves.
- Expose a unified facet set: `union(ancestors, labels)`.
- Filtering by a value such as `company-x` returns documents where that value appears in either ancestors or labels.
- UI should present one filter list and hide whether a match came from path or labels.

## Phase 2: `kataan validate`

Expand `crates/kataan-cli` and `crates/kataan-core::validate`.

Validation should check:

- Root `index.toml` exists and has `schema_version`.
- Required type folders exist.
- Every folder has `index.md` and `index.toml`.
- Every `.md` document has a matching `.toml` sidecar.
- Every document TOML has `markdown` pointing to the matching Markdown file.
- `markdown_checksum` matches exact Markdown bytes.
- Document `type` is known.
- Document top-level folder matches the type-folder mapping.
- Filenames and every canonical ID segment are lowercase kebab-case.
- Canonical ID depth does not exceed `[limits].max_folder_depth`; report `folder-depth-exceeded`.
- `created_by` and `last_updated_by` are one of `human`, `agent`, `system` when present.
- `status` is one of the normal lifecycle values when present; `raw` is not a valid status.
- Relationship refs in `belongs_to`, `related_to`, and `sources` resolve to existing canonical IDs, including broken `related_to` targets.
- `index.toml` document entries and subfolder entries match files in the folder.
- Folder `markdown_checksum`, `toml_checksum`, subfolder checksums, and recursive `folder_checksum` values are correct.

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
- `missing-toml-sidecar`
- `missing-markdown-file`
- `invalid-type`
- `type-folder-mismatch`
- `checksum-mismatch`
- `unresolved-reference`
- `index-drift`
- `folder-depth-exceeded`

## Phase 3: `kataan rebuild-indexes`

Implement repair for system-managed fields.

```sh
kataan rebuild-indexes <vault>
```

Should:

- Recompute document `markdown_checksum` fields.
- Rebuild every folder's direct `[[documents]]` and subfolder entries.
- Recompute folder `markdown_checksum`, `toml_checksum`, subfolder checksums, and recursive `folder_checksum` values.
- Update `updated_at` in root `index.toml`.
- Preserve human-authored metadata where possible.

This command makes direct filesystem edits repairable and keeps system-managed indexes canonical. Rebuild fixes checksum/index drift only; it does not auto-fix invariant violations such as missing sidecars, unresolved refs, unknown types, or depth violations.

## Phase 4: `kataan init`

Add:

```sh
kataan init <path> --name "My Vault"
```

Should create:

- Root `index.toml` with `[limits].max_folder_depth = 4`.
- Core folders: `raw`, `projects`, `people`, `notes`, `topics`, `type`.
- Folder `index.md` and `index.toml` files for every core folder.
- Core type definitions:
  - `raw`
  - `project`
  - `person`
  - `note`
  - `topic`
  - `type-definition`

Then run `rebuild-indexes`.

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

1. Load `vault/index.toml` and read limits.
2. Walk the vault and compute BLAKE3 checksums on all files.
3. Detect drift vs. stored checksums in folder indexes.
4. Validate structure, references, depth, and type-folder mapping.
5. If errors exist, serve read-only API plus diagnostics and expose rebuild.
6. If clean, enable the full read/write API.

Concurrency model:

- Single writer: API writes are serialized through an mpsc command queue drained by one task holding `&mut Vault`.
- Reads use `arc-swap<Vault>` for lock-free parallel access.
- Each successful write bumps a vault generation counter.
- Agent proposals carry the generation they were computed against; apply-time refuses if the counter advanced.
- No cross-process file lock in v1; document that the CLI should not mutate the vault while the server is running.
- The API checks depth on every write and rejects violations with `folder-depth-exceeded`.

## Phase 6: Astro web UI

Implement in `apps/web`.

Initial UI:

- Vault overview.
- Folder list.
- Document list per folder.
- Markdown viewer.
- Metadata panel.
- Validation panel.
- Button to run validation.
- Button to rebuild indexes.

Keep editing out of scope initially. Read-only UI first.

## Phase 7: Basic intake without agents

Before adding agent proposals, support manual raw intake.

```txt
paste text → save raw Markdown + TOML → rebuild indexes
```

Add endpoint/UI for:

- Pasted text.
- Source kind.
- Source label.

This creates a `raw` document with provenance metadata.

## Phase 8: Agent crate and proposal flow

Only start UI-driven proposal application after validation, rebuild, and raw intake work reliably. The crate structure can exist earlier so the data model, prompt, and provider boundary are explicit.

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
- Writes go through proposal review, not direct MCP tool calls.

Initial proposal support:

- Analyze raw document.
- Analyze current selected document.
- Suggest create/update/link/archive actions.
- Show proposal to human.
- Apply accepted actions.
- Rebuild indexes after apply.

Defer hardened base-checksum conflict handling until the end-to-end flow works.

## Recommended build order

1. `CanonicalId` and path helpers.
2. Vault and document loaders.
3. Checksum functions.
4. Type registry.
5. Full `kataan validate`.
6. `kataan rebuild-indexes`.
7. `kataan init`.
8. In-memory vault graph, with at least one test exercising graph construction before the server depends on it.
9. Server read API.
10. Read-only Astro UI.
11. Manual raw intake.
12. `kataan-agent` crate skeleton and API-key provider boundary.
13. OpenAI/Anthropic ask commands.
14. Server `/api/agent` endpoint.
15. Global UI agent overlay.
16. MCP read/repair surface.
17. Agent proposals.

## Near-term success criteria

The first meaningful milestone is:

```sh
kataan init /tmp/my-vault --name "My Vault"
kataan validate /tmp/my-vault
kataan rebuild-indexes /tmp/my-vault
kataan validate /tmp/my-vault
```

All commands should succeed, and the resulting vault should match the spec in `docs/kataan-brief.md`.
