# Kataan Implementation Plan

This plan prioritizes the filesystem model first. The goal is to make the vault readable, validatable, and repairable before building the agent workflow or a rich UI.

## Phase 1: Core vault primitives

Implement in `crates/kataan-core`.

### 1. Path and ID model

- Define `CanonicalId` as `folder/slug` without extension.
- Define slug validation: lowercase kebab-case.
- Add helpers for:
  - ID → Markdown path
  - ID → TOML path
  - path → ID
  - type → folder

### 2. TOML and Markdown loading

- Load root `index.toml`.
- Load folder `index.toml` files.
- Load document TOML sidecars.
- Read associated Markdown files.
- Preserve unknown TOML fields where practical or avoid rewriting document TOML until needed.

### 3. Checksums

- Compute BLAKE3 over exact raw file bytes.
- Validate `markdown_checksum` in document TOML.
- Compute `toml_checksum` for folder index entries.
- Compute `folder_checksum` from sorted document checksum entries.

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

- All documents keyed by canonical ID.
- `belongs_to` edges as child → parent containment edges.
- Computed children views by reversing `belongs_to` edges at read time.
- `related_to` edges treated as undirected for traversal.
- `sources` edges as derived → source provenance edges.

This graph will support folder/project views, backlink-style navigation, agent proposal context, and future "suggest missing links" features.

## Phase 2: `kataan validate`

Expand `crates/kataan-cli` and `crates/kataan-core::validate`.

Validation should check:

- Root `index.toml` exists and has `schema_version`.
- Required folders exist.
- Every folder has `index.toml`.
- Every `.md` document has a matching `.toml` sidecar.
- Every document TOML has `markdown` pointing to the matching Markdown file.
- `markdown_checksum` matches exact Markdown bytes.
- Document `type` is known.
- Document path matches the type-folder mapping.
- Filenames and canonical IDs are lowercase kebab-case.
- `created_by` and `last_updated_by` are one of `human`, `agent`, `system` when present.
- `status` is one of the initial status values when present.
- Relationship refs in `belongs_to`, `related_to`, and `sources` resolve to existing canonical IDs, including broken `related_to` targets.
- `index.toml` document entries match files in the folder.
- Folder `markdown_checksum`, `toml_checksum`, and `folder_checksum` are correct.

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

## Phase 3: `kataan rebuild-indexes`

Implement repair for system-managed fields.

```sh
kataan rebuild-indexes <vault>
```

Should:

- Recompute document `markdown_checksum` fields.
- Rebuild every folder's `[[documents]]` list.
- Recompute folder `markdown_checksum`, `toml_checksum`, and `folder_checksum` values.
- Update `updated_at` in root `index.toml`.
- Preserve human-authored metadata where possible.

This command makes direct filesystem edits safe and keeps the filesystem canonical.

## Phase 4: `kataan init`

Add:

```sh
kataan init <path> --name "My Vault"
```

Should create:

- Root `index.toml`.
- Core folders: `raw`, `projects`, `people`, `notes`, `topics`, `type`.
- Folder `index.toml` files.
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

## Phase 8: Agent proposal flow

Only start this after validation, rebuild, and raw intake work reliably.

Initial proposal support:

- Analyze raw document.
- Suggest create/update/link actions.
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
12. Agent proposals.

## Near-term success criteria

The first meaningful milestone is:

```sh
kataan init /tmp/my-vault --name "My Vault"
kataan validate /tmp/my-vault
kataan rebuild-indexes /tmp/my-vault
kataan validate /tmp/my-vault
```

All commands should succeed, and the resulting vault should match the spec in `docs/kataan-brief.md`.
