# Kataan Search

> **Status (1.0).** Keyword search is **shipped** in the `kataan-search` crate:
> SQLite FTS5 / BM25 over Markdown documents, folder indexes, and plain text
> files (title, aliases, labels, type/status, path ancestors, body) with the
> field weighting and facets described below, exposed over the HTTP API, the
> Web UI, and the MCP `search` tool. The index is a rebuildable cache, not
> vault truth.
>
> **Not yet implemented (future):** PDF and binary metadata indexing, the
> `[search]` config table, and semantic/vector search. Sections describing
> those are design notes, not current behavior.

## Summary

Kataan search should start simple: a local, derived SQLite index using FTS5 for keyword search over vault documents and lightweight artifact metadata. PDFs are indexed by metadata only in v1 because they are usually generated outputs/artifacts, not source knowledge.

Semantic/vector search can be added later as an optional sidecar using USearch and Reciprocal Rank Fusion (RRF), but it should not be part of the first implementation.

## Goals

- Provide fast local search with no cloud dependency.
- Search Kataan Markdown documents, folder indexes, metadata, aliases, labels, type/status, and path ancestors.
- Search artifact filenames and lightweight metadata.
- Treat PDFs as artifacts and index metadata only by default.
- Keep the search index as a rebuildable cache, not vault truth.
- Support filters/facets for type, status, labels, ancestors, and result kind.
- Keep the first ranking model understandable: SQLite FTS5 BM25 plus explicit field weighting.

## Non-goals for v1

- No vector embeddings.
- No USearch index.
- No learned-to-rank model.
- No PDF full-text extraction or OCR.
- No indexing binary content beyond basic metadata.
- No remote search or hosted embedding APIs.

## Architecture

Search lives in a dedicated crate:

```txt
crates/kataan-search
```

Responsibilities:

- Build and update a local SQLite search database.
- Convert `LoadedVault` document records into searchable items.
- Walk artifact files while respecting Kataan ignore rules.
- Extract lightweight metadata for supported artifact types.
- Execute search queries and return ranked results/snippets/facets.

The server exposes this through API endpoints. The web UI consumes those endpoints.

The search index is derived cache data. It can be deleted and rebuilt at any time.

Recommended cache location:

```txt
$XDG_CACHE_HOME/kataan/search/<vault-hash>/search.sqlite
```

Do not store the search database in the vault by default.

## Indexed content

### Kataan documents

For Markdown+TOML document pairs and folder index documents, index:

- canonical ID
- Markdown body
- title/heading, where available
- aliases
- labels
- derived path ancestors
- type
- status
- TOML metadata fields that are useful for search
- outgoing edge target IDs and predicate names as metadata text

TOML sidecars should not appear as separate search results when they belong to a document. Their useful metadata is folded into the document result.

### Folder indexes

Folder `index.md` + `index.toml` pairs are searchable as folder results.

Index:

- folder canonical ID
- folder name/title
- folder Markdown body
- folder metadata
- derived ancestors/facets

### Files

A vault holds more than its documents. The files beside them — exported JSON
records, CSV extracts, configuration, source — were unsearchable, so the answer
to "where did that number come from?" was a `grep` outside the tool.

Text files are now indexed in full, as `kind = "file"` results, alongside
documents and folders. A file is indexed when all of these hold:

- Its extension is on the allow-list in `crates/kataan-search/src/files.rs`:
  data and markup (`txt`, `csv`, `tsv`, `json`, `yaml`, `yml`, `toml`, `xml`,
  `rst`, `org`, `html`, `htm`, `astro`, `svg`) and source files. An allow-list,
  not a deny-list — an unknown extension is far more likely to be a binary
  nobody wants in the index than a text format worth adding, and adding one is
  a line of code.
- It is not a generated lockfile (`Cargo.lock`, `package-lock.json`,
  `bun.lock`, …). These are large, they change constantly, and no one has ever
  wanted one as a search result.
