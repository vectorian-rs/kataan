# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.0] - 2026-08-19

First stable release. Kataan is a local-first, plain-text knowledge vault where
the filesystem is the source of truth: Markdown holds content, TOML sidecars hold
metadata, links, and checksums, and everything is designed to be consumed and
mutated by both humans and agents.

### Vault model (`kataan-core`)

- Filesystem-native store: one `{slug}.md` + `{slug}.toml` pair per document,
  organized into type folders under a `kataan.toml` root config.
- Canonical ids with a stable grammar (mixed-case permitted), blake3 content
  checksums, and per-folder index files kept in sync by `rebuild_indexes`.
- Ontology-driven edge graph: predicates declare their legal `from`/`to` types,
  with inverse and symmetric edges derived at graph-build time.
- Crash-atomic writes for every file mutation.
- **Validated mutation layer** — `create_document`, `update_document`, and
  `add_edge` produce guaranteed-well-formed changes (correct id, ordered sidecar,
  refreshed checksums and indexes, ontology-legal edges) and reject bad requests
  (unknown type, id collision, illegal edge, invalid status) as distinct from
  on-disk corruption.

### Validation

- `kataan validate` reports structured diagnostics: checksum drift, folder/index
  document mismatches, status/actor enum violations, and illegal relationships.
- `--json` output for machine consumption.
- Vault walkers never follow symlinks; Markdown paths must be plain
  `{slug}.md` filenames.

### CLI (`kataan-cli`)

- `init`, `validate`, and `rebuild` commands.
- Logs go to stderr; command output goes to stdout (clean piping).

### HTTP server (`kataan-server`)

- Read API over the vault, including canonical-id query and `resolve` endpoints.
- Incremental filesystem watcher that maintains its fingerprint in place.
- Embedded web UI: with the `embed-ui` feature, a single binary serves both the
  API and the static SPA on one port.

### Web UI (`apps/web`)

- Astro single-page dashboard with folder navigation, a document reader, and
  search, talking to the server over `/api`.

### Search (`kataan-search`)

- SQLite FTS5 keyword search (BM25), with WAL and a busy timeout for safe
  concurrent access alongside reindexing.

### MCP server (`kataan-mcp`)

- Model Context Protocol server exposing the vault to MCP clients (Claude
  Desktop, IDE agents) as typed tools, over newline-delimited JSON-RPC on stdio —
  no SDK dependency.
- Read tools: `search`, `get_document`, `list_folders`, `get_folder`, `resolve`,
  `schema`, `vault_info` (return JSON).
- Write tools: `create_document`, `update_document`, `add_edge`, routed through
  the validated mutation layer, with the search index refreshed after each write.

### Tooling & CI

- `mise` + Bun for the toolchain; Prettier for the frontend.
- GitHub Actions running the `mise check` gate (fmt, clippy, tests, web checks).

[Unreleased]: https://github.com/vectorian-rs/kataan/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/vectorian-rs/kataan/releases/tag/v1.0.0