- It is under 1 MiB and decodes as UTF-8. The cap keeps a single stray dump
  from dominating the index; the decode check is what actually excludes
  binaries whose extension slipped through.
- It is not part of a document pair. A document's own `.md` and `.toml` are
  already indexed as the document, and indexing them again would return the
  same content twice under two kinds.

Ignore rules (`.gitignore`-style, plus the vault's own ignores) apply, so
`node_modules`, `target`, and `.git` never reach the walk.

Indexed for each file: filename as the title, vault-relative path, extension,
containing folder ancestors, and the full text.

Not yet indexed: binaries, and the metadata (size, mtime, checksum) that a
metadata-only entry would carry. A file that cannot be read as text is skipped
entirely rather than entered as a name.

### PDFs

PDFs are metadata-only in v1.

Index:

- filename
- vault-relative path
- containing folder ancestors
- extension/media type
- file size
- mtime
- checksum
- optional PDF document info metadata if available:
  - title
  - author
  - subject
  - keywords
  - page count

Do not extract PDF body text by default. Most Kataan PDFs are expected to be generated outputs, so full-text PDF indexing would duplicate source material and add noise.

Future optional config may enable PDF text extraction:

```toml
[search]
pdf_text = false
```

## SQLite schema sketch

```sql
CREATE TABLE search_items (
  item_key TEXT PRIMARY KEY,
  kind TEXT NOT NULL,          -- document, folder (file reserved for future use)
  id TEXT,                     -- canonical document/folder ID when applicable
  path TEXT NOT NULL,
  title TEXT,
  type TEXT,
  status TEXT,
  extension TEXT,
);
-- The last-indexed timestamp lives in a small search_metadata(key, value)
-- table, not per row. Per-document checksums (and an extractor-version stamp
-- for cache invalidation) will be reintroduced when incremental indexing is
-- implemented.

CREATE TABLE search_facets (
  item_key TEXT NOT NULL,
  facet TEXT NOT NULL,
  PRIMARY KEY (item_key, facet)
);

CREATE INDEX search_items_kind_idx ON search_items(kind);
CREATE INDEX search_items_type_idx ON search_items(type);
CREATE INDEX search_items_status_idx ON search_items(status);
CREATE INDEX search_facets_facet_idx ON search_facets(facet);

CREATE VIRTUAL TABLE search_fts USING fts5(
  item_key UNINDEXED,
  title,
  path,
  aliases,
  facets,
  metadata,
  body,
  tokenize = 'unicode61 remove_diacritics 2'
);
```

`search_fts.item_key` links back to `search_items.item_key`.

## Ranking

Use SQLite FTS5 BM25 ranking with column weights.

Suggested priority:

1. title
2. aliases
3. path
4. facets
5. metadata
6. body

Example conceptual weighting:

```txt
title:    5.0
aliases:  4.0
path:     3.0
facets:   3.0
metadata: 2.0
body:     1.0
```

Ranking should remain deterministic and explainable. Avoid learned ranking in v1.

## Query behavior

Search should support:

- free-text query
- optional kind filter: `document`, `folder`
- optional type filter
- optional status filter
- optional facet filter
- optional path-prefix filter
- result limit/offset
- snippets for full-text matches
- facet counts for narrowing results

Empty query may return recent or all indexed items with filters applied, but this can be deferred.

## Server API

Add endpoints:

```txt
GET  /api/search?q=...
GET  /api/search/status
POST /api/search/reindex
```

Optional query params:

```txt
kind=document|folder
type=project
status=active
facet=company-x
path_prefix=projects/company-x
limit=20
offset=0
```

Response shape:

```ts
type SearchResponse = {
  query: string;
  mode: 'keyword';
  results: SearchResult[];
  facets: SearchFacetCount[];
};

type SearchResult = {
  kind: 'document' | 'folder';
  id?: string;
  path: string;
  title?: string;
  type?: string;
  status?: string;
  extension?: string;
  facets: string[];
  snippet?: string;
  score: number;
};

type SearchFacetCount = {
  facet: string;
  count: number;
};
```

## Web UI

Add a simple global search box to the existing read-only UI.

Initial UI behavior:

- Search input in sidebar/header.
- Results panel or route showing ranked results.
- Result cards show title, kind, path, type/status, facets, and snippet.
- Clicking a document opens its existing document route.
- Clicking a folder opens the folder view.
- Clicking a file opens the existing file preview when supported.
- Facet chips can narrow the result set.

No advanced query language is needed for v1.

## Index lifecycle

### Initial build

On server startup:

1. Load `LoadedVault`.
2. Open/create the search database for the vault.
3. Check index status.
4. If missing or stale, either rebuild automatically or report stale status and allow `POST /api/search/reindex`.

For the first implementation, manual rebuild is acceptable.

### Reindex command

`POST /api/search/reindex` rebuilds the whole index in one transaction:

1. Drop and recreate the tables (so the schema is always current).
2. Index every document from `LoadedVault`.
3. Write the `last_indexed_at` metadata and commit.

It returns `ok`, `index_path`, item/document/folder counts, and `indexed_at`.
(Artifact files are not indexed yet.)

### Incremental updates

Not yet implemented — reindex is a full rebuild triggered manually via
`POST /api/search/reindex`. The per-row checksum column that this design needs was
removed as unused scaffolding and will be reintroduced when incremental indexing
is built. The intended checksum-based flow:

- unchanged checksum: skip
- changed file: delete old item/chunks and reindex
- deleted file: delete search rows
- moved file/document: delete old key and insert new key

Until then, full reindex is simpler and acceptable.

## Configuration

Add optional root config later:

```toml
[search]
enabled = true
index_artifacts = true
index_text_artifacts = true
index_pdf_metadata = true
index_pdf_text = false
```

Defaults should keep search lightweight.

## Future v2: semantic/hybrid search

If keyword search is not enough, add optional semantic search:

- SQLite FTS5 remains the keyword/metadata index.
- USearch stores embedding vectors for chunks.
- A local embedding model creates query/document vectors.
- Results from FTS5 and USearch are fused with Reciprocal Rank Fusion.

Hybrid flow:

```txt
query
  -> SQLite FTS5 keyword results
  -> USearch vector results
  -> RRF fusion
  -> final ranked results
```

Use RRF before any learned fusion:

```txt
score(document) = sum(1 / (k + rank_i))
```

Default:

```txt
k = 60
```

Do not add learning-to-rank until there is enough judged query data and end-to-end evaluation.

## Implementation phases

### Phase 1: Core keyword search

- Add `kataan-search` crate or module.
- Add SQLite FTS5 schema and migrations.
- Build index from `LoadedVault` documents and folder indexes.
- Add simple query API in Rust.
- Add tests for document search, alias search, label/facet search, and path search.

### Phase 2: Server API

- Add `/api/search`.
- Add `/api/search/status`.
- Add `/api/search/reindex`.
- Ensure validate/rebuild can mark the search index stale or trigger reindex later.

### Phase 3: Artifact metadata

- Walk non-document files while respecting ignore rules.
- Index artifact filename/path/metadata.
- Add PDF metadata-only indexing.
- Add tests ensuring PDFs do not full-text index body content.

### Phase 4: Web UI

- Add global search input.
- Add result list UI.
- Add result navigation.
- Add facet narrowing.

### Phase 5: Incremental indexing

- Integrate with filesystem watcher once available.
- Update changed/deleted files by checksum.
- Keep full reindex as a fallback.

### Phase 6: Optional hybrid search

- Add embedding/chunking only if needed.
- Add USearch vector sidecar.
- Fuse keyword/vector rankings with RRF.
- Keep semantic search optional and local-only.

## Design decision

For Kataan v1, use **SQLite FTS5**.

Do **not** start with USearch. USearch is a strong future option for vector search, but Kataan first needs reliable local keyword search over Markdown, metadata, paths, labels, and artifact metadata. SQLite gives us that with less complexity and better fit for the current vault model.
